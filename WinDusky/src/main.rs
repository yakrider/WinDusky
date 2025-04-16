// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr (
    all ( not(debug_assertions), target_os = "windows" ),
    windows_subsystem = "windows"
)]


mod dusky;
mod effects;


fn main() {
    dusky::start_overlay().expect("ERROR running WinDusky");
}
