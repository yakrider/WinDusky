#![ allow (non_snake_case) ]

//use no_deadlocks::RwLock;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::thread;
use std::time::Duration;
use tracing::{error, info};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_NOREPEAT};
use windows::Win32::UI::Magnification::{MagInitialize, MagSetColorEffect, MagSetWindowSource, MagUninitialize, MAGCOLOREFFECT, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindow, GetWindowLongW, KillTimer, PostMessageW, PostThreadMessageW, RegisterClassExW, SetTimer, SetWindowPos, CS_HREDRAW, CS_VREDRAW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, GW_HWNDPREV, HCURSOR, HICON, HWND_TOP, HWND_TOPMOST, MSG, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_HOTKEY, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};

use crate::{config, effects::*, rules, tray, types::*};
use crate::config::HotKey;
use crate::rules::RulesResult;



const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host Window";

const TIMER_TICK_MS : u32 = 16;

const HOTKEY_ID__TOGGLE      : usize = 1;
const HOTKEY_ID__NEXT_EFFECT : usize = 2;
const HOTKEY_ID__PREV_EFFECT : usize = 3;

const WM_APP__REQ_UPDATE         : u32 = WM_APP + 1;
const WM_APP__REQ_CREATE_OVERLAY : u32 = WM_APP + 2;



#[derive (Default, Debug)]
struct Overlay {
    host   : Hwnd,
    mag    : Hwnd,
    target : Hwnd,

    effect : ColorEffectAtomic,
    marked : Flag,
    is_top : Flag,
}



//#[derive (Debug)]
pub struct WinDusky {

    pub conf  : &'static config::Config,
    pub rules : &'static rules::RulesMonitor,

    thread_id  : AtomicU32,
    overlays   : RwLock <HashMap <Hwnd, Overlay>>,
    cur_timer  : AtomicUsize,
    ov_topmost : HwndAtomic,
}




impl Overlay {

    fn new (target:Hwnd, effect:ColorEffect) -> Result <Overlay, String> { unsafe {

        let h_inst : Option<HINSTANCE> = GetModuleHandleW(None) .ok() .map(|h| h.into());

        // Create the host for the magniier control
        let Ok(host) = CreateWindowExW (
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw (wide_string(HOST_WINDOW_CLASS_NAME).as_ptr()),
            PCWSTR::from_raw (wide_string(HOST_WINDOW_TITLE).as_ptr()),
            WS_POPUP, 0, 0, 0, 0, None, None, h_inst, None
        ) else {
            return Err(format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
        };

        // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
        let Ok(mag) = CreateWindowExW (
            WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, Some(host), None, h_inst, None,
        ) else {
            return Err(format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
        };

        // we have enough to create the new overlay now
        let overlay = Overlay {
            host   : host.into(),
            mag    : mag.into(),
            target,
            effect : ColorEffectAtomic::new (effect),
            marked : Flag::new(false),
            is_top : Flag::new(false),
        };

        // we'll apply the default smart inversion color-effect .. can ofc be cycled through via hotkeys later
        apply_color_effect (mag, overlay.effect.get());

        // we'll mark the overlay which will make our main loop timer-handler sync dimensions and position with the target
        overlay.marked.set();

        Ok(overlay)
    } }


    pub fn update (&self, wd: &WinDusky) { unsafe {

        //println!("updating overlay {:?}", self);

        // lets clear the flag upfront before we start changing stuff (so it can be marked dirtied in the mean time)
        self.marked.clear();

        // we'll size both the host and mag to fit the target hwnd when hotkey was invoked

        let mut rect = RECT::default();
        let (host, mag, target) = (self.host.into(), self.mag.into(), self.target.into());

        //let _ = GetWindowRect (fgnd, &mut rect) .is_err();
        // ^^ getting window-rect incluedes (often transparent) padding, which we dont want to invert, so we'll use window frame instead
        if DwmGetWindowAttribute (target, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as _, size_of::<RECT>() as u32) .is_err() {
            error!( "DwmGetWindowAttribute (frame) on target failed with error: {:?}", GetLastError());
        }
        let (x, y, w, h) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);

        if MagSetWindowSource (mag, rect) .as_bool() == false {
            error!( "MagSetWindowSource on mag-hwnd failed with error: {:?}", GetLastError());
        }
        if SetWindowPos (mag, None, 0, 0, w, h, Default::default()) .is_err() {
            error!( "SetWindowPos (w,h) on mag-hwnd failed with error: {:?}", GetLastError());
        }

        // for overlay host z-positioning .. we want the overlay to usually be just above the target hwnd, but not topmost
        // (the hope is to keep maintaining that such that other windows can come in front normally as well)
        // however .. while its fgnd, we'll make it top to avoid flashing etc (while the host and target switch turns being in front)
        // (and so then to keep these from lingering on top, we've added also sanitation to event listener itself)

        let fgnd : Hwnd = GetForegroundWindow().into();
        if self.target == fgnd {
            // now if some other overlay was previously on-top, we'll want to un-top it first
            let ov_top = wd.ov_topmost.load();
            if ov_top.is_valid() && ov_top != self.target {
               let overlays = wd.overlays.read().unwrap();
               if let Some(overlay) = overlays .get (&ov_top) {
                   overlay.resync_ov_z_order()
               }
            }
        }

        // now first, we'll do the general z-order repositioning
        let hwnd_insert = GetWindow (target, GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (host, None,               x, y, w, h,  SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW);
        let _ = SetWindowPos (host, Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOREDRAW);
        // ^^ the two step appears necessary, as w hwnd-insert specified, it doesnt seem to move/reposition the window!

        // next, if we are actually fgnd, we'll also try and set topmost (which OS might or might not always allow)
        if self.target == fgnd {
            let _ = SetWindowPos (host, Some(HWND_TOPMOST),  0, 0, 0, 0,  SWP_NOMOVE | SWP_NOSIZE);
            self.is_top.set();
            wd.ov_topmost.store(target);
        }

    } }

    pub fn resync_ov_z_order (&self) { unsafe {
        self.is_top.clear();
        let hwnd_insert = GetWindow (self.target.into(), GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (self.host.into(), Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE);
    } }

}


impl Drop for Overlay {
    fn drop (&mut self) { unsafe {
        let _ = DestroyWindow (self.host.into());
    } }
}





impl WinDusky {

    pub fn instance() -> &'static WinDusky {
        static INSTANCE : OnceLock <WinDusky> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            WinDusky {
                conf  : config::Config::instance(),
                rules : rules::RulesMonitor::instance(),

                thread_id  : AtomicU32::default(),
                overlays   : RwLock::new (HashMap::default()),
                cur_timer  : AtomicUsize::default(),
                ov_topmost : HwndAtomic::default(),
            }
        )
        // ^^ NOTE that init is not called here, and the user should do so at their own convenience !!
    }


    pub fn start_overlay_manager (&self) -> Result<(), String> { unsafe {

        if self.thread_id.load(Ordering::Acquire) != 0 {
            // if we already have a thread-id, we must have already initied
            return Ok(())
        };
        self.thread_id.store (GetCurrentThreadId(), Ordering::Release);

        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Initialize Magnification API
        if MagInitialize() == FALSE {
            return Err(format!("MagInitialize failed with error: {:?}", GetLastError()));
        }


        // Register Host Window Class
        let Ok(instance) = GetModuleHandleW(None) else {
            return Err(format!("GetModuleHandleW failed with error: {:?}", GetLastError()));
        };
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(host_window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR::from_raw (wide_string(HOST_WINDOW_CLASS_NAME).as_ptr()),
            hIconSm: HICON::default(),
        };
        if RegisterClassExW(&wc) == 0 {
            if GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(format!("RegisterClassExW failed with error: {:?}", GetLastError()));
            }
        }

        self.register_hotkeys();

        self.rules.load_conf_rules (self.conf);

        // lets also setup a win-event hook to monitor fgnd change so we can maintain the overlay z-ordering
        /*
            We want to cover at least :
                0x03   : EVENT_SYSTEM_FOREGROUND

                0x08   : EVENT_SYSTEM_CAPTURESTART
                0x09   : EVENT_SYSTEM_CAPTUREEND
                // ^^ w/o these, the target can end up z-ahead of overlay upon titlebar click etc
                0x0A   : EVENT_SYSTEM_MOVESIZESTART
                0x0B   : EVENT_SYSTEM_MOVESIZEEND

                0x16   : EVENT_SYSTEM_MINIMIZESTART
                0x17   : EVENT_SYSTEM_MINIMIZEEND

                0x8000 : EVENT_OBJECT_CREATE
                0x8001 : EVENT_OBJECT_DESTROY
                0x8002 : EVENT_OBJECT_SHOW
                0x8003 : EVENT_OBJECT_HIDE
                0x800B : EVENT_OBJECT_LOCATIONCHANGE

                0x8017 : EVENT_OBJECT_CLOAKED
                0x8018 : EVENT_OBJECT_UNCLOAKED
         */
        let _ = SetWinEventHook ( 0x0003, 0x0003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x0008, 0x000B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x0016, 0x0017, None, Some(win_event_proc), 0, 0, 0 );

        let _ = SetWinEventHook ( 0x8000, 0x8003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x800B, 0x800B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x8017, 0x8018, None, Some(win_event_proc), 0, 0, 0 );


        // finally we just babysit the message loop
        let mut msg: MSG = std::mem::zeroed();

        loop {
            if GetMessageW(&mut msg, None, 0, 0) == false {
                let _ = MagUninitialize();
                return Err(format!("GetMessageW failed with error: {:?}", GetLastError()));
            }
            else if msg.message == WM_TIMER || msg.message == WM_APP__REQ_UPDATE {
                let overlays = self.overlays.read().unwrap();
                for overlay in overlays.values() {
                    if overlay.marked.is_set() {
                        overlay.update(self);
                    }
                    let _ = InvalidateRect (Some(overlay.mag.into()), None, false);
                }
            }
            else if msg.message == WM_HOTKEY {

                let target = GetForegroundWindow().into();
                let mut overlays = self.overlays.write().unwrap();

                if let Some(overlay) = overlays.get(&target) {
                    match msg.wParam.0 {
                        HOTKEY_ID__TOGGLE => {
                            self.remove_overlay (target, &mut overlays);
                            self.rules.register_user_unapplied (target);
                        },
                        HOTKEY_ID__NEXT_EFFECT => {
                            apply_color_effect (overlay.mag, overlay.effect.cycle_next());
                        },
                        HOTKEY_ID__PREV_EFFECT => {
                            apply_color_effect (overlay.mag, overlay.effect.cycle_prev());
                        },
                        _ => { }
                    }
                }
                else if msg.wParam.0 == HOTKEY_ID__TOGGLE {
                    self.create_overlay (target, ColorEffect::default(), &mut overlays);
                }
                //tracing::debug!(overlays);
            }
            else if msg.message == WM_APP__REQ_CREATE_OVERLAY {
                let target = Hwnd (msg.wParam.0 as _);
                let mut overlays = self.overlays.write().unwrap();
                if !overlays.contains_key (&target) {
                    self.create_overlay ( target, ColorEffect (msg.lParam.0 as _),  &mut overlays );
                }
            }
            else {
                //let _ = TranslateMessage(&msg);
                // ^^ not needed as we dont do any gui w text etc
                DispatchMessageW(&msg);
            }
        }

    } }

    fn remove_overlay (&self, target: Hwnd, overlays: &mut HashMap <Hwnd, Overlay>) {
        if let Some(overlay) = overlays.remove (&target) {
            // ^^ the returned value is dropped and so its hwnds will get cleaned up
            if overlay.target == self.ov_topmost.load() { self.ov_topmost.clear(); }
        }
        if overlays.is_empty() { self.disable_timer() }
        tray::update_tray__overlay_count (overlays.len());
    }

    fn create_overlay (&self, target:Hwnd, effect:ColorEffect, overlays: &mut HashMap <Hwnd, Overlay>) {
        if let Ok(overlay) = Overlay::new (target, effect) {
            if overlays.is_empty() { self.ensure_timer_running() }
            overlays.insert (target, overlay);
            tray::update_tray__overlay_count (overlays.len());
        }
    }

    fn post_req__overlay_creation (&self, target:Hwnd, effect:ColorEffect) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_CREATE_OVERLAY, WPARAM (target.0 as _), LPARAM (effect.0 as _));
    } }
    fn post_req__update (&self) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_UPDATE, WPARAM(0), LPARAM(0));
    } }

    pub(crate) fn clear_overlays (&self) { unsafe {
        let mut overlays = self.overlays.write().unwrap();
        if overlays.is_empty() {
            return
        }
        let hwnds : Vec<_> = overlays.keys().copied().collect();
        for hwnd in hwnds {
            if let Some(overlay) = overlays.remove(&hwnd) {
                let _ = PostMessageW (Some(overlay.host.into()), WM_CLOSE, Default::default(), Default::default());
                let _ = InvalidateRect (Some(hwnd.into()), None, true);
                self.rules.register_user_unapplied (hwnd);
            }
        }
        self.ov_topmost.clear();
        self.disable_timer();
        tray::update_tray__overlay_count(0);
    } }

    pub fn ensure_timer_running (&self) { unsafe {
        let timer_id = SetTimer (None, 0, TIMER_TICK_MS, None);
        self.cur_timer .store (timer_id, Ordering::Release);
    } }

    pub fn disable_timer (&self) { unsafe {
        let _ = KillTimer (None, self.cur_timer .load(Ordering::Acquire));
    } }


    pub fn register_hotkeys (&self) {
        info! ("Registering hotkeys: ");
        self.conf.get_dusky_toggle_hotkey()      .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__TOGGLE as _));
        self.conf.get_dusky_next_effect_hotkey() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__PREV_EFFECT as _));
        self.conf.get_dusky_prev_effect_hotkey() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__NEXT_EFFECT as _));
    }


}

fn register_hotkey (hotkey:HotKey, id:i32) { unsafe {
    info! ("Attempting to register hotkey id:{:?} .. {:?}", id, &hotkey);
    if RegisterHotKey (None, id, hotkey.hk_mod() | MOD_NOREPEAT,  hotkey.key.to_vk_code() as _) .is_err() {
        error! ("Failed to register hotkey id:{:?} .. {:?}", id, GetLastError());
    }
} }




fn apply_color_effect (mag: impl Into<HWND>, effect: MAGCOLOREFFECT) { unsafe {
    if MagSetColorEffect (mag.into(), &effect as *const _ as _) == false {
        error! ("Setting Color Effect failed with error: {:?}", GetLastError());
        //let _ = MagUninitialize();
        //todo .. gotta get around to proper err handling at some point .. incl un-init before exits etc
    }
} }




// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}


#[allow (dead_code)] pub fn win_check_if_topmost (hwnd: Hwnd) -> bool { unsafe {
    GetWindowLongW (hwnd.into(), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == WS_EX_TOPMOST.0
} }




// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    host: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    // we'll ust leave default message handling
    DefWindowProcW (host, msg, wparam, lparam)
}




// Callback handling for our win-event hook
unsafe extern "system" fn win_event_proc (
    _hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32,
    _id_child: i32, _event_thread: u32, _event_time: u32,
) {
    if id_object != OBJID_WINDOW.0 { return; }
    // ^^ we only care about window level events

    let wd = WinDusky::instance();
    let mut overlays = wd.overlays.write().unwrap();

    //// debug printout of all events .. useful during dev
    //if !hwnd.is_invalid() && wd.overlays.lock().unwrap().contains_key(&hwnd.into()) {
    //    tracing::debug!("{:#06x}",event);
    //} // ^^ debug printouts (enable all events first)

    if let Some(overlay) = overlays .get (&hwnd.into()) {
        //tracing::debug!("got event {:#06x} for hwnd {:?}, id-object {:#06x}, id-child {:#06x}", event, hwnd, id_object, _id_child);
        if event == EVENT_OBJECT_DESTROY || event == EVENT_OBJECT_HIDE || event == EVENT_OBJECT_CLOAKED {
            wd.remove_overlay (hwnd.into(), &mut overlays);
        } else {
            // we simply mark the overlay here for an update on its next refresh
            overlay.marked.set();
            // and to kick an immdt update we'll post a message too (insead of waiting for timer)
            wd.post_req__update();
        }
    }
    else if event == EVENT_SYSTEM_FOREGROUND {
        // i.e non overlaid window came to fgnd .. so we'll clear any on-top overlays
        if let Some(overlay) = overlays .get (&wd.ov_topmost.load()) {
            wd.ov_topmost.clear();
            overlay.resync_ov_z_order();
        }

        // then we'll check if auto-rules mean we should create overlay for this hwnd itself
        let hwnd:Hwnd = hwnd.into();

        thread::spawn ( move ||  {

            // first lets see if we've already evaluated this and should move on
            let result = wd.rules.check_rule_cached (hwnd);

            if let Some ( RulesResult { enabled: false, .. } ) = result {
                return;
            }
            else if let Some ( RulesResult { enabled: true, effect} ) = result {
                wd.post_req__overlay_creation (hwnd, effect.unwrap_or_default());
                return
            }

            // so looks like this is first ever fgnd for this, so we'd like to eval from scratch ..
            // however, as seen before, it takes time for explorer windows to get all their properties after their new hwnds report fgnd
            // .. so we'll have to sit on delays and check it periodically (just like done in switche/krusty etc)

            //thread::sleep (Duration::from_millis(100));

            if let RulesResult { enabled: true, effect } = wd.rules.re_check_rule(hwnd) {
                wd.post_req__overlay_creation (hwnd, effect.unwrap_or_default());
            }
            thread::sleep (Duration::from_millis(300));

            if wd.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect } = wd.rules.re_check_rule(hwnd) {
                wd.post_req__overlay_creation (hwnd, effect.unwrap_or_default());
            }
            thread::sleep (Duration::from_millis(500));

            if wd.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect } = wd.rules.re_check_rule(hwnd) {
                wd.post_req__overlay_creation (hwnd, effect.unwrap_or_default());
            }
        } );
    }

}



