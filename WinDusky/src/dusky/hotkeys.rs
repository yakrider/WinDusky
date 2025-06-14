

use tracing::{error, info};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};

use crate::config::HotKey;
use crate::dusky::WinDusky;

const HOTKEY_ID__FULLSCREEN_TOGGLE : usize = 1;

const HOTKEY_ID__EFFECT_TOGGLE     : usize = 2;
const HOTKEY_ID__NEXT_EFFECT       : usize = 3;
const HOTKEY_ID__PREV_EFFECT       : usize = 4;

const HOTKEY_ID__CLEAR_OVERLAYS    : usize = 5;
const HOTKEY_ID__CLEAR_OVERRIDES   : usize = 6;

const HOTKEY_ID__GAMMA_PRESET_TOGGLE : usize = 7;
const HOTKEY_ID__GAMMA_PRESET_NEXT   : usize = 8;
const HOTKEY_ID__GAMMA_PRESET_PREV   : usize = 9;

const HOTKEY_ID__MAG_LEVEL_TOGGLE : usize = 10;
const HOTKEY_ID__MAG_LEVEL_NEXT   : usize = 11;
const HOTKEY_ID__MAG_LEVEL_PREV   : usize = 12;


const HOTKEY_ID_MAX_REGISTERED : usize = HOTKEY_ID__MAG_LEVEL_PREV;



impl WinDusky {

    pub(super) fn register_hotkeys (&self) {
        // Note that this must be called from a thread that will be monitoring its msg queue
        fn register_hotkey (hotkey:HotKey, id:i32) { unsafe {
            info! ("Registering hotkey id:{:?} .. {:?}", id, &hotkey);
            if RegisterHotKey (None, id, hotkey.hk_mod() | MOD_NOREPEAT,  hotkey.key.to_vk_code() as _) .is_err() {
                error! ("Failed to register hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }
        self.conf.get_hotkey__fullscreen_toggle() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__FULLSCREEN_TOGGLE as _));
        self.conf.get_hotkey__effect_toggle()     .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__EFFECT_TOGGLE as _));

        self.conf.get_hotkey__next_effect() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__NEXT_EFFECT as _));
        self.conf.get_hotkey__prev_effect() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__PREV_EFFECT as _));

        self.conf.get_hotkey__clear_overlays()  .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERLAYS as _));
        self.conf.get_hotkey__clear_overrides() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__CLEAR_OVERRIDES as _));

        self.conf.get_hotkey__gamma_preset_toggle() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__GAMMA_PRESET_TOGGLE as _));
        self.conf.get_hotkey__next_gamma_preset()   .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__GAMMA_PRESET_NEXT as _));
        self.conf.get_hotkey__prev_gamma_preset()   .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__GAMMA_PRESET_PREV as _));

        self.conf.get_hotkey__screen_mag_toggle() .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__MAG_LEVEL_TOGGLE as _));
        self.conf.get_hotkey__next_mag_level()    .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__MAG_LEVEL_NEXT as _));
        self.conf.get_hotkey__prev_mag_level()    .into_iter().for_each (|hk| register_hotkey (hk, HOTKEY_ID__MAG_LEVEL_PREV as _));
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

        // we'll split handling into blocks based on whether they care about modes etc
        let mut done = true;

        // first lets handle hotkeys that are global regardless of full-screen or per-hwnd effects mode
        match hotkey {
            HOTKEY_ID__FULLSCREEN_TOGGLE   => { self.toggle_full_screen_mode(); }

            HOTKEY_ID__GAMMA_PRESET_TOGGLE => { self.toggle_gamma_active(); }
            HOTKEY_ID__GAMMA_PRESET_NEXT   => { self.cycle_gamma_preset (true); }
            HOTKEY_ID__GAMMA_PRESET_PREV   => { self.cycle_gamma_preset (false); }

            HOTKEY_ID__MAG_LEVEL_TOGGLE => { self.toggle_mag_overlay(); }
            HOTKEY_ID__MAG_LEVEL_NEXT   => { self.cycle_mag_level (true); }
            HOTKEY_ID__MAG_LEVEL_PREV   => { self.cycle_mag_level (false); }

            _ => { done = false; }
        }
        if done { return }

        // then handle hotkeys for full screen mode
        if self.check_fs_mode() {
            match hotkey {
                HOTKEY_ID__EFFECT_TOGGLE  => { self.toggle_full_screen_effect(); }
                HOTKEY_ID__NEXT_EFFECT    => { self.cycle_full_screen_effect (true); }
                HOTKEY_ID__PREV_EFFECT    => { self.cycle_full_screen_effect (false); }
                HOTKEY_ID__CLEAR_OVERLAYS => { self.clear_full_screen_effect(); }
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

        // now for hotkeys that need a target hwnd, we'll load from cache since GetForegroundWindow can return null during transitions etc
        // further, this lets us filter out things like krusty-qbar where we might clicking to send out dusky hotkeys etc
        let target = self.fgnd_cache.load();

        // and finally we have can process the per-hwnd hotkeys that need a hwnd target
        match hotkey {
            HOTKEY_ID__EFFECT_TOGGLE => {
                if self .overlays .read().unwrap() .contains_key (&target) {
                    self.remove_overlay (target);
                    self.auto.register_user_unapplied (target);
                } else {
                    // if there was some effect for it in eval cache, we'll use that or the overlay
                    // (e.g. this would preserve last effect when the overlay might have been last toggled on/off)
                    // however, if we're toggling on, we dont want to toggle to nothing .. so we'll find some default
                    let effect = self.auto.check_rule_cached (target) .and_then (|r| r.effect)
                        .filter (|eff| !eff.is_identity()) .unwrap_or (self.effects.default);
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
