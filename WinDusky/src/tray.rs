
#![allow (unused, non_snake_case, non_upper_case_globals)]

use image::{ImageFormat, ImageReader};

use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::EventLoopBuilderExtWindows;

use tracing::{error, warn};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

use crate::dusky::WinDusky;




const ICON_BYTES: &[u8] = include_bytes!("../WinDusky_128.png");


fn get_dusky_icon() -> Icon {
    let icon = ImageReader::with_format (Cursor::new (ICON_BYTES.to_vec()), ImageFormat::Png).decode().unwrap();
    Icon::from_rgba (icon.into_bytes(), 128, 128).unwrap()
}



// We'd like to have a way to update the tray menu entries upon internal events ..
// However, the menus have Rc internally, and cant be made Sync .. so they cant be stored in an instance etc .. jeez
// So instead, we make a proxy to pump custom events to the runloop, and store a clone of that ..
// Then we can send custom events for the menu to act on!

#[derive(Debug)]
pub enum DuskyEvent {
    MenuAction (MenuEvent),
    AutoOverlayEnable (bool),
    OverlayUpdate { n_active : usize },
    OverridesUpdate { n_overrides : usize },
}


static tray_events_proxy : LazyLock <Mutex <Option <EventLoopProxy <DuskyEvent>>>> = LazyLock::new (|| Mutex::new (None));


/// these will inject an internal event into sys-tray event-loop which will update tray-menu checkboxes etc
pub fn update_auto_overlay_enable (enabled: bool) {
    if let Some(proxy) = tray_events_proxy .lock() .unwrap() .as_ref() {
        let _ = proxy.send_event ( DuskyEvent::AutoOverlayEnable (enabled) );
    }
}
pub fn update_tray__overlay_count (n_active: usize) {
    if let Some(proxy) = tray_events_proxy .lock() .unwrap() .as_ref() {
        let _ = proxy.send_event ( DuskyEvent::OverlayUpdate { n_active } );
    }
}
pub fn update_tray__overrides_count (n_overrides: usize) {
    if let Some(proxy) = tray_events_proxy .lock() .unwrap() .as_ref() {
        let _ = proxy.send_event ( DuskyEvent::OverridesUpdate { n_overrides } );
    }
}


const MENU_ELEVATED         : &str = "is_elevated";
const MENU_AUTO_OV_ENABLED  : &str = "auto_overlay_enabled";
const MENU_ACTIVE_OVERLAYS  : &str = "active_overlays";
const MENU_USER_OVERRIDES   : &str = "user_overrides";
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
        MENU_EDIT_CONF        => "Edit Config",
        MENU_RESET_CONF       => "Reset Config",
        MENU_RESTART          => "Restart",
        MENU_QUIT             => "Quit",
        _ => "",
    }
}

fn exec_menu_action (id: &str) {
    let wd = WinDusky::instance();
    match id {
      //MENU_ELEVATED         => { /* always disabled */ },
        MENU_AUTO_OV_ENABLED  => { update_auto_overlay_enable (wd.rules.toggle_auto_overlay_enabled()) }
        MENU_ACTIVE_OVERLAYS  => { wd.clear_overlays() }
        MENU_USER_OVERRIDES   => { wd.rules.clear_user_overrides() }
        MENU_EDIT_CONF        => { wd.conf.trigger_config_file_edit() }
        MENU_RESET_CONF       => { wd.conf.trigger_config_file_reset() }
        MENU_RESTART          => { handle_restart_request(wd) }
        MENU_QUIT             => { wd.post_req__quit() }
        _ => { }
    };
}





pub fn start_system_tray_monitor() {

    let make_menu_item  = |id, enabled| MenuItem::with_id (id, menu_disp_str(id), enabled, None);
    let make_menu_check = |id, enabled, checked| CheckMenuItem::with_id (id, menu_disp_str(id), enabled, checked, None);

    let is_elev = crate::win_utils::check_cur_proc_elevated().unwrap_or_default();
    let elev_str = if is_elev { "Elevated : YES " } else { "Elevated : NO" };
    let elevated = CheckMenuItem::with_id (MENU_ELEVATED, elev_str, false, true, None);

    let auto_ov_enabled = make_menu_check (MENU_AUTO_OV_ENABLED, true, true);

    let active    = make_menu_check (MENU_ACTIVE_OVERLAYS, true, false);
    let overrides = make_menu_check (MENU_USER_OVERRIDES, true, false);

    let edit_conf  = make_menu_item (MENU_EDIT_CONF, true);
    let reset_conf = make_menu_item (MENU_RESET_CONF, true);

    let restart = make_menu_item (MENU_RESTART, true);
    let quit = make_menu_item (MENU_QUIT, true);

    let sep = PredefinedMenuItem::separator();

    let tray_menu = Menu::new();
    tray_menu .append_items ( &[
        &elevated, &auto_ov_enabled, &sep,
        &active, &overrides, &sep,
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

    *tray_events_proxy.lock() .unwrap() = Some (event_loop_proxy.clone());

    let proxy = event_loop_proxy.clone();
    MenuEvent::set_event_handler ( Some ( move |event:MenuEvent| {
        let _ = proxy.send_event (DuskyEvent::MenuAction(event));
    } ) );


    fn update_active_counts (n_active: usize, active: &CheckMenuItem) {
        active.set_text (format!("Active Overlays: {:?}", n_active));
        active.set_checked (n_active > 0);
    };
    fn update_overrides_counts (n_overrides: usize, overrides: &CheckMenuItem) {
        overrides.set_text (format!("User Overrides: {:?}", n_overrides));
        overrides.set_checked (n_overrides > 0)
    };


    let events_handler = move |event: DuskyEvent| {
        //tracing::debug!("{event:?}");
        match event {
            DuskyEvent::OverlayUpdate { n_active } => {
                update_active_counts (n_active, &active);
            }
            DuskyEvent::OverridesUpdate { n_overrides } => {
                update_overrides_counts (n_overrides, &overrides);
            }
            DuskyEvent::MenuAction (event) => {
                exec_menu_action (&event.id.0);
            }
            DuskyEvent::AutoOverlayEnable (enabled) => {
                auto_ov_enabled.set_checked (enabled);
                auto_ov_enabled.set_text (if enabled {"Auto Overlay Enabled"} else {"Enable Auto Overlay"})
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
        cmd .creation_flags ((DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).0);

        if let Ok (proc) = cmd .spawn() {
            warn! ("Launched a new WinDusky process with pid: {:?}", proc.id());
        }

        //thread::sleep (Duration::from_millis (100));
        //std::process::exit(0);

        wd.post_req__quit();

    });

}

