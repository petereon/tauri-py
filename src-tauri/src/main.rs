#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod gen;

use pyo3::prelude::*;
use tauri::Builder;

use gen::py_commands::*;

fn main() {
    // Initialize Python environment here
    Python::with_gil(|_| {
        Builder::default()
            .invoke_handler(tauri::generate_handler![greet, sum])
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    });
}
