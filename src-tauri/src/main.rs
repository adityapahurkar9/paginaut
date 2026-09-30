// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = paginaut_lib::installer_switch() {
        std::process::exit(code);
    }
    paginaut_lib::run();
}
