mod agent;
mod providers;
mod resolve;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(agent::Runs::default())
        .invoke_handler(tauri::generate_handler![
            agent::start_run,
            agent::stop_run,
            providers::check_providers,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Open Agent");
}
