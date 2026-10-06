pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("Yikhを起動できませんでした");
}
