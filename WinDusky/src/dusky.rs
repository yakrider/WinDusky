#![allow (non_snake_case)]

//use no_deadlocks::RwLock;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

use strum_macros::FromRepr;
use tracing::{info, warn};

use windows::Win32::Foundation::{GetLastError, FALSE, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Magnification::{MagInitialize, MagUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage, PostThreadMessageW, SetTimer, MSG, WM_APP, WM_DESTROY, WM_HOTKEY, WM_TIMER};


mod hooks;
mod hotkeys;
mod overlay_effect;
mod overlay_fs_effect;
mod overlay_mag;

pub use overlay_effect::{Overlay};
pub use overlay_fs_effect::FullScreenOverlay;
pub use overlay_mag::{MagEffect, MagOverlay, MAG_EFFECT_DEFAULT, MAG_EFFECT_IDENTITY};

use crate::{*, types::*};
use crate::effects::{ColorEffect};
use crate::presets::{GammaPresets, GammaPreset, GammaPresetAtomic};
use crate::sys_events::SysEventsListener;

const TIMER_TICK_MS : u32 = 16;

#[derive (Debug, Clone, Copy, FromRepr)]
#[repr (u32)]
enum DuskyReq {

    Refresh = WM_APP + 1,
    MagRefresh,

    ToggleGammaPreset,
    ApplyGammaPreset,
    CycleGammaPreset,
    RecheckGammaGuard,

    ToggleMagLevel,
    ApplyMagLevel,
    CycleMagLevel,

    ToggleFullscreenMode,
    ToggleColorEffect,
    ToggleSelectedEff,
    CycleColorEffect,

    OverlayCreate,
    OverlayRemove,

    OverlayClearAll,
    OverridesClearAll,

    UnRegisterHotkeys,
}




//#[derive (Debug)]
pub struct WinDusky {
    pub conf    : &'static config::Config,
    pub auto    : &'static auto::AutoOverlay,
    pub effects : &'static effects::ColorEffects,
    pub presets : &'static GammaPresets,

    pub fs_overlay  : &'static FullScreenOverlay,
    pub mag_overlay : &'static MagOverlay,

    thread_id : u32,

    gamma_active : Flag,
    gamma_preset : GammaPresetAtomic,
    gamma_guard_enabled : Flag,

    overlays : RwLock <HashMap <Hwnd, Overlay>>,
    hosts    : RwLock <HashSet <Hwnd>>,

    ov_topmost : HwndAtomic,
    // ^^ which overlay target hwnd (if any) we have cur set topmost

    cur_timer : AtomicUsize,
    // ^^ OS timer id changes every time we start/stop .. so a ref to cur timer to shut it later

    occl_marked : Flag,
    // ^^ whether we've been marked to have to refresh overlay occlusion calcs (based on win-events)

    pub(crate) fgnd_cache : HwndAtomic,
    // ^^ since GetForegroundWindow can return null in transitions, we'd rather act on last cached fgnd for fallback

    pub(crate) fgnd_ov_cache : HwndAtomic,
    // ^^ further, for tray action, we also want to separately remember what the last active overlay hwnd was
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
            return Err ("WinDusky was already started!!".into());
        }

        let _ = SetProcessDpiAwarenessContext (DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        if MagInitialize() == FALSE {
            return Err (format!("MagInitialize failed with error: {:?}", GetLastError()));
        }

        overlay_effect::register_overlay_class()?;

        conf.check_dusky_conf_version_match();

        let effects = effects::ColorEffects::init (conf);
        let presets = GammaPresets::init (conf);

        let fs_overlay = FullScreenOverlay::instance();
        fs_overlay.effect.store (effects.default_fs);

        let mag_overlay = MagOverlay::instance();
        mag_overlay.level.store (*MAG_EFFECT_DEFAULT);

        let auto = auto::AutoOverlay::init (conf, effects);

        let gamma_active = Flag::new (conf.check_flag__gamma_at_startup());
        let gamma_preset = GammaPresetAtomic::new (presets.default);
        let gamma_guard_enabled = Flag::new (conf.check_flag__gamma_guard_enabled());

        let dusky =  WinDusky {
            conf, auto, effects, presets, fs_overlay, mag_overlay,
            gamma_active, gamma_preset, gamma_guard_enabled,

            thread_id : GetCurrentThreadId(),

            overlays : RwLock::new (HashMap::default()),
            hosts    : RwLock::new (HashSet::default()),

            ov_topmost  : HwndAtomic::default(),
            cur_timer   : AtomicUsize::default(),
            occl_marked : Flag::new(true),

            fgnd_cache     : HwndAtomic::default(),
            fgnd_ov_cache  : HwndAtomic::default(),
        };

        Ok ( WIN_DUSKY .get_or_init (move || dusky) )
    } }




    pub fn start_win_dusky (&self) -> Result<(), String> { unsafe {

        self.register_hotkeys();

        self.setup_win_events_hooks();
        self.setup_pointer_move_hook();

        // we'll setup gamma, but only if specified active at startup (to avoid resetting otherwise)
        if self.gamma_active.is_set() { self.update_gamma_state(); }

        tray::update_tray__autostart_state();
        tray::update_tray__auto_overlay_enable (self.auto.auto_overlay_enabled.is_set());
        tray::update_tray__gamma_guard_enable (self.gamma_guard_enabled.is_set());

        // to guard gamma, if enabled, we'll just start a 1 sec poll to request a gamma guard check
        // (in theory for our initial use case, trying to listen to device driver change notif worked to catch screen off/on but felt fragile)
        thread::spawn ( || {
            let wd = WinDusky::instance();
            loop {
                thread::sleep (Duration::from_millis(3000));
                if wd.conf.check_flag__gamma_guard_enabled() {
                    wd.post_req__gamma_guard_recheck();
                }
            }
        });

        SysEventsListener::instance().setup_system_events_listener();

        // finally we just babysit the message loop
        let mut msg: MSG = std::mem::zeroed();

        loop {

            if GetMessageW (&mut msg, None, 0, 0) == false {
                let _ = MagUninitialize();
                return Err (format!("GetMessageW failed with error: {:?}", GetLastError()));
            }

            use DuskyReq::*;
            match DuskyReq::from_repr (msg.message) {

                Some (req) => match req {

                    Refresh    => { self.refresh_overlays(); }
                    MagRefresh => { self.mag_overlay.refresh_mag_overlay(); }

                    ToggleGammaPreset => { self.handle_req__toggle_gamma_active(); }
                    ApplyGammaPreset  => { self.handle_req__apply_gamma_preset (GammaPreset (msg.lParam.0 as _)); }
                    CycleGammaPreset  => { self.handle_req__cycle_gamma_preset (msg.wParam.0 != 0); }
                    RecheckGammaGuard => { self.handle_req__gamma_guard_recheck(); }

                    ToggleMagLevel => { self.handle_req__toggle_mag_overlay(); }
                    CycleMagLevel  => { self.handle_req__cycle_mag_level (msg.wParam.0 != 0); }
                    ApplyMagLevel  => { self.handle_req__apply_mag_level (MagEffect (msg.lParam.0 as _)); }

                    ToggleFullscreenMode => { self.handle_req__toggle_full_screen_mode(); }
                    ToggleColorEffect    => { self.handle_req__toggle_color_effect (Hwnd (msg.wParam.0 as _)); }
                    ToggleSelectedEff    => { self.handle_req__toggle_selected_effect (ColorEffect (msg.lParam.0 as _)); }
                    CycleColorEffect     => { self.handle_req__cycle_color_effect (msg.wParam.0 != 0); }

                    OverlayCreate => { self.handle_req__overlay_create (Hwnd (msg.wParam.0 as _), ColorEffect (msg.lParam.0 as _)); }
                    OverlayRemove => { self.handle_req__overlay_remove (Hwnd (msg.wParam.0 as _)); }

                    OverlayClearAll   => { self.handle_req__clear_overlays(); }
                    OverridesClearAll => { self.auto.clear_user_overrides(); }

                    UnRegisterHotkeys => { self.un_register_hotkeys(); },
                }

                None => match msg.message {
                    WM_TIMER        => { self.refresh_overlays(); }
                    WM_HOTKEY       => { self.handle_hotkeys (msg.wParam.0); }
                    WM_DESTROY      => { self.handle_shutdown_req(); }
                    _               => { DispatchMessageW (&msg); }
                }

            };

        }

    } }



    fn update_gamma_state (&self) {
        if let Some (preset) = self.gamma_active.is_set() .then_some ( {
            GammaPreset::from (&self.gamma_preset)
        } ) {
            info! ("Applying GammaPreset values from Preset : {:?}", preset.name());
            let spec = preset.get();
            let succeeded = gamma::set_screen_ramp_gbct (&spec.gbc, spec.color_temp);
            tray::update_tray__gamma_state (true, succeeded, Some(preset));
        } else {
            info! ("Resetting Gamma values to Normal");
            gamma::reset_screen_ramp();
            tray::update_tray__gamma_state (false, true, None);
        }
    }
    fn check_active_gamma_preset_match (&self) -> Option <bool> {
        let preset = GammaPreset::from (&self.gamma_preset) .get();
        gamma::check_active_gamma_match (&preset.gbc, preset.color_temp)
    }

    fn handle_req__toggle_gamma_active (&self) {
        if self.gamma_active.is_set() && !self.check_active_gamma_preset_match().unwrap_or_default() {
            warn! ("Gamma Preset Toggle requested, but active ramp does not match preset .. Re-applying instead !!");
            self.update_gamma_state();
            return
        }
        self.gamma_active.toggle();
        self.update_gamma_state();
    }
    fn handle_req__apply_gamma_preset (&self, preset: GammaPreset) {
        self.gamma_preset.store (preset);
        self.gamma_active.set();
        self.update_gamma_state();
    }
    fn handle_req__reapply_gamma_preset (&self) {
        self.gamma_active.set();
        self.update_gamma_state();
    }
    fn handle_req__cycle_gamma_preset (&self, forward: bool) {
        if self.gamma_active.is_clear() { return }
        self.gamma_preset.cycle (forward);
        self.update_gamma_state();
    }


    pub(crate) fn toggle_gamma_guard_enabled (&self) -> bool {
        let enabled = !self.gamma_guard_enabled.toggle();
        tray::update_tray__gamma_guard_enable (enabled);
        enabled
    }
    fn handle_req__gamma_guard_recheck (&self) {
        //if self.gamma_guard_enabled.is_set() && self.gamma_active.is_set() {
        // ^^ hmm .. we'd rather only check for guard at timer loop, and for sys-events triggers, we'll check anyway!
        if self.gamma_active.is_set() {
            if !self.check_active_gamma_preset_match().unwrap_or_default() {
                warn! ("Gamma Guard check .. active ramp does not match preset .. Re-applying preset !!");
                self.update_gamma_state();
            }
        }
    }





    fn handle_req__toggle_mag_overlay (&self) {
        let mag_eff = self.mag_overlay.toggle_effect();
        tray::update_tray__mag_level (mag_eff);
        self.post_req__mag_refresh();
    }
    fn handle_req__apply_mag_level (&self, mag_eff: MagEffect) {
        self.mag_overlay.active.set();
        self.mag_overlay.level.store (mag_eff);
        self.mag_overlay .apply_mag_level_cycled (None);
        tray::update_tray__mag_level (Some(mag_eff));
        self.post_req__mag_refresh();
    }
    fn handle_req__cycle_mag_level (&self, forward: bool) {
        //if self.mag_overlay.active.is_clear() { return }
        //^^ we'll allow direct zoom level cycling, and auto-enable it if its not enabled
        // .. but if we were inactive, we'll reset mag before we start cycling
        if !self.mag_overlay.active.is_set() {
            self.mag_overlay.level.store (*MAG_EFFECT_IDENTITY);
        }
        self.mag_overlay.active.set();
        let mag_eff = self.mag_overlay .apply_mag_level_cycled (Some(forward));
        tray::update_tray__mag_level (Some(mag_eff));
        self.post_req__mag_refresh();
    }




    pub fn check_fs_mode (&self) -> bool {
        self.fs_overlay.enabled.is_set()
    }
    pub fn get_fs_eff (&self) -> Option <ColorEffect> {
        self.fs_overlay.active.is_set() .then_some (self.fs_overlay.effect.load())
    }

    fn toggle_full_screen_effect (&self) {
        let eff = self.fs_overlay.toggle_effect();
        tray::update_tray__full_screen_mode (self.fs_overlay.enabled.is_set(), eff);
    }
    fn apply_full_screen_effect (&self, effect: ColorEffect) {
        self.fs_overlay.apply_effect (effect);
        tray::update_tray__full_screen_mode (self.fs_overlay.enabled.is_set(), Some(effect));
    }
    fn cycle_full_screen_effect (&self, forward: bool) {
        if self.fs_overlay.active.is_clear() { return }
        let eff = self.fs_overlay.apply_effect_cycled (Some(forward));
        tray::update_tray__full_screen_mode (true, Some(eff))
    }
    fn clear_full_screen_effect (&self) {
        let eff = self.fs_overlay .unapply_effect();
        tray::update_tray__full_screen_mode (false, eff);
    }
    fn handle_req__toggle_full_screen_mode (&self) {
        // Note that this to toggle the ENABLED state .. not to toggle overlay alone
        // When toggling on, it does apply overlay, and when toggling off, it does remove it..
        // .. however, one can unapply the overlay (e.g. via hotkey) w/o toggling the mode off too!
        let enabled = self.fs_overlay.toggle();
        self.fs_overlay.set_enabled(enabled);
        if enabled {
            // we gotta clear out per-hwnd overlays upon entering full-screen mode
            self.clear_hwnd_overlays();  // <- will also clear overrides
        }
        //else {}  // <- if we just toggled off fs-mode .. thats it, nothing more to do

        let effect = enabled .then_some (self.fs_overlay.effect.load());
        tray::update_tray__full_screen_mode (enabled, effect);
    }




    fn create_overlay (&self, target:Hwnd, effect:ColorEffect) {
        // Warning : This should only be called from overlay-manager thread
        if !target.is_valid() {
            warn! ("~~ WARNING ~~ Overlay creation request for {:?} .. Ignoring", &target);
            return
        }
        let mut overlays = self.overlays.write().unwrap();
        if overlays .contains_key (&target) {
            warn! ("Ignoring overlay creation request for {:?} .. Overlay already exists!!", &target);
            return
        }
        match Overlay::new (target, effect) {
            Ok (overlay) => {
                if overlays.is_empty() { self.ensure_timer_running() }
                self.hosts.write().unwrap().insert(overlay.host);
                self.occl_marked.set();
                overlays.insert (target, overlay);
                self.fgnd_ov_cache.store(target);
                tray::update_tray__overlay_created (target, effect);
                tray::update_tray__overlays_count (overlays.len());
                info! ("Created Overlay on {:?} with effect: {:?}, tot: {:?}", target, effect.name(), overlays.len());
            }
            Err (err) => {
                warn! (
                    "Failed to create Overlay on {:?} with effect: {:?}, tot: {:?} .. error: {:?}",
                    target, effect.name(), overlays.len(), err
                );
            }
        };
    }

    fn remove_overlay (&self, target:Hwnd) {
        // Warning : This should only be called from overlay-manager thread
        let mut overlays = self.overlays.write().unwrap();
        if let Some(overlay) = overlays.remove (&target) {
            overlay.destroy();
            self.hosts.write().unwrap().remove (&overlay.host);
            if self.ov_topmost.contains(target) { self.ov_topmost.clear(); }
            if self.fgnd_ov_cache.contains(target) { self.fgnd_ov_cache.clear(); }
            info! ("Removed Overlay from {:?}, tot: {:?}", overlay.target, overlays.len());
            tray::update_tray__overlay_removed (overlay.target);
        }
        if overlays.is_empty() { self.disable_timer() }
        self.occl_marked.set();
        tray::update_tray__overlays_count (overlays.len());
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

    fn handle_req__clear_overlays (&self) {
        // Reminder that this MUST be called from overlay-manager thread to have effect on overlay hwnds
        if self.fs_overlay.enabled.is_set() {
            self.clear_full_screen_effect();
        } else {
            self.clear_hwnd_overlays();
        }
    }
    fn clear_hwnd_overlays (&self) {
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
        tray::update_tray__overlays_count(0);
        self.auto.clear_user_overrides();
    }


    fn handle_req__toggle_selected_effect (&self, effect: ColorEffect) {
        if self.fs_overlay.enabled.is_set() {
            if self.fs_overlay.active.is_set() && self.fs_overlay.effect.load() == effect {
                self.toggle_full_screen_effect();
            } else {
                self.apply_full_screen_effect (effect);
            }
        } else {
            // if we're trying to toggle a selected effect (from list), it must apply to last-fgnd-ov
            let target = self.fgnd_ov_cache.load();
            if let Some(overlay) = self.overlays.write().unwrap().get_mut(&target) {
                let prior_eff = overlay.effect.load();
                if effect == prior_eff {
                    self.post_req__overlay_remove (target);
                } else {
                    overlay.apply_effect(effect);
                    tray::update_tray__overlay_cycled (target, effect);
                }
            }
        }
    }
    fn handle_req__cycle_color_effect (&self, is_fwd: bool) {
        if self.check_fs_mode() {
            self.cycle_full_screen_effect (is_fwd);
            return;
        }
        let target = self.fgnd_ov_cache.load();
        // ^^ unlike toggling effect, cycling must be on prior toggled hwnd, so we use fgnd_ov_cache instead of fgnd_cache
        if let Some(overlay) = self.overlays .read().unwrap() .get (&target) {
            let effect = overlay.apply_effect_cycled (Some(is_fwd));
            self.auto.update_cached_rule_result_effect (target, effect);
            tray::update_tray__overlay_cycled (target, effect);
        }
    }
    fn handle_req__toggle_color_effect (&self, target:Hwnd) {
        if self.fs_overlay.enabled.is_set() {
            self.toggle_full_screen_effect();
        } else {
            self.handle_req__toggle_overlay (target);
        }
    }
    fn handle_req__toggle_overlay (&self, target:Hwnd) {
        let target = if !target.is_valid() { self.fgnd_cache.load() } else { target };
        if self .overlays .read().unwrap() .contains_key (&target) {
            self.handle_req__overlay_remove (target);
        } else {
            // if there was some effect for it in eval cache, we'll use that or the overlay
            // (e.g. this would preserve last effect when the overlay might have been last toggled on/off)
            // however, if we're toggling on, we dont want to toggle to nothing .. so we'll find some default
            let effect = self.auto.check_rule_cached (target) .and_then (|r| r.effect)
                .filter (|eff| !eff.is_identity()) .unwrap_or (self.effects.default);
            self.handle_req__overlay_create (target, effect);
        }
    }
    fn handle_req__overlay_create (&self, target:Hwnd, effect:ColorEffect) {
        self.create_overlay (target, effect);
    }
    fn handle_req__overlay_remove (&self, target:Hwnd) {
        self.remove_overlay (target);
        self.auto.register_user_unapplied (target);
        self.occl_marked.set();
    }

    pub fn has_overlay (&self, hwnd: &Hwnd) -> bool {
        self.overlays .read() .is_ok_and (|ovs| ovs.contains_key (hwnd))
    }




    fn post_simple_req (&self, msg: DuskyReq) { unsafe {
        let _ = PostThreadMessageW (self.thread_id,  msg as _,  WPARAM(0),  LPARAM(0));
    } }
    fn post_cycle_req (&self, msg: DuskyReq, is_fwd: bool) { unsafe {
        let wparam = if is_fwd { 1 } else { 0 };
        let _ = PostThreadMessageW (self.thread_id,  msg as _,  WPARAM (wparam),  LPARAM(0));
    } }
    fn post_req (&self, req: DuskyReq, target: Option<Hwnd>, arg: Option<usize>) { unsafe {
        let wparam = WPARAM (target .map (|t| t.0) .unwrap_or_default() as _);
        let lparam = LPARAM (arg .unwrap_or_default() as _);
        let _ = PostThreadMessageW (self.thread_id,  req as _,  wparam,  lparam);
    } }

    pub fn post_req__refresh             (&self) { self.post_simple_req (DuskyReq::Refresh) }
    pub fn post_req__mag_refresh         (&self) { self.post_simple_req (DuskyReq::MagRefresh) }

    pub fn post_req__overlay_clear_all   (&self) { self.post_simple_req (DuskyReq::OverlayClearAll) }
    pub fn post_req__un_register_hotkeys (&self) { self.post_simple_req (DuskyReq::UnRegisterHotkeys) }

    pub fn post_req__toggle_gamma_preset (&self) { self.post_simple_req (DuskyReq::ToggleGammaPreset) }
    pub fn post_req__toggle_mag_level    (&self) { self.post_simple_req (DuskyReq::ToggleMagLevel) }
    pub fn post_req__toggle_fs_mode      (&self) { self.post_simple_req (DuskyReq::ToggleFullscreenMode) }

    pub fn post_req__toggle_color_eff    (&self, targ:Hwnd)       { self.post_req (DuskyReq::ToggleColorEffect, Some(targ), None) }
    pub fn post_req__toggle_selected_eff (&self, eff:ColorEffect) { self.post_req (DuskyReq::ToggleSelectedEff, None, Some(eff.0)) }

    pub fn post_req__cycle_gamma_preset  (&self, is_fwd:bool) { self.post_cycle_req (DuskyReq::CycleGammaPreset, is_fwd) }
    pub fn post_req__cycle_mag_level     (&self, is_fwd:bool) { self.post_cycle_req (DuskyReq::CycleMagLevel,    is_fwd) }
    pub fn post_req__cycle_color_effect  (&self, is_fwd:bool) { self.post_cycle_req (DuskyReq::CycleColorEffect, is_fwd) }

    pub fn post_req__apply_gamma_preset  (&self, gp:GammaPreset) { self.post_req (DuskyReq::ApplyGammaPreset, None, Some(gp.0)) }
    pub fn post_req__apply_mag_level     (&self, mag:MagEffect) { self.post_req (DuskyReq::ApplyMagLevel, None, Some(mag.0 as _)) }

    pub fn post_req__gamma_guard_recheck (&self) { self.post_req (DuskyReq::RecheckGammaGuard, None, None) }

    pub fn post_req__overlay_create      (&self, targ:Hwnd, eff:ColorEffect) { self.post_req (DuskyReq::OverlayCreate, Some(targ), Some(eff.0)) }
    pub fn post_req__overlay_remove      (&self, targ:Hwnd) { self.post_req (DuskyReq::OverlayRemove, Some(targ), None) }

    pub fn post_req__exit (&self) { unsafe {
        let _ = PostThreadMessageW (self.thread_id, WM_DESTROY, WPARAM (0), LPARAM(0));
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

    fn handle_shutdown_req (&self) { unsafe {
        warn!("Shutting down .. ~~~~ GOOD BYE ~~~~ !!");
        if self.gamma_active.is_set() { gamma::reset_screen_ramp(); }
        let _ = MagUninitialize();
        PostQuitMessage(0);
    } }

}





