#![ allow (non_snake_case) ]

//use no_deadlocks::RwLock;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};
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
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindow, KillTimer, PostMessageW, PostThreadMessageW, RegisterClassExW, SetTimer, SetWindowPos, CS_HREDRAW, CS_VREDRAW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_SYSTEM_FOREGROUND, GW_HWNDPREV, HCURSOR, HICON, HWND_TOP, HWND_TOPMOST, MSG, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_HOTKEY, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};

use crate::config::{Config, HotKey};
use crate::effects::{ColorEffect, ColorEffectAtomic, ColorEffects};
use crate::rules::{RulesMonitor, RulesResult};
use crate::win_utils::wide_string;
use crate::{tray, types::*};


const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host";

const TIMER_TICK_MS : u32 = 16;

const HOTKEY_ID__TOGGLE          : usize = 1;
const HOTKEY_ID__NEXT_EFFECT     : usize = 2;
const HOTKEY_ID__PREV_EFFECT     : usize = 3;
const HOTKEY_ID__CLEAR_OVERLAYS  : usize = 4;
const HOTKEY_ID__CLEAR_OVERRIDES : usize = 5;
const HOTKEY_ID__CLEAR_ALL       : usize = 6;

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

    pub conf    : &'static Config,
    pub rules   : &'static RulesMonitor,
    pub effects : &'static ColorEffects,

    thread_id  : AtomicU32,
    overlays   : RwLock <HashMap <Hwnd, Overlay>>,
    hosts      : RwLock <HashSet <Hwnd>>,
    cur_timer  : AtomicUsize,
    ov_topmost : HwndAtomic,
}




impl Overlay {

    fn new (target:Hwnd, effect:ColorEffect) -> Result <Overlay, String> { unsafe {

        let h_inst : Option<HINSTANCE> = GetModuleHandleW(None) .ok() .map(|h| h.into());

        // Create the host for the magniier control
        let Ok(host) = CreateWindowExW (
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw (wide_string (HOST_WINDOW_CLASS_NAME).as_ptr()),
            PCWSTR::from_raw (wide_string (&format!("{} for {:#x}", HOST_WINDOW_TITLE, target.0)).as_ptr()),
            WS_POPUP, 0, 0, 0, 0, None, None, h_inst, None
        ) else {
            return Err (format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
        };

        // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
        let Ok(mag) = CreateWindowExW (
            WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, Some(host), None, h_inst, None,
        ) else {
            return Err (format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
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
        overlay.apply_color_effect (overlay.effect.get());

        // we'll mark the overlay which will make our main loop timer-handler sync dimensions and position with the target
        overlay.marked.set();

        Ok(overlay)
    } }


    pub fn update (&self, wd: &WinDusky) { unsafe {

        //tracing::debug! ("updating overlay {:?}", self);

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
                //tracing::debug! ("found active ov_top {:?}, will reorder it.", ov_top);
                let overlays = wd.overlays.read().unwrap();
                if let Some(overlay) = overlays .get (&ov_top) {
                    overlay.resync_ov_z_order()
                }
            }
        }
        //let hts : std::collections::HashSet<Hwnd> = vec! (self.host, self.target) .into_iter() .collect();
        //tracing::debug! ("(host,target): {:?}", (self.host, self.target));
        //tracing::debug! ("... pre-order : {:?}", crate::win_utils::win_get_hwnds_ordered(&hts));

        // now first, we'll do the general z-order repositioning
        let hwnd_insert = GetWindow (target, GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (host, None,               x, y, w, h,  SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW);
        let _ = SetWindowPos (host, Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOREDRAW);
        // ^^ the two step appears necessary, as w hwnd-insert specified, it doesnt seem to move/reposition the window!

        // next, if we are actually fgnd, we'll also try and set topmost (which OS might or might not always allow)
        if self.target == fgnd {
            let _ = SetWindowPos (host, Some(HWND_TOP),      0, 0, 0, 0,  SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetWindowPos (host, Some(HWND_TOPMOST),  0, 0, 0, 0,  SWP_NOMOVE | SWP_NOSIZE);
            // ^^ having both seems to be required for robustness, esp after freshly closing some overlain windows etc ¯\_(ツ)_/¯
            self.is_top.set();
            wd.ov_topmost.store(target);
        }
        //tracing::debug! ("... post-order : {:?}", crate::win_utils::win_get_hwnds_ordered(&hts));
    } }

    pub fn resync_ov_z_order (&self) { unsafe {
        self.is_top.clear();
        let hwnd_insert = GetWindow (self.target.into(), GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (self.host.into(), Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE);
    } }

    pub fn apply_color_effect (&self, effect: MAGCOLOREFFECT) { unsafe {
        if ! MagSetColorEffect (self.mag.into(), &effect as *const _ as _) .as_bool() {
            error! ("Setting Color Effect failed with error: {:?}", GetLastError());
        }
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
                conf    : Config::instance(),
                rules   : RulesMonitor::instance(),
                effects : ColorEffects::instance(),

                thread_id  : AtomicU32::default(),
                overlays   : RwLock::new (HashMap::default()),
                hosts      : RwLock::new (HashSet::default()),
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
            return Err (format!("MagInitialize failed with error: {:?}", GetLastError()));
        }

        self.register_overlay_class()?;

        self.conf.check_dusky_conf_version_match();

        self.effects.load_effects_from_conf (self.conf);

        self.rules.load_conf_rules (self.conf, self.effects);

        self.register_hotkeys();

        self.setup_win_hooks();


        // finally we just babysit the message loop
        let mut msg: MSG = std::mem::zeroed();

        loop {
            if GetMessageW (&mut msg, None, 0, 0) == false {
                let _ = MagUninitialize();
                return Err (format!("GetMessageW failed with error: {:?}", GetLastError()));
            }
            else if msg.message == WM_TIMER || msg.message == WM_APP__REQ_UPDATE {
                self.update_overlays();
            }
            else if msg.message == WM_HOTKEY {
                self.handle_hotkeys (msg.wParam.0);
            }
            else if msg.message == WM_APP__REQ_CREATE_OVERLAY {
                self.create_overlay ( Hwnd (msg.wParam.0 as _), ColorEffect (msg.lParam.0 as _) );
            }
            else {
                //let _ = TranslateMessage(&msg);
                // ^^ not needed as we dont do any gui w text etc
                DispatchMessageW(&msg);
            }
        }

    } }


    fn create_overlay (&self, target:Hwnd, effect:ColorEffect) {
        let mut overlays = self.overlays.write().unwrap();
        if overlays .contains_key (&target) {
            warn! ("Ignoring overlay creation request for {:?} .. Overlay already exists!!", &target);
            return
        }
        if let Ok(overlay) = Overlay::new (target, effect) {
            if overlays.is_empty() { self.ensure_timer_running() }
            self.hosts.write().unwrap().insert(overlay.host);
            overlays.insert (target, overlay);
            tray::update_tray__overlay_count (overlays.len());
        }
        info! ("Created Overlay on {:?} with {:?}, tot: {:?}", target, effect, overlays.len());
    }

    fn remove_overlay (&self, target:Hwnd) {
        let mut overlays = self.overlays.write().unwrap();
        if let Some(overlay) = overlays.remove (&target) {
            // ^^ the returned value is dropped and so its hwnds will get cleaned up
            self.hosts.write().unwrap().remove(&overlay.host);
            if overlay.target == self.ov_topmost.load() { self.ov_topmost.clear(); }
            info! ("Removed Overlay from {:?}, tot: {:?}", overlay.target, overlays.len());
        }
        if overlays.is_empty() { self.disable_timer() }
        tray::update_tray__overlay_count (overlays.len());
    }

    fn update_overlays (&self) {
        for overlay in self.overlays.read().unwrap().values() {
            if overlay.marked.is_set() {
                overlay.update(self);
            }
            let _ = unsafe { InvalidateRect (Some(overlay.mag.into()), None, false) };
        }
    }

    pub(crate) fn clear_overlays (&self) { unsafe {
        let mut overlays = self.overlays.write().unwrap();
        if overlays.is_empty() {
            return
        }
        info! ("Clearing all {:?} Color Effect Overlays", overlays.len());
        let hwnds : Vec<_> = overlays.keys().copied().collect();
        for hwnd in hwnds {
            if let Some(overlay) = overlays.remove(&hwnd) {
                let _ = PostMessageW (Some(overlay.host.into()), WM_CLOSE, Default::default(), Default::default());
                let _ = InvalidateRect (Some(hwnd.into()), None, true);
                self.hosts.write().unwrap().remove(&overlay.host);
                self.rules.register_user_unapplied (hwnd);
            }
        }
        self.ov_topmost.clear();
        self.disable_timer();
        tray::update_tray__overlay_count(0);
    } }

    fn post_req__overlay_creation (&self, target:Hwnd, effect:ColorEffect) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_CREATE_OVERLAY, WPARAM (target.0 as _), LPARAM (effect.0 as _));
    } }
    fn post_req__update (&self) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_UPDATE, WPARAM(0), LPARAM(0));
    } }

    fn ensure_timer_running (&self) { unsafe {
        let timer_id = SetTimer (None, 0, TIMER_TICK_MS, None);
        self.cur_timer .store (timer_id, Ordering::Release);
    } }

    fn disable_timer (&self) { unsafe {
        let _ = KillTimer (None, self.cur_timer .load(Ordering::Acquire));
    } }


    fn handle_auto_overlay (&'static self, hwnd:Hwnd) {

        // So we got an hwnd that doesnt have overlay yet, and we wanna see if auto-overlay rules apply to it

        if !self.rules.check_auto_overlay_enabled() { return }

        // next we'll check if we have have evaluated auto-overlay rules for this previously
        let result = self.rules.check_rule_cached (hwnd);

        if let Some ( RulesResult { enabled: false, .. } ) = result {
            return;
        }
        else if let Some ( RulesResult { enabled: true, effect, ..} ) = result {
            self.post_req__overlay_creation (hwnd, effect.unwrap_or (self.effects.get_default()));
            return
        }


        // so looks like this is first ever fgnd for this, so we'd like to eval from scratch ..
        // .. but eval for luminance requies screen cap, so we'll spawn thread to do all that
        thread::spawn ( move || {

            // further, doing a screen cap too early (esp with BitBlt) can capture not-quite-painted hwnds
            // .. so we'll put up a small delay before we go about the hwnd screen capture business
            thread::sleep (Duration::from_millis (self.rules.get_auto_overlay_delay_ms() as _));

            let result = self.rules.re_check_rule(hwnd);

            // but we'll ditch early if elevation restrictions apply (i.e this guy is elev but we're not)
            if let RulesResult { elev_excl: true, .. } = result {
                warn! ("!! WARNING !! .. WinDusky is NOT Elevated. Cannot overlay elevated {:?}", hwnd);
                return;
            }

            // otherwise, if it passed rules, we can go ahead and request an overlay creation
            if let RulesResult { enabled: true, effect, .. } = result {
                self.post_req__overlay_creation (hwnd, effect.unwrap_or (self.effects.get_default()));
            }

            // however, as seen before, it takes time for explorer windows to get all their properties after their new hwnds report fgnd
            // .. so we'll just sit on delays and check it a couple times (just like done in switche/krusty etc)

            thread::sleep (Duration::from_millis(300));

            if self.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.rules.re_check_rule(hwnd) {
                self.post_req__overlay_creation (hwnd, effect.unwrap_or (self.effects.get_default()));
            }

            thread::sleep (Duration::from_millis(500));

            if self.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.rules.re_check_rule(hwnd) {
                self.post_req__overlay_creation (hwnd, effect.unwrap_or (self.effects.get_default()))
            }

        } );

    }


    unsafe fn register_overlay_class (&self) -> Result <(), String> {
        let Ok(instance) = GetModuleHandleW(None) else {
            return Err (format!("GetModuleHandleW failed with error: {:?}", GetLastError()));
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
                return Err (format!("RegisterClassExW failed with error: {:?}", GetLastError()));
            }
        }
        Ok(())
    }


    fn register_hotkeys (&self) {
        // Note that this must be called from a thread that will be monitoring its msg queue
        fn register_hotkey (hotkey:HotKey, id:i32) { unsafe {
            info! ("Attempting to register hotkey id:{:?} .. {:?}", id, &hotkey);
            if RegisterHotKey (None, id, hotkey.hk_mod() | MOD_NOREPEAT,  hotkey.key.to_vk_code() as _) .is_err() {
                error! ("Failed to register hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }
        self.conf.get_hotkey__dusky_toggle()    .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__TOGGLE as _));
        self.conf.get_hotkey__next_effect()     .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__NEXT_EFFECT as _));
        self.conf.get_hotkey__prev_effect()     .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__PREV_EFFECT as _));
        self.conf.get_hotkey__clear_overlays()  .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERLAYS as _));
        self.conf.get_hotkey__clear_overrides() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERRIDES as _));
        self.conf.get_hotkey__clear_all()       .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_ALL as _));
    }


    fn handle_hotkeys (&self, hotkey: usize) {
        let target = unsafe { GetForegroundWindow().into() };
        match hotkey {
            HOTKEY_ID__TOGGLE => {
                if self.overlays.read().unwrap().contains_key(&target) {
                    self.remove_overlay (target);
                    self.rules.register_user_unapplied (target);
                } else {
                    // if there was some effect for it in eval cache, we'll use that or the overlay
                    // (e.g. this would preserve last effect when the overlay might have been last toggled on/off)
                    let effect = self.rules.check_rule_cached (target) .and_then (|r| r.effect) .unwrap_or (self.effects.get_default());
                    self.create_overlay (target, effect);
                }
            }
            HOTKEY_ID__NEXT_EFFECT => {
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target){
                    let effect = overlay.effect.cycle_next();
                    overlay .apply_color_effect (effect.get());
                    self.rules.update_cached_rule_result_effect (target, effect);
                }
            }
            HOTKEY_ID__PREV_EFFECT => {
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target){
                    let effect = overlay.effect.cycle_prev();
                    overlay .apply_color_effect (effect.get());
                    self.rules.update_cached_rule_result_effect (target, effect);
                }
            }
            HOTKEY_ID__CLEAR_OVERLAYS => {
                self.clear_overlays();
            }
            HOTKEY_ID__CLEAR_OVERRIDES => {
                self.rules.clear_user_overrides();
            }
            HOTKEY_ID__CLEAR_ALL => {
                self.clear_overlays();
                self.rules.clear_user_overrides();
            }
            _ => { }
        }
    }


    fn setup_win_hooks (&self) { unsafe {
        // Note this this must be called from some thread that will be monitoring its msg queue
        // Here, we'll setup a win-event hook to monitor fgnd change so we can maintain the overlay z-ordering
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

    } }


    pub fn handle_win_hook_event (&'static self, hwnd:Hwnd, event:u32) {
        // Note, only hwnd level events make it this far (child or non-window-obj events are filtered out)

        // // debug printout of all events .. useful during dev.. (enable all events first if so desired)
        // let overlays = self.overlays.read().unwrap();
        // if !hwnd.is_invalid() { //&& overlays.contains_key(&hwnd.into()) {
        //     let ov = if overlays.contains_key(&hwnd.into()) { "ov" } else { "  " };
        //     tracing::debug!("got event {:#06x} for {} hwnd {:?}, id-object {:#06x}, id-child {:#06x}", event, ov, hwnd, id_object, _id_child);
        // }

        // first off, lets ignore our own overlay hosts
        if self.hosts.read().unwrap().contains(&hwnd) { return }

        match event {
            EVENT_OBJECT_HIDE | EVENT_OBJECT_CLOAKED | EVENT_OBJECT_DESTROY => {
                // we treat hidden/closed/cloaked similarly by removing the overlay if there was any
                if self.overlays .read().unwrap() .contains_key (&hwnd) {
                    self.remove_overlay (hwnd)
                }
            }
            EVENT_SYSTEM_FOREGROUND => {
                // If this hwnd already had overlays, we just mark it for udpate and request one
                if let Some(overlay) = self.overlays .read().unwrap() .get (&hwnd) {
                    overlay.marked.set();
                    self.post_req__update();
                    return
                }
                // so we got a non-overlain hwnd to fgnd .. so if we had any overlain hwnds on-top, we should clear them
                if let Some(overlay) = self.overlays.read().unwrap() .get (&self.ov_topmost.load()) {
                    self.ov_topmost.clear();
                    overlay.resync_ov_z_order();
                }
                // finally we'll see if auto-overlay rules should apply to this
                self.handle_auto_overlay (hwnd);
            }
            _ => {
                // for all other registered events, we only process if hwnd had overlay, and if so we trigger an update
                if let Some(overlay) = self.overlays .read().unwrap() .get (&hwnd) {
                    overlay.marked.set();
                    self.post_req__update();
                }
            }
        }
    }

}





// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    host: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    // we'll just leave default message handling
    DefWindowProcW (host, msg, wparam, lparam)
}




// Callback handling for our win-event hook
unsafe extern "system" fn win_event_proc (
    _hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32,
    _id_child: i32, _event_thread: u32, _event_time: u32,
) {
    // we'll filter out non-window level events and pass up the rest
    if id_object != OBJID_WINDOW.0 { return; }
    WinDusky::instance() .handle_win_hook_event (hwnd.into(), event);
}




