// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr (
    all ( not(debug_assertions), target_os = "windows" ),
    windows_subsystem = "windows"
)]


use std::thread;


mod types;
mod dusky;
mod effects;
mod rules;
mod tray;
mod config;
mod keys;
mod win_utils;
mod luminance;

fn main() {

    let wd = dusky::WinDusky::instance();

    // we want the non-blocking log-appender guard to be here in main, to ensure any pending logs get flushed upon crash etc
    let _guard = wd.conf.setup_log_subscriber();

    tracing::info! ("Starting WinDusky ...");

    thread::spawn (|| {
        tray::start_system_tray_monitor();
    });

    wd .start_overlay_manager() .expect("ERROR running WinDusky");

}
