// Prevents additional console window on Windows, DO NOT REMOVE!!
// we could make it only for release instead by using : all ( not(debug_assertions), target_os = "windows" )
#![cfg_attr (
    all (target_os = "windows"),
    windows_subsystem = "windows"
)]


use std::thread;
use std::time::Duration;


mod types;
mod keys;
mod dusky;    // <- sub-mods: overlay, hooks, hotkeys
mod config;
mod effects;
mod presets;
mod gamma;
mod auto;
mod luminance;
mod occlusion;
mod tray;
mod win_utils;



fn main() {

    let conf = config::Config::instance();

    // first we want to load/init the config, then get the non-blocking log-appender guard here in main
    // this ensures any pending logs get flushed when the guard scope is dropped (e.g force exit, crash etc)
    let _guard = conf.setup_log_subscriber();

    tracing::info! ("Initializing WinDusky ...");
    let wd = dusky::WinDusky::init(conf) .expect("ERROR initialising WinDusky");

    tracing::info! ("Starting WinDusky ...");

    thread::spawn (|| {
        tray::start_system_tray_monitor();
    });
    thread::sleep (Duration::from_millis(100));
    // ^^ we'll give a bit for tray to come up so it can absorb changes from dusky-startup below

    wd .start_win_dusky() .expect("ERROR running WinDusky");

}
