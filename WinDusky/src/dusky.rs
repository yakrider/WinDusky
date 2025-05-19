#![allow (non_snake_case)]

//use no_deadlocks::RwLock;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use tracing::{info, warn};
use windows::Win32::Foundation::{GetLastError, FALSE, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Magnification::{MagInitialize, MagUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage, PostThreadMessageW, SetTimer, MSG, WM_APP, WM_DESTROY, WM_HOTKEY, WM_TIMER};

use crate::{*, types::*};
use crate::effects::ColorEffect;

mod overlay;
mod hooks;
mod hotkeys;

use overlay::{FullScreenOverlay, Overlay};
use crate::presets::{GammaPresets, GammaPreset, GammaPresetAtomic};



const TIMER_TICK_MS : u32 = 16;

const WM_APP__REQ_REFRESH                : u32 = WM_APP + 1;
const WM_APP__REQ_OVERLAY_CREATE         : u32 = WM_APP + 2;
const WM_APP__REQ_OVERLAY_CLEAR_ALL      : u32 = WM_APP + 3;
const WM_APP__UN_REGISTER_HOTEKYS        : u32 = WM_APP + 4;
const WM_APP__REQ_TOGGLE_FULLSCREEN_MODE : u32 = WM_APP + 5;
const WM_APP__REQ_TOGGLE_FULLSCREEN_EFF  : u32 = WM_APP + 6;




//#[derive (Debug)]
pub struct WinDusky {
    pub conf    : &'static config::Config,
    pub auto    : &'static auto::AutoOverlay,
    pub effects : &'static effects::ColorEffects,
    pub presets : &'static GammaPresets,

    fs_overlay : &'static FullScreenOverlay,

    thread_id : u32,

    gamma_active : Flag,
    gamma_preset : GammaPresetAtomic,

    overlays : RwLock <HashMap <Hwnd, Overlay>>,
    hosts    : RwLock <HashSet <Hwnd>>,

    ov_topmost : HwndAtomic,
    // ^^ which overlay target hwnd (if any) we have cur set topmost

    cur_timer : AtomicUsize,
    // ^^ OS timer id changes every time we start/stop .. so a ref to cur timer to shut it later

    occl_marked : Flag,
    // ^^ whether we've been marked to have to refresh overlay occlusion calcs (based on win-events)

    fgnd_cache : HwndAtomic,
    // ^^ since GetForegroundWindow can return null in transitions, we'd rather act on last cached fgnd for fallback
}



static WIN_DUSKY : OnceLock <WinDusky> = OnceLock::new();

impl WinDusky {

    // Reminder that actions on hwnds (e.g. deletion) only have effect when called from the owning thread !!
    // .. so such actions must be posted to our msg queue here (rather than directly exposed as pub fns)
    // (Note however, that handlers for hotkeys registered from here, or hooks set here, are still in our thread context !!)

    pub fn instance() -> &'static WinDusky {
        WIN_DUSKY .get() .expect ("WinDusky not initialised yet !!")
    }

    pub fn init (conf: &'static config::Config) -> Result <&'static WinDusky, String> { unsafe {

        if WIN_DUSKY.get().is_some() {
            return Err ("WinDusky was allready started!!".into());
        }

        let _ = SetProcessDpiAwarenessContext (DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        if MagInitialize() == FALSE {
            return Err (format!("MagInitialize failed with error: {:?}", GetLastError()));
        }

        overlay::register_overlay_class()?;

        conf.check_dusky_conf_version_match();

        let effects = effects::ColorEffects::init (conf);
        let presets = GammaPresets::init (conf);

        let fs_overlay = FullScreenOverlay::instance();
        fs_overlay.effect.store (effects.default);

        let auto = auto::AutoOverlay::init (conf, effects);

        let dusky =  WinDusky {
            conf, auto, effects, presets, fs_overlay,

            thread_id : GetCurrentThreadId(),

            gamma_active : Flag::default(),
            gamma_preset : GammaPresetAtomic::default(),

            overlays : RwLock::new (HashMap::default()),
            hosts    : RwLock::new (HashSet::default()),

            ov_topmost  : HwndAtomic::default(),
            cur_timer   : AtomicUsize::default(),
            occl_marked : Flag::new(true),

            fgnd_cache  : HwndAtomic::default(),
        };

        Ok ( WIN_DUSKY .get_or_init (move || dusky) )
    } }


    pub fn start_win_dusky (&self) -> Result<(), String> { unsafe {

        self.register_hotkeys();

        self.setup_win_hooks();

        // we'll setup gamma, but only if specified active at startup (to avoid resetting otherwise)
        self.gamma_active.store (self.conf.check_flag__gamma_at_startup());
        self.gamma_preset.store (self.presets.default);
        if self.gamma_active.is_set() { self.update_gamma_state(); }

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
                    if self.gamma_active.is_set() { gamma::reset_screen_ramp(); }
                    let _ = MagUninitialize();
                    PostQuitMessage(0);
                }
                _ => { DispatchMessageW(&msg); }
            }
        }

    } }

    pub fn update_gamma_state (&self) {
        if let Some ((spec, name)) = self.gamma_active.is_set() .then_some ( {
            let preset = GammaPreset::from (&self.gamma_preset);
            (preset.get(), preset.name())
        } ) {
            info! ("Applying GammaPreset values from Preset : {:?}", name);
            let succeeded = gamma::set_screen_ramp_gbct (&spec.gbc, spec.color_temp);
            tray::update_tray__gamma_state (true, succeeded, Some(name));
        } else {
            info! ("Resetting Gamma values to Normal");
            gamma::reset_screen_ramp();
            tray::update_tray__gamma_state (false, true, None);
        }
    }

    pub fn toggle_gamma_active (&self) {
        if self.gamma_active.is_set() && !self.check_active_gamma_preset_match().unwrap_or_default() {
            warn! ("Gamma Preset Toggle requested, but active ramp does not match preset .. Re-applying instead !!");
            self.update_gamma_state();
            return
        }
        self.gamma_active.toggle();
        self.update_gamma_state();
    }

    pub fn check_active_gamma_preset_match (&self) -> Option <bool> {
        let preset = GammaPreset::from (&self.gamma_preset) .get();
        gamma::check_active_gamma_match (&preset.gbc, preset.color_temp)
    }

    pub fn cycle_gamma_preset (&self, forward: bool) {
        if self.gamma_active.is_clear() { return }
        self.gamma_preset.cycle (forward);
        self.update_gamma_state();
    }


    pub fn check_fs_mode (&self) -> bool {
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
        let prior_eff : ColorEffect = (&self.fs_overlay.effect).into();
        self.fs_overlay.set_enabled(enabled);
        if enabled {
            // we gotta clear out per-hwnd overlays upon entering full-screen mode
            // Note that since this could be called from other threads (e.g tray), we must post message
            // (as hwnds can only be destroyed from the threads that called them)
            self.post_req__overlay_clear_all();
            // ^^ will also clear overrides
        }
        //else {}  // <- if we just toggled off fs-mode .. thats it, nothing more to do

        let effect = if !enabled { prior_eff } else { (&self.fs_overlay.effect).into() };
        tray::update_full_screen_mode (enabled, Some(effect.name()));
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
            info! ("Created Overlay on {:?} with {:?}, tot: {:?}", target, effect, overlays.len());
        } else {
            warn! ("Failed to create Overlay on {:?} with {:?}, tot: {:?}", target, effect, overlays.len());
        }
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
        self.occl_marked.clear();
        let viz_res = occlusion::calc_viz_bounds (
            self, self.overlays .read().unwrap() .values() .map (|ov| ov.target)
        );  // <- separately to limit lock scope
        if let Ok (bounds_map) = viz_res {
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
        self.auto.clear_user_overrides();
    }

    pub fn has_overlay (&self, hwnd: &Hwnd) -> bool {
        self.overlays .read() .is_ok_and (|ovs| ovs.contains_key (hwnd))
    }


    fn post_simple_req (&self, msg:u32) { unsafe {
        let _ = PostThreadMessageW (self.thread_id, msg, WPARAM(0), LPARAM(0));
    } }
    pub fn post_req__toggle_fs_mode      (&self) { self.post_simple_req (WM_APP__REQ_TOGGLE_FULLSCREEN_MODE) }
    pub fn post_req__toggle_fs_eff       (&self) { self.post_simple_req (WM_APP__REQ_TOGGLE_FULLSCREEN_EFF) }
    pub fn post_req__refresh             (&self) { self.post_simple_req (WM_APP__REQ_REFRESH) }
    pub fn post_req__overlay_clear_all   (&self) { self.post_simple_req (WM_APP__REQ_OVERLAY_CLEAR_ALL) }
    pub fn post_req__un_register_hotkeys (&self) { self.post_simple_req (WM_APP__UN_REGISTER_HOTEKYS) }
    pub fn post_req__quit                (&self) { self.post_simple_req (WM_DESTROY) }

    pub fn post_req__overlay_create (&self, target:Hwnd, effect:ColorEffect) { unsafe {
        let _ = PostThreadMessageW (
            self.thread_id, WM_APP__REQ_OVERLAY_CREATE, WPARAM (target.0 as _), LPARAM (effect.0 as _)
        );
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

}





