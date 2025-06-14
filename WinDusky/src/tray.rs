
#![allow (unused, non_snake_case, non_upper_case_globals)]

use image::{ImageFormat, ImageReader};

use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::EventLoopBuilderExtWindows;

use tracing::{error, warn};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS};

use crate::dusky::{self, WinDusky, MagEffect, MAG_EFFECT_IDENTITY};
use crate::effects::{ColorEffect};



const ICON_BYTES: &[u8] = include_bytes!("../WinDusky_128.png");


fn get_dusky_icon() -> Icon {
    let icon = ImageReader::with_format (Cursor::new (ICON_BYTES.to_vec()), ImageFormat::Png).decode().unwrap();
    Icon::from_rgba (icon.into_bytes(), 128, 128).unwrap()
}



// We'd like to have a way to update the tray menu entries upon internal events ..
// However, the menus have Rc internally, and cant be made Sync .. so they cant be stored in an instance etc .. jeez
// So instead, we make a proxy to pump custom events to the runloop, and store a clone of that ..
// Then we can send custom events for the menu to act on!

#[derive (Debug)]
pub enum DuskyEvent {
    MenuAction (MenuEvent),
    AutoOverlayEnable (bool),
    OverlayUpdate { n_active : usize },
    OverridesUpdate { n_overrides : usize },
    FullScreenMode { enabled: bool, effect: Option <ColorEffect>},
    MagLevel { level: Option <MagEffect>},
    GammaState { applied: bool, succeeded: bool, preset: Option <&'static str>},
}


static tray_events_proxy : OnceLock <EventLoopProxy <DuskyEvent>> = OnceLock::new();


/// these will inject an internal event into sys-tray event-loop which will update tray-menu checkboxes etc
pub fn update_tray__full_screen_mode (enabled:bool, effect: Option <ColorEffect>) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::FullScreenMode {enabled, effect} );
    }
}
pub fn update_tray__mag_level (level: Option <MagEffect>) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::MagLevel {level} );
    }
}
pub fn update_tray__auto_overlay_enable (enabled: bool) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::AutoOverlayEnable (enabled) );
    }
}
pub fn update_tray__overlay_count (n_active: usize) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::OverlayUpdate { n_active } );
    }
}
pub fn update_tray__overrides_count (n_overrides: usize) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::OverridesUpdate { n_overrides } );
    }
}
pub fn update_tray__gamma_state (applied:bool, succeeded:bool, preset: Option <&'static str>) {
    if let Some(proxy) = tray_events_proxy.get() {
        let _ = proxy.send_event ( DuskyEvent::GammaState { applied, succeeded, preset } );
    }
}



const MENU_ELEVATED         : &str = "is_elevated";
const MENU_AUTO_OV_ENABLED  : &str = "auto_overlay_enabled";
const MENU_ACTIVE_OVERLAYS  : &str = "active_overlays";
const MENU_USER_OVERRIDES   : &str = "user_overrides";
const MENU_FULL_SCREEN_MODE : &str = "full_screen_mode";
const MENU_FULL_SCREEN_EFF  : &str = "full_screen_effect";
const MENU_MAG_LEVEL        : &str = "mag_level";
const MENU_GAMMA_PRESET     : &str = "gamma_preset";
const MENU_EDIT_CONF        : &str = "edit_conf";
const MENU_RESET_CONF       : &str = "reset_conf";
const MENU_RESTART          : &str = "restart";
const MENU_QUIT             : &str = "quit";


fn menu_disp_str (id:&str) -> &str {
    match id {
      //MENU_ELEVATED         => "Elevated : ??",
        MENU_AUTO_OV_ENABLED  => "Auto Overlay Enabled",
        MENU_ACTIVE_OVERLAYS  => "Overlays : 0",
        MENU_USER_OVERRIDES   => "User Overrides : 0",
        MENU_FULL_SCREEN_MODE => "Enable Full Screen Effect",
        MENU_FULL_SCREEN_EFF  => "(Effect: None)",
        MENU_MAG_LEVEL        => "Magnification Level : None",
        MENU_GAMMA_PRESET     => "Gamma Preset: None",
        MENU_EDIT_CONF        => "Edit Config",
        MENU_RESET_CONF       => "Reset Config",
        MENU_RESTART          => "Restart",
        MENU_QUIT             => "Quit",
        _ => "",
    }
}

fn exec_menu_action (id: &str) {

    // Reminder that since tray runs in a separate thread, direct actions to dusky host/mag hwnds cant be triggered from here!!
    // .. ie all that has to go via posting messages to dusky thread msg queue

    let wd = WinDusky::instance();

    match id {
      //MENU_ELEVATED         => { /* always disabled */ },
        MENU_AUTO_OV_ENABLED  => { wd.auto.toggle_auto_overlay_enabled(); }
        MENU_ACTIVE_OVERLAYS  => { wd.post_req__overlay_clear_all(); }
        MENU_USER_OVERRIDES   => { wd.auto.clear_user_overrides(); }
        MENU_FULL_SCREEN_MODE => { wd.post_req__toggle_fs_mode(); }
        MENU_FULL_SCREEN_EFF  => { wd.post_req__toggle_fs_eff(); }
        MENU_MAG_LEVEL        => { wd.post_req__toggle_mag_level(); }
        MENU_GAMMA_PRESET     => { wd.toggle_gamma_active(); }
        MENU_EDIT_CONF        => { wd.conf.trigger_config_file_edit(); }
        MENU_RESET_CONF       => { wd.conf.trigger_config_file_reset(); }
        MENU_RESTART          => { handle_restart_request(wd); }
        MENU_QUIT             => { wd.post_req__quit(); }
        _ => { }
    };

}





pub fn start_system_tray_monitor() {

    let make_menu_item  = |id, enabled| MenuItem::with_id (id, menu_disp_str(id), enabled, None);
    let make_menu_check = |id, enabled, checked| CheckMenuItem::with_id (id, menu_disp_str(id), enabled, checked, None);

    let is_elev = crate::win_utils::check_cur_proc_elevated().unwrap_or_default();
    let elev_str = if is_elev { "Elevated : YES " } else { "Elevated : NO" };
    let elevated = CheckMenuItem::with_id (MENU_ELEVATED, elev_str, false, is_elev, None);

    let auto_ov_enabled  = make_menu_check (MENU_AUTO_OV_ENABLED, true, true);

    let active    = make_menu_check (MENU_ACTIVE_OVERLAYS, true, false);
    let overrides = make_menu_check (MENU_USER_OVERRIDES, true, false);

    let full_screen_mode = make_menu_check (MENU_FULL_SCREEN_MODE, true, false);
    let full_screen_eff  = make_menu_check (MENU_FULL_SCREEN_EFF, false, false);

    let mag_level = make_menu_check (MENU_MAG_LEVEL, true, false);

    let gamma_preset  = make_menu_check (MENU_GAMMA_PRESET, true, false);

    let edit_conf  = make_menu_item (MENU_EDIT_CONF, true);
    let reset_conf = make_menu_item (MENU_RESET_CONF, true);

    let restart = make_menu_item (MENU_RESTART, true);
    let quit = make_menu_item (MENU_QUIT, true);

    let sep = PredefinedMenuItem::separator();

    let tray_menu = Menu::new();
    tray_menu .append_items ( &[
        &elevated, &sep,
        &auto_ov_enabled, &active, &overrides, &sep,
        &full_screen_mode, &full_screen_eff, &sep,
        &mag_level, &sep,
        &gamma_preset, &sep,
        &edit_conf ,&reset_conf, &sep,
        &restart, &quit
    ] );


    let tray_icon = TrayIconBuilder::new()
        .with_menu (Box::new(tray_menu))
        .with_tooltip ("WinDusky")
        .with_icon (get_dusky_icon())
        .build() .unwrap();


    // we can now setup the event-loop to monitor events from the tray and tray-menu
    //let event_loop : EventLoop<DuskyEvent> = EventLoopBuilder::with_user_event().build();
    let event_loop : EventLoop<DuskyEvent> = EventLoopBuilder::with_user_event().with_any_thread(true).build();

    let event_loop_proxy = event_loop.create_proxy();

    let _ = tray_events_proxy .set (event_loop_proxy.clone());

    let proxy = event_loop_proxy.clone();
    MenuEvent::set_event_handler ( Some ( move |event:MenuEvent| {
        let _ = proxy.send_event (DuskyEvent::MenuAction(event));
    } ) );


    fn update_active_counts (n_active: usize, active: &CheckMenuItem) {
        active.set_text (format!("Active Overlays: {n_active:?}"));
        active.set_checked (n_active > 0);
    };
    fn update_overrides_counts (n_overrides: usize, overrides: &CheckMenuItem) {
        overrides.set_text (format!("User Overrides: {n_overrides:?}"));
        overrides.set_checked (n_overrides > 0)
    };


    let events_handler = move |event: DuskyEvent| {
        //tracing::debug!("{event:?}");
        match event {
            DuskyEvent::MenuAction (event) => {
                exec_menu_action (&event.id.0);
            }
            DuskyEvent::OverlayUpdate { n_active } => {
                update_active_counts (n_active, &active);
            }
            DuskyEvent::OverridesUpdate { n_overrides } => {
                update_overrides_counts (n_overrides, &overrides);
            }
            DuskyEvent::AutoOverlayEnable (enabled) => {
                auto_ov_enabled.set_checked (enabled);
                auto_ov_enabled.set_text (if enabled {"Auto Overlay Enabled"} else {"Enable Auto Overlay"})
            }
            DuskyEvent::FullScreenMode {enabled, effect} => {
                full_screen_mode.set_checked (enabled);
                full_screen_eff .set_enabled (enabled);
                full_screen_eff .set_checked (enabled && effect.is_some());
                full_screen_eff .set_text (format! ("(Effect: {:.50})", effect .map (|e| e.name()) .unwrap_or("None")));
                full_screen_mode.set_text (if enabled {"Full Screen Effect Enabled"} else {"Enable Full Screen Effect"});
                for menu in [&auto_ov_enabled, &active, &overrides] {
                    menu.set_enabled (!enabled)
                }
            }
            DuskyEvent::MagLevel { level } => {
                let level = level .unwrap_or (*MAG_EFFECT_IDENTITY);
                mag_level.set_checked (level.0 > 0);
                mag_level.set_text (format! ("Magnification Level : {:?}  ({:.2}x)", level.0, level.get()));
            }
            DuskyEvent::GammaState {applied, succeeded, preset} => {
                gamma_preset.set_checked (applied);
                let prefix = if succeeded {""} else {"❌ <- "};
                gamma_preset .set_text (format! ("{prefix}Gamma Preset: {:.50}", preset.unwrap_or("None")));
            }
        }
    };


    // now finally we can kick off the event loop itself .. (from which we will not return!)
    event_loop .run ( move |event, _win_target, control_flow| {

        *control_flow = ControlFlow::Wait;
        // ^^ default is Poll which isnt necessary for us

        if let Event::UserEvent(menu_ev) = event {
            events_handler (menu_ev)
        }

    } )

}





fn handle_restart_request (wd: &'static WinDusky) {

    thread::spawn (|| {

        warn! ("Attempting to Restart WinDusky !!");

        wd.post_req__un_register_hotkeys();
        thread::sleep (Duration::from_millis(100));

        let mut cmd = Command::new (std::env::current_exe().unwrap());
        cmd .creation_flags ((DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW).0);

        if let Ok (proc) = cmd .spawn() {
            warn! ("Launched a new WinDusky process with pid: {:?}", proc.id());
        }

        //thread::sleep (Duration::from_millis (100));
        //std::process::exit(0);

        wd.post_req__quit();

    });

}
