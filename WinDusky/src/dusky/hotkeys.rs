

use tracing::{error, info};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::config::HotKey;
use crate::dusky::WinDusky;



const HOTKEY_ID__TOGGLE          : usize = 1;
const HOTKEY_ID__NEXT_EFFECT     : usize = 2;
const HOTKEY_ID__PREV_EFFECT     : usize = 3;
const HOTKEY_ID__CLEAR_OVERLAYS  : usize = 4;
const HOTKEY_ID__CLEAR_OVERRIDES : usize = 5;
const HOTKEY_ID__CLEAR_ALL       : usize = 6;

const HOTKEY_ID_MAX_REGISTERED : usize = HOTKEY_ID__CLEAR_ALL;



impl WinDusky {

    pub(super) fn register_hotkeys (&self) {
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
        let target = unsafe { GetForegroundWindow().into() };
        match hotkey {
            HOTKEY_ID__TOGGLE => {
                if self .overlays .read().unwrap() .contains_key (&target) {
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
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target) {
                    let effect = overlay.apply_effect_next();
                    self.rules.update_cached_rule_result_effect (target, effect);
                }
            }
            HOTKEY_ID__PREV_EFFECT => {
                if let Some(overlay) = self.overlays .read().unwrap() .get (&target) {
                    let effect = overlay.apply_effect_prev();
                    self.rules.update_cached_rule_result_effect (target, effect);
                };
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

}
