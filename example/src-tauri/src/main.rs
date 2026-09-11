#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_jelto::init())
        .run(tauri::generate_context!())
        .expect("failed to run the example application");
}
