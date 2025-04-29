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
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage, PostThreadMessageW, SetTimer, MSG, WM_APP, WM_DESTROY, WM_HOTKEY, WM_TIMER};

use crate::config::Config;
use crate::effects::{ColorEffect, ColorEffects};
use crate::rules::{RulesMonitor, RulesResult};
use crate::{tray, types::*, *};

mod overlay;
mod hooks;
mod hotkeys;

use overlay::{FullScreenOverlay, Overlay};

const TIMER_TICK_MS : u32 = 16;

const WM_APP__REQ_REFRESH                : u32 = WM_APP + 1;
const WM_APP__REQ_OVERLAY_CREATE         : u32 = WM_APP + 2;
const WM_APP__REQ_OVERLAY_CLEAR_ALL      : u32 = WM_APP + 3;
const WM_APP__UN_REGISTER_HOTEKYS        : u32 = WM_APP + 4;
const WM_APP__REQ_TOGGLE_FULLSCREEN_MODE : u32 = WM_APP + 5;
const WM_APP__REQ_TOGGLE_FULLSCREEN_EFF  : u32 = WM_APP + 6;




//#[derive (Debug)]
pub struct WinDusky {
    pub conf    : &'static Config,
    pub rules   : &'static RulesMonitor,
    pub effects : &'static ColorEffects,

    fs_overlay : &'static FullScreenOverlay,

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

    // Reminder that actions on hwnds (e.g. deletion) only have effect when called from the owning thread !!
    // .. so such actions must be posted to our msg queue here (rather than directly exposed as pub fns)
    // (Note however, that handlers for hotkeys registered from here, or hooks set here, are still in our thread context !!)

    pub fn instance() -> &'static WinDusky {
        static INSTANCE : OnceLock <WinDusky> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            WinDusky {
                conf    : Config::instance(),
                rules   : RulesMonitor::instance(),
                effects : ColorEffects::instance(),

                fs_overlay : FullScreenOverlay::instance(),

                thread_id : AtomicU32::default(),
                overlays  : RwLock::new (HashMap::default()),
                hosts     : RwLock::new (HashSet::default()),

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

        // our defaults come from confs, so we'll want to load those defaults into our defaults !!
        self.effects.load_effects_from_conf (self.conf);
        self.fs_overlay.effect.store (self.effects.get_default());

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
            match msg.message {
                WM_TIMER | WM_APP__REQ_REFRESH => {
                    self.refresh_overlays();
                }
                WM_HOTKEY => {
                    self.handle_hotkeys (msg.wParam.0);
                }
                WM_APP__REQ_TOGGLE_FULLSCREEN_MODE => {
                    self.toggle_full_screen_mode();
                }
                WM_APP__REQ_TOGGLE_FULLSCREEN_EFF => {
                    let eff = self.fs_overlay.toggle_effect();
                    tray::update_full_screen_mode (self.fs_overlay.enabled.is_set(), eff.map (|e| e.name()));
                }
                WM_APP__REQ_OVERLAY_CREATE => {
                    self.create_overlay (Hwnd (msg.wParam.0 as _), ColorEffect (msg.lParam.0 as _));
                }
                WM_APP__REQ_OVERLAY_CLEAR_ALL => {
                    self.clear_overlays();
                }
                WM_APP__UN_REGISTER_HOTEKYS => {
                    self.un_register_hotkeys();
                }
                WM_DESTROY => {
                    warn!("Shutting down .. ~~~~ GOOD BYE ~~~~ !!");
                    let _ = MagUninitialize();
                    PostQuitMessage(0);
                }
                _ => { DispatchMessageW(&msg); }
            }
        }

    } }



    fn check_fs_mode (&self) -> bool {
        self.fs_overlay.enabled.is_set()
    }
    fn toggle_full_screen_mode (&self) -> bool {
        // Note that this to toggle the ENABLED state .. not to toggle overlay alone
        // When toggling on, it does apply overlay, and when toggling off, it does remove it..
        // .. however, once can unapply the overlay (e.g. via hotkey) w/o toggling the mode off too!
        let enabled = self.fs_overlay.toggle();
        self.set_full_screen_mode (enabled);
        enabled
    }
    fn set_full_screen_mode (&self, enabled:bool) {
        self.fs_overlay.set_enabled(enabled);
        if enabled {
            // we gotta clear out per-hwnd overlays upon entering full-screen mode
            // Note that since this could be called from other threads (e.g tray), we must post message
            // (as hwnds can only be destroyed from the threads that called them)
            self.post_req__overlay_clear_all();
            self.rules.clear_user_overrides();
        }
        //else {}  // <- if we jsut toggled off fs-mode .. thats it, nothing more to do

        let effect = if !enabled { None } else {
            Some (ColorEffect::from (&self.fs_overlay.effect) .name())
        };
        tray::update_full_screen_mode (enabled, effect);
    }


    fn create_overlay (&self, target:Hwnd, effect:ColorEffect) {
        // Warning : This should only be called from overlay-manager thread
        if !target.is_valid() {
            warn! ("~~ WARNING ~~ Overlay creation request for {:?} .. Ingoring", &target);
            return
        }
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

    fn remove_overlay (&self, target:Hwnd) {
        // Warning : This should only be called from overlay-manager thread
        let mut overlays = self.overlays.write().unwrap();
        if let Some(overlay) = overlays.remove (&target) {
            overlay.destroy();
            self.hosts.write().unwrap().remove (&overlay.host);
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

    fn clear_overlays (&self) {
        // Reminder that this MUST be called from overlay-manager thread to have effect on overlay hwnds
        let mut overlays = self.overlays.write().unwrap();
        info! ("Clearing all {:?} Color Effect Overlays", overlays.len());

        let hwnds : Vec<_> = overlays.keys().copied().collect();
        for hwnd in hwnds {
            if let Some(overlay) = overlays.remove(&hwnd) {
                overlay.destroy();
                self.hosts.write().unwrap().remove(&overlay.host);
            }
        }
        self.ov_topmost.clear();
        self.disable_timer();
        tray::update_tray__overlay_count(0);
        self.rules.clear_user_overrides();
    }


    fn post_simple_req (&self, msg:u32) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, msg, WPARAM(0), LPARAM(0));
    } }
    pub fn post_req__toggle_fs_mode      (&self) { self.post_simple_req (WM_APP__REQ_TOGGLE_FULLSCREEN_MODE) }
    pub fn post_req__toggle_fs_eff       (&self) { self.post_simple_req (WM_APP__REQ_TOGGLE_FULLSCREEN_EFF) }
    pub fn post_req__refresh             (&self) { self.post_simple_req (WM_APP__REQ_REFRESH) }
    pub fn post_req__overlay_clear_all   (&self) { self.post_simple_req (WM_APP__REQ_OVERLAY_CLEAR_ALL) }
    pub fn post_req__un_register_hotkeys (&self) { self.post_simple_req (WM_APP__UN_REGISTER_HOTEKYS) }
    pub fn post_req__quit                (&self) { self.post_simple_req (WM_DESTROY) }

    pub fn post_req__overlay_create (&self, target:Hwnd, effect:ColorEffect) { unsafe {
        let thread_id = self.thread_id.load(Ordering::Relaxed);
        let _ = PostThreadMessageW (thread_id, WM_APP__REQ_OVERLAY_CREATE, WPARAM (target.0 as _), LPARAM (effect.0 as _));
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

        if self.check_fs_mode() { return }
        if !self.rules.check_auto_overlay_enabled() { return }

        // next we'll check if we have have evaluated auto-overlay rules for this previously
        let result = self.rules.check_rule_cached (hwnd);

        if let Some ( RulesResult { enabled: false, .. } ) = result {
            return;
        }
        else if let Some ( RulesResult { enabled: true, effect, ..} ) = result {
            self.post_req__overlay_create (hwnd, effect.unwrap_or (self.effects.get_default()));
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
                self.post_req__overlay_create (hwnd, effect.unwrap_or (self.effects.get_default()));
            }

            // however, as seen before, it takes time for some windows to get all their properties after newly created hwnds report fgnd
            // .. so we'll just sit on delays and check it a couple times (just like done in switche/krusty etc)
            // The easiest way to test the utility of this is prob to start something like perfmon.exe w/ and w/o delay-waits

            thread::sleep (Duration::from_millis(300));

            if self.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.rules.re_check_rule(hwnd) {
                self.post_req__overlay_create (hwnd, effect.unwrap_or (self.effects.get_default()));
            }

            thread::sleep (Duration::from_millis(500));

            if self.overlays.read().unwrap().contains_key(&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.rules.re_check_rule(hwnd) {
                self.post_req__overlay_create (hwnd, effect.unwrap_or (self.effects.get_default()))
            }

        } );

    }

}





