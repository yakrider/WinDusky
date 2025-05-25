

use std::sync::OnceLock;
use tracing::{error, info};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Magnification::{MagSetFullscreenColorEffect, MAGCOLOREFFECT};
use crate::effects::{ColorEffect, ColorEffectAtomic, ColorEffects, COLOR_EFF__IDENTITY};
use crate::types::Flag;




#[derive (Debug, Default)]
pub struct FullScreenOverlay {
    pub enabled : Flag,
    pub active  : Flag,
    pub effect  : ColorEffectAtomic,
}



impl FullScreenOverlay {

    pub(super) fn instance() -> &'static FullScreenOverlay {
        static INSTANCE : OnceLock <FullScreenOverlay> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            FullScreenOverlay {
                enabled : Flag::default(),
                active  : Flag::default(),
                effect  : ColorEffectAtomic::new (ColorEffects::instance().default),
            }
        )
    }

    /// toggles full screen effect enabled state and returns the updated state
    pub(super) fn toggle (&self) -> bool {
        let enabled = !self.enabled.toggle();
        self.active .store (enabled);
        info! ("Setting FULL-SCREEN_OVERLAY mode to : {} !!", if enabled {"ON"} else {"OFF"} );
        if enabled { self.toggle_effect_active(); }
        else { self.apply_color_effect (COLOR_EFF__IDENTITY) };
        enabled
    }

    pub(super) fn set_enabled (&self, enabled: bool) {
        let prior = self.enabled.swap(enabled);
        self.active .store (enabled);
        if prior != enabled {
            info! ("Setting FULL-SCREEN_OVERLAY mode to : {} !!", if enabled {"ON"} else {"OFF"} );
            if enabled { self.toggle_effect_active(); }
            else { self.apply_color_effect (COLOR_EFF__IDENTITY) };
        }
    }

    /// toggles the effect applied full screen (does not affect the enabled state itself!)
    pub(super) fn toggle_effect (&self) -> Option <ColorEffect> {
        if self.active.is_set() {
            self.unapply_effect();
            return None
        }
        Some (self.toggle_effect_active())
    }

    fn toggle_effect_active (&self) -> ColorEffect {
        if ColorEffect::from (&self.effect) .is_identity() {
            self.effect.store (ColorEffects::instance().default)
        }
        self.apply_effect_cycled (None)
    }

    pub(super) fn unapply_effect (&self) -> Option<ColorEffect> {
        let prior = (&self.effect).into();
        info! ("Clearing Full Screen Overlay color effect .. (the mode remains active)!");
        self.active.clear();
        self.apply_color_effect (COLOR_EFF__IDENTITY);
        Some (prior)
    }

    fn apply_color_effect (&self, effect: MAGCOLOREFFECT) { unsafe {
        if ! MagSetFullscreenColorEffect (&effect) .as_bool() {
            error! ("Error settting Fullscreen Color Effect : {:?}", GetLastError());
        }
    } }

    pub(super) fn apply_effect_cycled (&self, forward: Option<bool>) -> ColorEffect {
        let effect = if let Some(forward) = forward { self.effect.cycle (forward) } else { (&self.effect).into() };
        info! ("Setting Full Screen Overlay Color Effect to : {:?}", effect.name());
        self.apply_color_effect (effect.get());
        self.active.set();
        effect
    }

}



