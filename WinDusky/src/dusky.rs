#![ allow (non_snake_case) ]

//use no_deadlocks::RwLock;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use std::thread;
use std::time::Duration;
use tracing::{info, warn};
use windows::Win32::Foundation::{GetLastError, FALSE, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Magnification::{MagInitialize, MagUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, KillTimer, PostThreadMessageW, SetTimer, MSG, WM_APP, WM_HOTKEY, WM_TIMER};

use crate::config::Config;
use crate::effects::{ColorEffect, ColorEffects};
use overlay::Overlay;
use crate::rules::{RulesMonitor, RulesResult};
use crate::{tray, types::*, *};


mod overlay;
mod hooks;
mod hotkeys;


const TIMER_TICK_MS : u32 = 16;

const WM_APP__REQ_REFRESH         : u32 = WM_APP + 1;
const WM_APP__REQ_CREATE_OVERLAY : u32 = WM_APP + 2;




//#[derive (Debug)]
pub struct WinDusky {
    pub conf    : &'static Config,
    pub rules   : &'static RulesMonitor,
    pub effects : &'static ColorEffects,

    thread_id : AtomicU32,
    overlays  : RwLock <HashMap <Hwnd, Overlay>>,
    hosts     : RwLock <HashSet <Hwnd>>,

    ov_topmost : HwndAtomic,
    // ^^ which overlay target hwnd (if any) we have cur set topmost

    cur_timer : AtomicUsize,
    // ^^ OS timer id changes every time we start/stop .. so a ref to cur timer to shut it later

    occl_marked : Flag,
    // ^^ whether we've been marked to have to refresh overlay occlusion calcs (based on win-events)
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

                ov_topmost  : HwndAtomic::default(),
                cur_timer   : AtomicUsize::default(),
                occl_marked : Flag::new(true),
            }
        )
        // ^^ NOTE that init is not called here, and the user should do so at their own convenience !!
    }


    pub fn start_overlay_manager (&self) -> Result<(), String> { unsafe {

        if self.thread_id .load (Ordering::Acquire) != 0 {
            // if we already have a thread-id, we must have already initied
            return Ok(())
        };
        self.thread_id.store (GetCurrentThreadId(), Ordering::Release);

        let _ = SetProcessDpiAwarenessContext (DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        if MagInitialize() == FALSE {
            return Err (format!("MagInitialize failed with error: {:?}", GetLastError()));
        }

        overlay::register_overlay_class()?;

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
            else if msg.message == WM_TIMER || msg.message == WM_APP__REQ_REFRESH {
                self.refresh_overlays();
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


    pub(crate) fn create_overlay (&self, target:Hwnd, effect:ColorEffect) {
        let mut overlays = self.overlays.write().unwrap();
        if overlays .contains_key (&target) {
            warn! ("Ignoring overlay creation request for {:?} .. Overlay already exists!!", &target);
            return
        }
        if let Ok(overlay) = Overlay::new (target, effect) {
            if overlays.is_empty() { self.ensure_timer_running() }
            self.hosts.write().unwrap().insert(overlay.host);
            self.occl_marked.set();
            overlays.insert (target, overlay);
            tray::update_tray__overlay_count (overlays.len());
        }
        info! ("Created Overlay on {:?} with {:?}, tot: {:?}", target, effect, overlays.len());
    }

    pub(crate) fn remove_overlay (&self, target:Hwnd) {
        let mut overlays = self.overlays.write().unwrap();
        if let Some(overlay) = overlays.remove (&target) {
            // ^^ the returned value is dropped and so its hwnds will get cleaned up
            self.hosts.write().unwrap().remove(&overlay.host);
            if overlay.target == self.ov_topmost.load() { self.ov_topmost.clear(); }
            info! ("Removed Overlay from {:?}, tot: {:?}", overlay.target, overlays.len());
        }
        if overlays.is_empty() { self.disable_timer() }
        self.occl_marked.set();
        tray::update_tray__overlay_count (overlays.len());
    }

    fn refresh_overlays (&self) {
        if self.occl_marked.is_set() {
            self.refresh_viz_bounds();
        }
        for overlay in self.overlays.read().unwrap().values() {
            overlay.refresh(self);
        }
    }

    fn refresh_viz_bounds (&self) {
        let targets : Vec<Hwnd> = self.overlays .read().unwrap() .values() .map (|ov| ov.target) .collect();
        self.occl_marked.clear();
        if let Ok (bounds_map) = occlusion::calc_viz_bounds (self, &targets) {
            let mut overlays = self.overlays.write().unwrap();
            for (target, bounds) in bounds_map .into_iter() {
                if let Some (overlay) = overlays .get_mut (&target) {
                    overlay.viz_bounds = bounds;
                    //tracing::debug! ("{:?} : {:?}", target, bounds);
                }
        }   }
    }

    pub fn clear_overlays (&self) {
        let mut overlays = self.overlays.write().unwrap();
        info! ("Clearing all {:?} Color Effect Overlays", overlays.len());
        if overlays.is_empty() {
            return
        }
        let hwnds : Vec<_> = overlays.keys().copied().collect();
        for hwnd in hwnds {
            if let Some(overlay) = overlays.remove(&hwnd) {
                self.hosts.write().unwrap().remove(&overlay.host);
                self.rules.register_user_unapplied (hwnd);
            }
        }
        self.ov_topmost.clear();
        self.disable_timer();
        tray::update_tray__overlay_count(0);
    }


    fn post_req__overlay_creation (&self, target:Hwnd, effect:ColorEffect) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_CREATE_OVERLAY, WPARAM (target.0 as _), LPARAM (effect.0 as _));
    } }
    fn post_req__refresh (&self) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_REFRESH, WPARAM(0), LPARAM(0));
    } }


    fn ensure_timer_running (&self) { unsafe {
        let timer_id = SetTimer (None, 0, TIMER_TICK_MS, None);
        self.cur_timer .store (timer_id, Ordering::Release);
    } }

    fn disable_timer (&self) { unsafe {
        let _ = KillTimer (None, self.cur_timer .load(Ordering::Acquire));
    } }


    pub fn get_hosts (&self) -> HashSet<Hwnd> {
        self.hosts.read().unwrap().clone()
    }


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

            // however, as seen before, it takes time for some windows to get all their properties after newly created hwnds report fgnd
            // .. so we'll just sit on delays and check it a couple times (just like done in switche/krusty etc)
            // The easiest way to test the utility of this is prob to start something like perfmon.exe w/ and w/o delay-waits

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

}





