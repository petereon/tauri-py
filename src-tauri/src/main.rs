#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod gen;

use pyo3::prelude::*;
use std::sync::Mutex;
use tauri::{Builder, Manager};

use gen::{py_commands::*, state::state::AppState};

fn main() {
    // Initialize Python environment here
    Python::with_gil(|_| {
        Builder::default()
            .invoke_handler(tauri::generate_handler![greet, sum])
            .setup(|app| {
                app.manage(Mutex::new(AppState::default()));
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    });
}
