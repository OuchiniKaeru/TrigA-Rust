// リリースビルド（tauri build）時にWindowsでコンソール画面を出さない設定
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    automation_manager::run();
}