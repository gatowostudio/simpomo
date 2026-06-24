pub mod timer;

// 雛形(#1)の疎通確認用コマンド。フロント↔バックエンドの invoke が通ることだけを確認する。
// #3 でタイマー用コマンド/イベントに接続する際に削除する。
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
