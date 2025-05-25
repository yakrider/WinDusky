use std::ptr::null;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, OnceLock};
use tracing::{error, info};

use windows::Win32::Foundation::{GetLastError, POINT};
use windows::Win32::UI::Magnification::{MagSetFullscreenTransform, MagSetInputTransform};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::types::Flag;
use crate::win_utils::get_pointer_loc;




#[derive (Debug)]
pub struct MagOverlay {
    pub active : Flag,
    pub level  : MagEffectAtomic,
}



const MAG_SCALING_STEP : f32   = 1.10_f32;   // ten-percent magnification per step
const MAX_MAG_LEVELS   : usize = 32;         // max number of magnification steps allowed


#[derive (Debug, Copy, Clone, PartialEq, Eq)]
/// Magnification as how many levels of <?>% increment to apply <br>
/// e.g. 5 -> 1.10^8 = ~2.14x
pub struct MagEffect (pub u8);

#[derive (Debug)]
pub struct MagEffectAtomic (AtomicU8);


pub static MAG_EFFECT_IDENTITY : LazyLock <MagEffect> = LazyLock::new (|| MagEffect(0));
pub static MAG_EFFECT_DEFAULT  : LazyLock <MagEffect> = LazyLock::new (|| MagEffect(1));




impl MagEffect {
    pub fn get (&self) -> f32 {
        MAG_SCALING_STEP .powf (self.0 as f32)
    }
    pub fn is_identity (&self) -> bool {
        *self == *MAG_EFFECT_IDENTITY
    }
}

impl From <&MagEffectAtomic> for MagEffect {
    fn from (eff: &MagEffectAtomic) -> Self {
        MagEffect (eff.0 .load(Ordering::Relaxed))
    }
}



impl MagEffectAtomic {

    pub fn new (effect : MagEffect) -> MagEffectAtomic {
        MagEffectAtomic (AtomicU8::new (effect.0))
    }
    pub fn store (&self, effect : MagEffect) {
        self.0.store (effect.0, Ordering::Release);
    }

    pub fn get (&self) -> f32 {
        MagEffect::from(self).get()
    }

    pub fn cycle (&self, forward: bool) -> MagEffect {
        // for magnification, we'll use non-wrapping cycling (i.e. clamp at [min,max] extremes)
        let incr = if forward { 1 } else { -1};
        let update_fn = |cur| Some ((cur as isize + incr) .clamp (0, MAX_MAG_LEVELS as isize) as u8);
        let prior_level = self.0.fetch_update ( Ordering::AcqRel, Ordering::Acquire, update_fn );
        let new_level = update_fn (prior_level.unwrap_or_else (|e| e)) .unwrap();
        MagEffect (new_level)
    }
}





impl MagOverlay {

    pub(super) fn instance() -> &'static MagOverlay {
        static INSTANCE : OnceLock <MagOverlay> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            MagOverlay {
                active : Flag::default(),
                level  : MagEffectAtomic::new (MagEffect(0)),
                //pointer_pos: Mutex::new((0, 0)),
            }
        )
    }


    pub fn refresh_mag_overlay (&self) { unsafe {

        // REMINDER that this will work ONLY from the same thread that did the MagInit !!

        if self.active.is_clear() {
            let _ = MagSetFullscreenTransform (MAG_EFFECT_IDENTITY.get(), 0, 0);
            let _ = MagSetInputTransform (false, null(), null());
            return;
        }

        let mag = self.level.get();
        let POINT {x, y} = get_pointer_loc();

        let (screen_w, screen_h) = (GetSystemMetrics (SM_CXSCREEN),  GetSystemMetrics (SM_CYSCREEN));
        let (src_w, src_h) = ((screen_w as f32 / mag) as i32,  (screen_h as f32 / mag) as i32);
        let x_offset = (x - src_w/2) .clamp (0, screen_w - src_w);
        let y_offset = (y - src_h/2) .clamp (0, screen_h - src_h);

        if !MagSetFullscreenTransform (mag, x_offset, y_offset) .as_bool() {
            error! ("Error setting Screen Magnification transform: {:?}", GetLastError());
        }
        // // we'll also setup input transform for pen/touch inputs (mouse works even w/o this)
        // let mag_src = RECT { left: x_offset, top: y_offset, right: x_offset + src_w, bottom: y_offset + src_h };
        // let mag_dst = RECT { left: 0, top: 0, right: screen_w, bottom: screen_h };
        // if MagSetInputTransform (true, &mag_src, &mag_dst) .is_err() {
        //     error! ("Error setting Screen Magnification InputTransform: {:?}", GetLastError());
        // }
        // ^^ well, turns out that requires an app manifest requesting uiAccess=true ..
        // .. however, having that requires the app to have code-signing, installation in secure dir etc etc
        // .. so for now, we'll just ignore it .. dont think touch while zoomed is that all big a deal
    } }


    pub(super) fn toggle_effect (&self) -> Option <MagEffect> {
        // we only toggle off if we were active and the mag was valid (ie. > 1.0)
        let cur_mag_valid = !MagEffect::from(&self.level).is_identity();
        if self.active.is_set() && cur_mag_valid {
            self.unapply_effect();
            return None;
        }
        // else, we'll apply some effect, and if the mag was invalid, we'll apply default mag
        if !cur_mag_valid {
            self.level .store (*MAG_EFFECT_DEFAULT)
        }
        Some ( self.apply_mag_level_cycled (None) )
    }

    pub(super) fn unapply_effect (&self) -> Option<MagEffect> {
        let prior = (&self.level).into();
        self.active.clear();
        info! ("Clearing Magnification Overlay!");
        //let _ = unsafe { MagSetFullscreenTransform (MAG_EFFECT_IDENTITY.get(), 0, 0) };
        // ^^ since we might be getting called from non-mag-init-thread, the caller should instead post req for mag refresh
        Some (prior)
    }

    pub(super) fn apply_mag_level_cycled (&self, forward: Option<bool>) -> MagEffect {
        let level = if let Some(forward) = forward { self.level.cycle (forward) } else { (&self.level).into() };
        self.active .store (level != *MAG_EFFECT_IDENTITY);
        info! ("Setting Screen Magnification to Level:{level:?}, i.e {:.2}x", level.get());
        //self.refresh_mag_overlay();
        // ^^ since we might be getting called from non-mag-init-thread, the caller should instead post req for mag refresh
        level
    }

}



