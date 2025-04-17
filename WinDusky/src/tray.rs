
#![allow(unused)]

use std::io::Cursor;
use std::sync::{LazyLock, Mutex};

use image::{ImageFormat, ImageReader};

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::EventLoopBuilderExtWindows;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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
pub enum DuskyTauriEvent {
    OverlayEvent { n_active : usize },
    MenuEvent (MenuEvent),
}


#[ allow (non_upper_case_globals) ]
static tray_events_proxy : LazyLock <Mutex <Option <EventLoopProxy <DuskyTauriEvent>>>> = LazyLock::new (|| Mutex::new (None));


#[ allow (non_snake_case) ]
/// this will inject an internal event into sys-tray event-loop which will update tray-menu checkboxes
pub fn update_tray__overlay_count (n_active: usize) {
    if let Some(proxy) = tray_events_proxy .lock() .unwrap() .as_ref() {
        let _ = proxy.send_event ( DuskyTauriEvent::OverlayEvent { n_active } );
    }
}



pub fn start_system_tray_monitor() {

    let is_elev = check_cur_proc_elevated().unwrap_or_default();
    let elev_txt = if is_elev { "Elevated : YES" } else { "Elevated : NO" };
    let elevated = CheckMenuItem::new (elev_txt, false, is_elev, None);

    let active = CheckMenuItem::new ("Overlays : 0", true, false, None);

    let quit = MenuItem::new ("Quit", true, None);

    let tray_menu = Menu::new();
    tray_menu .append_items ( &[ &elevated, &active, &quit ] );

    let tray_icon = TrayIconBuilder::new()
        .with_menu (Box::new(tray_menu))
        .with_tooltip ("WinDusky")
        .with_icon (get_dusky_icon())
        .build() .unwrap();


    // we can now setup the event-loop to monitor events from the tray and tray-menu
    //let event_loop : EventLoop<DuskyTauriEvent> = EventLoopBuilder::with_user_event().build();
    let event_loop : EventLoop<DuskyTauriEvent> = EventLoopBuilder::with_user_event().with_any_thread(true).build();

    let event_loop_proxy = event_loop.create_proxy();

    *tray_events_proxy.lock() .unwrap() = Some (event_loop_proxy.clone());

    let proxy = event_loop_proxy.clone();
    MenuEvent::set_event_handler ( Some ( move |event:MenuEvent| {
        let _ = proxy.send_event (DuskyTauriEvent::MenuEvent(event));
    } ) );


    fn update_active_counts (n_active: usize, active: &CheckMenuItem) {
        if n_active > 0 {
            active.set_checked (true);
            active.set_text (format!("Overlays: {:?}", n_active));
        } else {
            active.set_checked (false);
            active.set_text ("Overlays: 0");
        }
    };


    let events_handler = move |event: DuskyTauriEvent| {
        //println!("dusky-tauri-event: {event:?}");
        match event {
            DuskyTauriEvent::OverlayEvent { n_active } => {
                update_active_counts (n_active, &active);
            }
            DuskyTauriEvent::MenuEvent(event) => {
                if event.id == quit.id() {
                    std::process::exit(0);
                }
                else if event.id == active.id() {
                    let wd = WinDusky::instance();
                    if wd.overlays_count() > 0 {
                        wd.clear_overlays();
                    }
                    update_active_counts (0, &active);
                }
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



pub fn check_cur_proc_elevated () -> Option<bool> {
    check_proc_elevated ( unsafe { GetCurrentProcess() } )
}
pub fn check_proc_elevated (h_proc:HANDLE) -> Option<bool> { unsafe {
    let mut h_token = HANDLE::default();
    if OpenProcessToken (h_proc, TOKEN_QUERY, &mut h_token) .is_err() {
        return None;
    };
    let mut token_info : TOKEN_ELEVATION = TOKEN_ELEVATION::default();
    let mut token_info_len = size_of::<TOKEN_ELEVATION>() as u32;
    GetTokenInformation (
        h_token, TokenElevation, Some(&mut token_info as *mut _ as *mut _),
        token_info_len, &mut token_info_len
    ) .ok()?;
    Some (token_info.TokenIsElevated != 0)
} }
