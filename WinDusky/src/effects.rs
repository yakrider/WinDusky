
use itertools::Itertools;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock};
use tracing::info;
use windows::Win32::UI::Magnification::MAGCOLOREFFECT;

use crate::*;
use crate::config::DynamicEffectSpec;





#[derive (Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct ColorEffect (pub usize);



#[derive (Debug, Default)]
pub struct ColorEffectAtomic (AtomicUsize);



#[derive (Debug)]
pub struct ColorEffects {
    //pub effects     : HashMap <String, MAGCOLOREFFECT>,
    // ^^ can add back if we need by-name lookup .. for now, we'll just put the data into cycle-order vec itself
    pub cycle_order : Vec <(String, MAGCOLOREFFECT)>,
    pub default     : ColorEffect,
    pub default_fs  : ColorEffect,
}



// there is no turning-off of full-screen color effect, it should simply be set to identity
pub const COLOR_EFF__IDENTITY : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
     1.0,  0.0,  0.0,  0.0,  0.0,
     0.0,  1.0,  0.0,  0.0,  0.0,
     0.0,  0.0,  1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     0.0,  0.0,  0.0,  0.0,  1.0,
] };

// we'll also define the simple inversion in code itself as fallback default for missing configs etc
pub const COLOR_EFF__SIMPLE_INVERSION : MAGCOLOREFFECT = MAGCOLOREFFECT { transform: [
    -1.0,  0.0,  0.0,  0.0,  0.0,
     0.0, -1.0,  0.0,  0.0,  0.0,
     0.0,  0.0, -1.0,  0.0,  0.0,
     0.0,  0.0,  0.0,  1.0,  0.0,
     1.0,  1.0,  1.0,  1.0,  1.0,
] };
pub const COLOR_EFF__FALLBACK_DEFAULT : MAGCOLOREFFECT = COLOR_EFF__SIMPLE_INVERSION;



static COLOR_EFFECTS : OnceLock <ColorEffects> = OnceLock::new();




impl ColorEffect {
    pub fn get (&self) -> MAGCOLOREFFECT {
        let cycler = &ColorEffects::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(_,v)| *v) .unwrap_or (COLOR_EFF__FALLBACK_DEFAULT)
    }
    pub fn name (&self) -> &'static str {
        let cycler = &ColorEffects::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(s,_)| s.as_str()) .unwrap_or("")
    }
    pub fn is_identity (&self) -> bool {
        self.get() == COLOR_EFF__IDENTITY
    }
}



impl ColorEffectAtomic {

    pub fn new (effect : ColorEffect) -> ColorEffectAtomic {
        ColorEffectAtomic (AtomicUsize::new (effect.0))
    }
    pub fn load (&self) -> ColorEffect {
        ColorEffect (self.0 .load (Ordering::Relaxed))
    }
    pub fn store (&self, effect : ColorEffect) {
        self.0.store (effect.0, Ordering::Release);
    }

    pub fn get (&self) -> MAGCOLOREFFECT {
        self.load().get()
    }

    pub fn cycle (&self, forward: bool) -> ColorEffect {
        let cyc_len = ColorEffects::instance().cycle_order.len();
        let incr = if forward { cyc_len + 1 } else { cyc_len -1 };
        let update_fn = |cur| Some ((cur + incr) % cyc_len);
        let prior_idx = self.0.fetch_update (Ordering::AcqRel, Ordering::Acquire, update_fn);
        let new_idx = update_fn (prior_idx.unwrap_or_else(|e| e)) .unwrap();
        ColorEffect (new_idx)
    }
}




impl ColorEffects {

    pub fn instance() -> &'static ColorEffects {
       COLOR_EFFECTS .get() .expect ("ColorEffects not initialized yet !!")
    }

    pub fn init (conf: &config::Config) -> &'static ColorEffects {

        // lets load all the color-effects specified in conf first
        let mut effects : HashMap <String, MAGCOLOREFFECT> = HashMap::new();
        for effect in conf.get_color_effects() {
            let _ = effects .insert ( effect.name, effect.effect );
        }
        // if no effects were defined, we'll at least populate with a default (simple inversion)
        if effects.is_empty() {
            let _ = effects .insert ( "DEFAULT".into(), COLOR_EFF__FALLBACK_DEFAULT );
        }
        info! ("loaded color-effects (alphabetically) : {:?}", effects.keys().sorted());

        // next we'll load the defined cycle-order
        let mut cycle_order = conf.get_effects_cycle_order() .into_iter()
            .filter_map (|s| effects .get(&s) .map (|v| (s,*v))) .collect::<Vec<_>>();

        // but if it was empty or not specified, lets just put all defined effects in cycle-order
        if cycle_order.is_empty() {
            for (name, effect) in effects.iter() {
                cycle_order .push ((name.clone(), *effect));
            }
            cycle_order .sort_by (|a,b| a.0.cmp(&b.0));
        }
        info! ("color-effects cycle-order: {:?}", cycle_order .iter() .map (|(s,_)| s) .collect::<Vec<_>>());

        let default_effect = &conf.get_effects_default();
        let default_id = cycle_order .iter().find_position (|(s,_)| s == default_effect) .map (|(idx, _)| idx) .unwrap_or(0);
        let default = ColorEffect (default_id);
        info! ("loaded default color-effect as : {:?}", (default, &default_effect));

        let default_fs_effect = &conf.get_effects_full_screen_default();
        let default_fs_id = cycle_order .iter().find_position (|(s,_)| s == default_fs_effect) .map (|(idx, _)| idx) .unwrap_or(0);
        let default_fs = ColorEffect (default_fs_id);
        info! ("loaded default full-screen color-effect as : {:?}", (default_fs, &default_fs_effect));

        COLOR_EFFECTS .get_or_init ( || ColorEffects { cycle_order, default, default_fs } )

    }

    pub fn find_by_name (&self, name: &str) -> ColorEffect {
        // if the conf specifies a valid default effect, we'll use that, else we'll use the first entry in cycle order
        let idx = self.cycle_order .iter() .find_position (|(s,_)| s == name) .map (|(idx, _)| idx) .unwrap_or_default();
        ColorEffect (idx)
    }

}





#[allow (unused)]
fn mix_effects (a: &MAGCOLOREFFECT, b: &MAGCOLOREFFECT, t: f32) -> MAGCOLOREFFECT {
    let mut transform = [0.0f32; 25];
    for (i, v) in transform.iter_mut().enumerate() {
        *v = a.transform[i] * (1.0 - t) + b.transform[i] * t;
    }
    MAGCOLOREFFECT { transform }
}


pub fn compose_effects (a: &MAGCOLOREFFECT, b: &MAGCOLOREFFECT) -> MAGCOLOREFFECT {
    let mut transform = [0.0f32; 25];
    for row in 0..5 {
        for col in 0..5 {
            transform [row * 5 + col] = (0..5).map(|k| {
                a.transform[row * 5 + k] * b.transform[k * 5 + col]
            }).sum();
        }
    }
    MAGCOLOREFFECT { transform }
}


pub fn compose_dynamic (spec:&DynamicEffectSpec) -> MAGCOLOREFFECT {
    let mut eff = hue_rotation (spec.hue_rotate);
    eff = compose_effects (&saturation (spec.saturation), &eff);
    eff = compose_effects (&contrast   (spec.contrast),   &eff);
    eff = compose_effects (&brightness (spec.brightness), &eff);
    eff
}




fn hue_rotation (theta: f32) -> MAGCOLOREFFECT {

    let theta = theta.to_radians();
    let a = (1.0 + 2.0 * theta.cos()) / 3.0;
    let b = (1.0 - theta.cos()) / 3.0;
    let c = theta.sin() / (3f32.sqrt());

    MAGCOLOREFFECT { transform : [
        a,       b - c,    b + c,    0.0,   0.0,
        b + c,   a,        b - c,    0.0,   0.0,
        b - c,   b + c,    a,        0.0,   0.0,
        0.0,     0.0,      0.0,      1.0,   0.0,
        0.0,     0.0,      0.0,      0.0,   1.0,
    ] }
}


fn brightness (lvl: f32) -> MAGCOLOREFFECT {

    // dynamic spec params are [0.0 .. 2.0], but we want brightness values between [-1.0 .. 1.0]
    let lvl = (lvl - 1.0) .clamp (-1.0, 1.0);

    MAGCOLOREFFECT { transform : [
        1.0,     0.0,     0.0,    0.0,   0.0,
        0.0,     1.0,     0.0,    0.0,   0.0,
        0.0,     0.0,     1.0,    0.0,   0.0,
        0.0,     0.0,     0.0,    1.0,   0.0,
        lvl,     lvl,     lvl,    0.0,   1.0,
    ] }
}


fn contrast (lvl: f32) -> MAGCOLOREFFECT {

    let lvl = lvl .clamp (0.0, 2.0);
    let bias = (1.0 - lvl) / 2.0;

    MAGCOLOREFFECT { transform : [
        lvl,     0.0,     0.0,    0.0,   0.0,
        0.0,     lvl,     0.0,    0.0,   0.0,
        0.0,     0.0,     lvl,    0.0,   0.0,
        0.0,     0.0,     0.0,    1.0,   0.0,
       bias,    bias,    bias,    0.0,   1.0,
    ] }
}


fn saturation (lvl: f32) -> MAGCOLOREFFECT {

    // we'll use standard luminosity profile
    //let (lr, lg, lb) = (0.213, 0.715, 0.072);
    let (lr, lg, lb)   = (0.309, 0.609, 0.082);

    let s = lvl .clamp (0.0, 2.0);
    let ss = 1.0 - s;
    let (sr, sg, sb) = (ss * lr,  ss * lg,  ss * lb);

    MAGCOLOREFFECT { transform : [
        s + sr,       sr,       sr,   0.0,   0.0,
            sg,   s + sg,       sg,   0.0,   0.0,
            sb,       sb,   s + sb,   0.0,   0.0,
           0.0,      0.0,      0.0,   1.0,   0.0,
           0.0,      0.0,      0.0,   0.0,   1.0,
    ] }
}

