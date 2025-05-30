
use strum::EnumCount;
use strum_macros::{EnumCount, EnumIter, FromRepr};
use tracing::{error, info, warn};

use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};

use crate::config::HotKey;
use crate::dusky::WinDusky;




#[derive (Debug, Clone, Copy, PartialEq, Eq, EnumIter, FromRepr, EnumCount)]
#[repr (usize)]
enum HotKeyId {

    FullscreenToggle = 1,
    ClearOverlays,
    ClearOverrides,

    EffectToggle,
    EffectNext,
    EffectPrev,

    GammaPresetToggle,
    GammaPresetNext,
    GammaPresetPrev,

    MagLevelToggle,
    MagLevelNext,
    MagLevelPrev,
}




impl WinDusky {

    pub(super) fn register_hotkeys(&self) {
        // Note that this must be called from a thread that will be monitoring its msg queue

        fn register_hotkey (id: HotKeyId, hotkey: Option <HotKey>) { unsafe {
            let Some(hotkey) = hotkey else {
                warn! ("Failed to parse hotkey to register for id: {id:?} !!");
                return
            };
            info! ("Registering hotkey id:{:?} .. {:?}", id, &hotkey);
            if RegisterHotKey (
                None,  id as _,  hotkey.hk_mod() | MOD_NOREPEAT,  hotkey.key.to_vk_code() as _
            ) .is_err() {
                error! ("Failed to register hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }

        use HotKeyId::*;

        register_hotkey ( FullscreenToggle,   self.conf.get_hotkey__fullscreen_toggle()   );
        register_hotkey ( ClearOverlays,      self.conf.get_hotkey__clear_overlays()      );
        register_hotkey ( ClearOverrides,     self.conf.get_hotkey__clear_overrides()     );

        register_hotkey ( EffectToggle,       self.conf.get_hotkey__effect_toggle()       );
        register_hotkey ( EffectNext,         self.conf.get_hotkey__next_effect()         );
        register_hotkey ( EffectPrev,         self.conf.get_hotkey__prev_effect()         );

        register_hotkey ( GammaPresetToggle,  self.conf.get_hotkey__gamma_preset_toggle() );
        register_hotkey ( GammaPresetNext,    self.conf.get_hotkey__next_gamma_preset()   );
        register_hotkey ( GammaPresetPrev,    self.conf.get_hotkey__prev_gamma_preset()   );

        register_hotkey ( MagLevelToggle,     self.conf.get_hotkey__screen_mag_toggle()   );
        register_hotkey ( MagLevelNext,       self.conf.get_hotkey__next_mag_level()      );
        register_hotkey ( MagLevelPrev,       self.conf.get_hotkey__prev_mag_level()      );

    }



    pub(super) fn un_register_hotkeys (&self) {
        // NOTE that this must be called from the same thread that registered the hotkeys!
        fn un_register_hotkey (id:i32) { unsafe {
            if UnregisterHotKey (None, id) .is_err() {
                error! ("Failed to unregister hotkey id:{:?} .. {:?}", id, GetLastError());
            }
        } }
        info! ("Attempting to un-register all WinDusky hotkeys");
        for id in 1 .. HotKeyId::COUNT + 1 {
            un_register_hotkey (id as _);
        }
    }



    pub(super) fn handle_hotkeys (&self, hotkey: usize) {
        use HotKeyId::*;

        let Some (hotkey) = HotKeyId::from_repr (hotkey) else { return };

        match hotkey {
            FullscreenToggle  => { self.handle_req__toggle_full_screen_mode(); }
            ClearOverlays     => { self.handle_req__clear_overlays(); }   // <- also clears cache and user overrides
            ClearOverrides    => { self.auto.clear_user_overrides(); }

            // Note that for hotkey effect toggle, we specifically use fgnd-cache hwnd, not last-active-overlay hwnd (as from tray)
            // (and if we're in fullscreen-overlay mode, the hwnd gets ignored anyway)
            EffectToggle  => { self.handle_req__toggle_color_effect (self.fgnd_cache.load()); }
            EffectNext    => { self.handle_req__cycle_color_effect (true ); }
            EffectPrev    => { self.handle_req__cycle_color_effect (false); }

            GammaPresetToggle  => { self.handle_req__toggle_gamma_active(); }
            GammaPresetNext    => { self.handle_req__cycle_gamma_preset (true ); }
            GammaPresetPrev    => { self.handle_req__cycle_gamma_preset (false); }

            MagLevelToggle  => { self.handle_req__toggle_mag_overlay(); }
            MagLevelNext    => { self.handle_req__cycle_mag_level (true ); }
            MagLevelPrev    => { self.handle_req__cycle_mag_level (false); }
        }

    }

}
