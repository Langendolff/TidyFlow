fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o TidyFlow");
}

#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
