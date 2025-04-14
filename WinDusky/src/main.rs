// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr (
    all ( not(debug_assertions), target_os = "windows" ),
    windows_subsystem = "windows"
)]


mod win_dusky;
mod color_matrices;


fn main() {

    println!("well, hello there!!");

    unsafe {
        win_dusky::run_it().expect("TODO: panic message");
    }

}
