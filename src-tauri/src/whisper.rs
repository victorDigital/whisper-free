use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperStatus {
    pub installed: bool,
    pub server_running: bool,
    pub server_healthy: bool,
    pub port: u16,
    pub model: String,
    pub install_path: String,
    pub binary_exists: bool,
    pub model_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressPayload {
    pub stage: String,
    pub message: String,
    pub done: bool,
    pub error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogPayload {
    pub source: String,
    pub message: String,
}

// ─── State ───────────────────────────────────────────────────────────────────

pub struct WhisperState {
    pub server_child: Mutex<Option<CommandChild>>,
    pub port: Mutex<u16>,
    pub model: Mutex<String>,
    pub installing: Mutex<bool>,
}

impl Default for WhisperState {
    fn default() -> Self {
        Self {
            server_child: Mutex::new(None),
            port: Mutex::new(8080),
            model: Mutex::new("base.en".to_string()),
            installing: Mutex::new(false),
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn whisper_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
    Ok(data_dir.join("whisper.cpp"))
}

fn whisper_binary(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(whisper_dir(app)?.join("build").join("bin").join("whisper-server"))
}

fn model_path(app: &AppHandle, model: &str) -> Result<PathBuf, String> {
    Ok(whisper_dir(app)?
        .join("models")
        .join(format!("ggml-{model}.bin")))
}

fn emit_progress(app: &AppHandle, stage: &str, message: &str, done: bool, error: bool) {
    let _ = app.emit(
        "whisper://progress",
        ProgressPayload {
            stage: stage.to_string(),
            message: message.to_string(),
            done,
            error,
        },
    );
}

/// Build a PATH that includes Homebrew and other common locations.
/// macOS GUI apps launched from Finder have a minimal PATH, so tools like
/// git, cmake, ffmpeg installed via Homebrew won't be found without this.
fn enhanced_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let extras = [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ];
    let mut parts: Vec<String> = extras.iter().map(|s| s.to_string()).collect();
    for p in current.split(':') {
        if !parts.contains(&p.to_string()) {
            parts.push(p.to_string());
        }
    }
    parts.join(":")
}

/// Run a shell command with the enhanced PATH, wait for completion, return output.
async fn run_cmd(
    app: &AppHandle,
    program: &str,
    args: &[&str],
    cwd: &PathBuf,
) -> Result<tauri_plugin_shell::process::Output, String> {
    app.shell()
        .command(program)
        .args(args)
        .current_dir(cwd.clone())
        .env("PATH", enhanced_path())
        .output()
        .await
        .map_err(|e| format!("Failed to run `{program}`: {e}"))
}

async fn check_health(port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    let addr = format!("127.0.0.1:{port}");
    // Run blocking TCP connect off the async runtime
    tauri::async_runtime::spawn_blocking(move || {
        TcpStream::connect_timeout(
            &addr.parse().unwrap(),
            Duration::from_secs(2),
        )
        .is_ok()
    })
    .await
    .unwrap_or(false)
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn whisper_get_status(
    app: AppHandle,
    state: tauri::State<'_, WhisperState>,
) -> Result<WhisperStatus, String> {
    let wdir = whisper_dir(&app)?;
    let binary = whisper_binary(&app)?;
    let model_name = state.model.lock().map_err(|e| e.to_string())?.clone();
    let mpath = model_path(&app, &model_name)?;
    let port = *state.port.lock().map_err(|e| e.to_string())?;

    let binary_exists = binary.exists();
    let model_exists = mpath.exists();
    let installed = binary_exists && model_exists;

    let server_running = {
        let child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
        child_lock.is_some()
    };

    let server_healthy = if server_running {
        check_health(port).await
    } else {
        false
    };

    Ok(WhisperStatus {
        installed,
        server_running,
        server_healthy,
        port,
        model: model_name,
        install_path: wdir.to_string_lossy().to_string(),
        binary_exists,
        model_exists,
    })
}

#[tauri::command]
pub async fn whisper_check_deps(app: AppHandle) -> Result<serde_json::Value, String> {
    let path_env = enhanced_path();
    let dummy_dir = std::env::temp_dir();

    let git_ok = app
        .shell()
        .command("git")
        .arg("--version")
        .env("PATH", &path_env)
        .current_dir(dummy_dir.clone())
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let cmake_ok = app
        .shell()
        .command("cmake")
        .arg("--version")
        .env("PATH", &path_env)
        .current_dir(dummy_dir.clone())
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let ffmpeg_ok = app
        .shell()
        .command("ffmpeg")
        .arg("-version")
        .env("PATH", &path_env)
        .current_dir(dummy_dir.clone())
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let cc_ok = app
        .shell()
        .command("cc")
        .arg("--version")
        .env("PATH", &path_env)
        .current_dir(dummy_dir)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    Ok(serde_json::json!({
        "git": git_ok,
        "cmake": cmake_ok,
        "ffmpeg": ffmpeg_ok,
        "cc": cc_ok,
        "all_ok": git_ok && cmake_ok && cc_ok,
    }))
}

#[tauri::command]
pub async fn whisper_install(
    app: AppHandle,
    state: tauri::State<'_, WhisperState>,
    model: Option<String>,
) -> Result<(), String> {
    // Prevent concurrent installs
    {
        let mut installing = state.installing.lock().map_err(|e| e.to_string())?;
        if *installing {
            return Err("Installation already in progress".to_string());
        }
        *installing = true;
    }

    let model_name = model.unwrap_or_else(|| "base.en".to_string());

    // Update the model in state
    {
        let mut m = state.model.lock().map_err(|e| e.to_string())?;
        *m = model_name.clone();
    }

    let result = do_install(&app, &model_name).await;

    // Clear installing flag
    {
        let mut installing = state.installing.lock().map_err(|e| e.to_string())?;
        *installing = false;
    }

    result
}

async fn do_install(app: &AppHandle, model_name: &str) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
    let wdir = data_dir.join("whisper.cpp");

    // Ensure data directory exists
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data dir: {e}"))?;

    // ── Step 1: Clone ────────────────────────────────────────────────────────
    if !wdir.exists() {
        emit_progress(app, "clone", "Cloning whisper.cpp repository...", false, false);

        let output = run_cmd(
            app,
            "git",
            &[
                "clone",
                "--depth",
                "1",
                "https://github.com/ggerganov/whisper.cpp.git",
            ],
            &data_dir,
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            emit_progress(app, "clone", &format!("Clone failed: {stderr}"), true, true);
            return Err(format!("git clone failed: {stderr}"));
        }
        emit_progress(app, "clone", "Repository cloned successfully", false, false);
    } else {
        emit_progress(
            app,
            "clone",
            "Repository already exists, skipping clone",
            false,
            false,
        );
    }

    // ── Step 2: CMake configure ──────────────────────────────────────────────
    emit_progress(
        app,
        "configure",
        "Configuring build with CMake...",
        false,
        false,
    );

    let mut cmake_args: Vec<&str> = vec!["-B", "build"];

    // Enable Metal acceleration on macOS
    if cfg!(target_os = "macos") {
        cmake_args.push("-DWHISPER_METAL=ON");
    }

    let output = run_cmd(app, "cmake", &cmake_args, &wdir).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            app,
            "configure",
            &format!("CMake configure failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("cmake configure failed: {stderr}"));
    }
    emit_progress(
        app,
        "configure",
        "CMake configured successfully",
        false,
        false,
    );

    // ── Step 3: CMake build ──────────────────────────────────────────────────
    emit_progress(
        app,
        "build",
        "Building whisper.cpp (this may take a few minutes)...",
        false,
        false,
    );

    let output = run_cmd(app, "cmake", &["--build", "build", "-j"], &wdir).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            app,
            "build",
            &format!("Build failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("cmake build failed: {stderr}"));
    }
    emit_progress(app, "build", "Build completed successfully", false, false);

    // ── Step 4: Download model ───────────────────────────────────────────────
    emit_progress(
        app,
        "model",
        &format!("Downloading model: {model_name}..."),
        false,
        false,
    );

    let output = run_cmd(
        app,
        "bash",
        &["models/download-ggml-model.sh", model_name],
        &wdir,
    )
    .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            app,
            "model",
            &format!("Model download failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("Model download failed: {stderr}"));
    }
    emit_progress(
        app,
        "model",
        &format!("Model {model_name} downloaded successfully"),
        false,
        false,
    );

    emit_progress(app, "done", "Installation complete!", true, false);

    Ok(())
}

#[tauri::command]
pub async fn whisper_start_server(
    app: AppHandle,
    state: tauri::State<'_, WhisperState>,
    port: Option<u16>,
) -> Result<(), String> {
    // Check if already running
    {
        let child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
        if child_lock.is_some() {
            return Err("Server is already running".to_string());
        }
    }

    let server_port = port.unwrap_or(8080);
    let model_name = state.model.lock().map_err(|e| e.to_string())?.clone();

    // Update port in state
    {
        let mut p = state.port.lock().map_err(|e| e.to_string())?;
        *p = server_port;
    }

    let wdir = whisper_dir(&app)?;
    let binary = whisper_binary(&app)?;
    let mpath = model_path(&app, &model_name)?;

    if !binary.exists() {
        return Err("whisper-server binary not found. Please install first.".to_string());
    }
    if !mpath.exists() {
        return Err(format!(
            "Model file not found: {}. Please install first.",
            mpath.display()
        ));
    }

    let port_str = server_port.to_string();
    let model_str = mpath.to_string_lossy().to_string();

    let (mut rx, child) = app
        .shell()
        .command(binary.to_string_lossy().as_ref())
        .args(["-m", &model_str, "--port", &port_str, "--host", "127.0.0.1"])
        .current_dir(wdir)
        .env("PATH", enhanced_path())
        .spawn()
        .map_err(|e| format!("Failed to start whisper-server: {e}"))?;

    // Store the child process handle
    {
        let mut child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
        *child_lock = Some(child);
    }

    // Spawn a background task to forward server output as events
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let msg = String::from_utf8_lossy(&line).to_string();
                    let _ = app_handle.emit(
                        "whisper://server-log",
                        LogPayload {
                            source: "stdout".to_string(),
                            message: msg,
                        },
                    );
                }
                CommandEvent::Stderr(line) => {
                    let msg = String::from_utf8_lossy(&line).to_string();
                    let _ = app_handle.emit(
                        "whisper://server-log",
                        LogPayload {
                            source: "stderr".to_string(),
                            message: msg,
                        },
                    );
                }
                CommandEvent::Terminated(payload) => {
                    let _ = app_handle.emit(
                        "whisper://server-stopped",
                        serde_json::json!({
                            "code": payload.code,
                            "signal": payload.signal,
                        }),
                    );
                    // Clear the child from state so status reflects reality
                    if let Some(ws) = app_handle.try_state::<WhisperState>() {
                        if let Ok(mut child_lock) = ws.server_child.lock() {
                            *child_lock = None;
                        }
                    }
                    break;
                }
                CommandEvent::Error(err) => {
                    let _ = app_handle.emit(
                        "whisper://server-log",
                        LogPayload {
                            source: "error".to_string(),
                            message: err,
                        },
                    );
                }
                _ => {}
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn whisper_stop_server(
    state: tauri::State<'_, WhisperState>,
) -> Result<(), String> {
    let mut child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
    if let Some(child) = child_lock.take() {
        child
            .kill()
            .map_err(|e| format!("Failed to kill server: {e}"))?;
        Ok(())
    } else {
        Err("Server is not running".to_string())
    }
}

#[tauri::command]
pub async fn whisper_health_check(
    state: tauri::State<'_, WhisperState>,
) -> Result<bool, String> {
    let port = *state.port.lock().map_err(|e| e.to_string())?;
    Ok(check_health(port).await)
}

#[tauri::command]
pub async fn whisper_update(
    app: AppHandle,
    state: tauri::State<'_, WhisperState>,
) -> Result<(), String> {
    // Stop server if running
    {
        let mut child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
        if let Some(child) = child_lock.take() {
            let _ = child.kill();
        }
    }

    let wdir = whisper_dir(&app)?;
    if !wdir.exists() {
        return Err("whisper.cpp is not installed".to_string());
    }

    // ── Step 1: Git pull ─────────────────────────────────────────────────────
    emit_progress(&app, "update", "Pulling latest changes...", false, false);

    let output = run_cmd(&app, "git", &["pull", "--ff-only"], &wdir).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            &app,
            "update",
            &format!("Git pull failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("git pull failed: {stderr}"));
    }
    emit_progress(&app, "update", "Pulled latest changes", false, false);

    // ── Step 2: CMake configure ──────────────────────────────────────────────
    emit_progress(
        &app,
        "build",
        "Reconfiguring build with CMake...",
        false,
        false,
    );

    let mut cmake_args: Vec<&str> = vec!["-B", "build"];
    if cfg!(target_os = "macos") {
        cmake_args.push("-DWHISPER_METAL=ON");
    }

    let output = run_cmd(&app, "cmake", &cmake_args, &wdir).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            &app,
            "build",
            &format!("CMake configure failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("cmake configure failed: {stderr}"));
    }

    // ── Step 3: CMake build ──────────────────────────────────────────────────
    emit_progress(
        &app,
        "build",
        "Rebuilding whisper.cpp...",
        false,
        false,
    );

    let output = run_cmd(&app, "cmake", &["--build", "build", "-j"], &wdir).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        emit_progress(
            &app,
            "build",
            &format!("Build failed: {stderr}"),
            true,
            true,
        );
        return Err(format!("Build failed: {stderr}"));
    }

    emit_progress(&app, "done", "Update complete!", true, false);
    Ok(())
}

#[tauri::command]
pub async fn whisper_remove(
    app: AppHandle,
    state: tauri::State<'_, WhisperState>,
) -> Result<(), String> {
    // Stop server if running
    {
        let mut child_lock = state.server_child.lock().map_err(|e| e.to_string())?;
        if let Some(child) = child_lock.take() {
            let _ = child.kill();
        }
    }

    let wdir = whisper_dir(&app)?;
    if wdir.exists() {
        std::fs::remove_dir_all(&wdir)
            .map_err(|e| format!("Failed to remove whisper.cpp: {e}"))?;
    }

    Ok(())
}
