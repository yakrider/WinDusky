// Prevents additional console window on Windows, DO NOT REMOVE!!
// we could make it only for release instead by using : all ( not(debug_assertions), target_os = "windows" )
#![cfg_attr (
    all (target_os = "windows"),
    windows_subsystem = "windows"
)]


use std::thread;


mod types;
mod keys;
mod effects;
mod config;
mod dusky;    // <- sub-mods: overlay, hooks, hotkeys
mod auto;
mod luminance;
mod occlusion;
mod tray;
mod win_utils;




fn main() {

    let wd = dusky::WinDusky::init() .expect("ERROR initialising WinDusky");

    // we want the non-blocking log-appender guard to be here in main, to ensure any pending logs get flushed upon crash etc
    let _guard = wd.conf.setup_log_subscriber();

    tracing::info! ("Starting WinDusky ...");

    thread::spawn (|| {
        tray::start_system_tray_monitor();
    });

    wd .start_win_dusky() .expect("ERROR running WinDusky");

}
