

use tracing::{error, info, warn};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::config::HotKey;
use crate::dusky::WinDusky;
use crate::effects::ColorEffect;
use crate::tray;
use crate::types::*;

const HOTKEY_ID__TOGGLE            : usize = 1;
const HOTKEY_ID__FULLSCREEN_TOGGLE : usize = 2;
const HOTKEY_ID__NEXT_EFFECT       : usize = 3;
const HOTKEY_ID__PREV_EFFECT       : usize = 4;
const HOTKEY_ID__CLEAR_OVERLAYS    : usize = 5;
const HOTKEY_ID__CLEAR_OVERRIDES   : usize = 6;

const HOTKEY_ID_MAX_REGISTERED : usize = HOTKEY_ID__CLEAR_OVERRIDES;



impl WinDusky {

    pub(super) fn register_hotkeys (&self) {
        // Note that this must be called from a thread that will be monitoring its msg queue
        fn register_hotkey (hotkey:HotKey, id:i32) { unsafe {
            info! ("Registering hotkey id:{:?} .. {:?}", id, &hotkey);
            if RegisterHotKey (None, id, hotkey.hk_mod() | MOD_NOREPEAT,  hotkey.key.to_vk_code() as _) .is_err() {
                error! ("Failed to register hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }
        self.conf.get_hotkey__dusky_toggle()      .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__TOGGLE as _));
        self.conf.get_hotkey__fullscreen_toggle() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__FULLSCREEN_TOGGLE as _));

        self.conf.get_hotkey__next_effect() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__NEXT_EFFECT as _));
        self.conf.get_hotkey__prev_effect() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__PREV_EFFECT as _));

        self.conf.get_hotkey__clear_overlays()  .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERLAYS as _));
        self.conf.get_hotkey__clear_overrides() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERRIDES as _));
    }

    pub(super) fn un_register_hotkeys (&self) {
        // NOTE that this must be called from the same thread that registered the hotkeys!
        fn un_register_hotkey (id:i32) { unsafe {
            if UnregisterHotKey (None, id) .is_err() {
                error! ("Failed to unregister hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }
        info! ("Attempting to un-register all WinDusky hotkeys");
        for id in 1 .. HOTKEY_ID_MAX_REGISTERED + 1 {
            un_register_hotkey (id as _);
        }
    }



    pub(super) fn handle_hotkeys (&self, hotkey: usize) {

        // we'll split handling into those for mode toggle, and for each modes

        // .. first the global full-screen mode toggle
        if hotkey == HOTKEY_ID__FULLSCREEN_TOGGLE {
            self.toggle_full_screen_mode();
            return;
        }

        // then handle hotkeys for full screen mode
        if self.check_fs_mode() {
            fn update_tray (enabled:bool, eff: Option <ColorEffect>) {
                tray::update_full_screen_mode (enabled, eff.map(|e| e.name()))
            }
            match hotkey {
                HOTKEY_ID__TOGGLE         => { update_tray (true, self.fs_overlay .toggle_effect() ) }
                HOTKEY_ID__NEXT_EFFECT    => { update_tray (true, self.fs_overlay .apply_effect_next() ) }
                HOTKEY_ID__PREV_EFFECT    => { update_tray (true, self.fs_overlay .apply_effect_prev() ) }
                HOTKEY_ID__CLEAR_OVERLAYS => { self.fs_overlay .unapply_effect(); update_tray (false, None); }
                _ => {}
            }
            return;
        }

        // now for per-hwnd mode, first lets handle hotkeys that dont need a target
        match hotkey {
            HOTKEY_ID__CLEAR_OVERLAYS => {
                self.clear_overlays();
                // ^^ also clears cache and user overrides
            }
            HOTKEY_ID__CLEAR_OVERRIDES => {
                self.auto.clear_user_overrides();
            }
            _ => { }
        }

        // and now for those that need a target hwnd, we'll return early if there's no active fgnd
        let target : Hwnd = unsafe { GetForegroundWindow().into() };
        if !target.is_valid() {
            warn! ("~~ WARNING ~~ Hotkey received, but no active foreground found .. Ignoring!!");
            return
        }

        // and finally we have the rest of the per-hwnd hotkeys
        match hotkey {
            HOTKEY_ID__TOGGLE => {
                if self .overlays .read().unwrap() .contains_key (&target) {
                    self.remove_overlay (target);
                    self.auto.register_user_unapplied (target);
                } else {
                    // if there was some effect for it in eval cache, we'll use that or the overlay
                    // (e.g. this would preserve last effect when the overlay might have been last toggled on/off)
                    let effect = self.auto.check_rule_cached (target) .and_then (|r| r.effect) .unwrap_or (self.effects.default);
                    self.create_overlay (target, effect);
                }
            }
            HOTKEY_ID__NEXT_EFFECT => {
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target) {
                    let effect = overlay.apply_effect_next();
                    self.auto.update_cached_rule_result_effect (target, effect);
                }
            }
            HOTKEY_ID__PREV_EFFECT => {
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target) {
                    let effect = overlay.apply_effect_prev();
                    self.auto.update_cached_rule_result_effect (target, effect);
                }
            }
            _ => { }
        }
    }

}
