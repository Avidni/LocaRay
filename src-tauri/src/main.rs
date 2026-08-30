#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = locaray_lib::run() {
        eprintln!("LocaRay failed to start: {error}");
        std::process::exit(1);
    }
}
