use tauri_plugin_updater::UpdaterExt;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match handle.updater().expect("failed to build updater").check().await {
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
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
