// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr (
    all ( not(debug_assertions), target_os = "windows" ),
    windows_subsystem = "windows"
)]

use std::thread;

mod dusky;
mod effects;
mod tray;


fn main() {

    let wd = dusky::WinDusky::instance();

    thread::spawn (|| {
        tray::start_system_tray_monitor();
    });

    wd .start_monitor() .expect("ERROR running WinDusky");

}
