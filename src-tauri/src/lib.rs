mod whisper;

use tauri_plugin_updater::UpdaterExt;
use whisper::WhisperState;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .manage(WhisperState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match handle
                    .updater()
                    .expect("failed to build updater")
                    .check()
                    .await
                {
                    Ok(Some(update)) => {
                        println!(
                            "Update available: {} -> {}",
                            update.current_version, update.version
                        );
                        if let Err(e) = update
                            .download_and_install(
                                |chunk_length, content_length| {
                                    println!(
                                        "Downloaded {} of {:?} bytes",
                                        chunk_length, content_length
                                    );
                                },
                                || {
                                    println!("Download finished");
                                },
                            )
                            .await
                        {
                            eprintln!("Failed to download and install update: {}", e);
                        }
                    }
                    Ok(None) => {
                        println!("No update available");
                    }
                    Err(e) => {
                        eprintln!("Failed to check for updates: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            whisper::whisper_get_status,
            whisper::whisper_check_deps,
            whisper::whisper_install,
            whisper::whisper_start_server,
            whisper::whisper_stop_server,
            whisper::whisper_health_check,
            whisper::whisper_update,
            whisper::whisper_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
