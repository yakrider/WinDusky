// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr (
    all ( not(debug_assertions), target_os = "windows" ),
    windows_subsystem = "windows"
)]

use crate::win_dusky::hello_again;

mod win_dusky;


fn main() {

    println!("well, hello there!!");

    hello_again();

}
