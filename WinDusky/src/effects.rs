#![ allow (dead_code) ]

use itertools::Itertools;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use tracing::info;
use windows::Win32::UI::Magnification::MAGCOLOREFFECT;

use crate::*;



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



#[derive (Debug)]
pub struct ColorEffects {
    pub effects     : HashMap <String, MAGCOLOREFFECT>,
    pub cycle_order : Vec <(String, MAGCOLOREFFECT)>,
    pub default     : ColorEffect,
}

static COLOR_EFFECTS : OnceLock <ColorEffects> = OnceLock::new();

impl ColorEffects {

    pub fn instance() -> &'static ColorEffects {
       COLOR_EFFECTS .get() .expect ("ColorEffects not iniitialized yet !!")
    }

    pub fn init (conf: &config::Config) -> &'static ColorEffects {

        // lets load all the color-effects specified in conf first
        let mut effects : HashMap <String, MAGCOLOREFFECT> = HashMap::new();
        for effect in conf.get_color_effects() {
            let _ = effects .insert ( effect.name, MAGCOLOREFFECT { transform: effect.transform } );
        }
        // if no effects were defined, we'll at least populate with a default (simple inversion)
        if effects.is_empty() {
            let _ = effects .insert ( "DEFAULT".into(), COLOR_EFF__FALLBACK_DEFAULT );
        }

        // next we'll load the defined cycle-order
        let mut cycle_order = conf.get_effects_cycle_order() .into_iter()
            .filter_map (|s| effects .get(&s) .map (|v| (s,*v))) .collect::<Vec<_>>();

        // but it it was empty or not specified, lets just put all defined effects in cycle-order
        if cycle_order.is_empty() {
            for (name, effect) in effects.iter() {
                cycle_order .push ((name.clone(), *effect));
            }
        }
        info! ("loaded color-effects in cycle-order: {:?}", cycle_order.iter().map(|(s,_)| s).collect::<Vec<_>>());

        let default_effect = &conf.get_effects_default();
        let default_id = cycle_order .iter().find_position (|(s,_)| s == default_effect) .map (|(idx, _)| idx) .unwrap_or(0);
        let default = ColorEffect (default_id);
        info! ("loaded default color-effect as : {:?}", (default, &default_effect));

        COLOR_EFFECTS .get_or_init ( || ColorEffects { effects, cycle_order, default } )

    }

    pub fn get_by_name (&self, name: &str) -> ColorEffect {
        // if the conf specifies a valid default effect, we'll use that, else we'll use the first entry in cycle order
        let idx = self.cycle_order .iter() .find_position (|(s,_)| s == name) .map (|(idx, _)| idx) .unwrap_or_default();
        ColorEffect (idx)
    }

}





#[derive (Debug, Default, Copy, Clone)]
pub struct ColorEffect (pub usize);

#[derive (Debug, Default)]
pub struct ColorEffectAtomic (AtomicUsize);


impl ColorEffect {
    pub fn new (idx:usize) -> ColorEffect {
        ColorEffect(idx)
    }
    pub fn get (&self) -> MAGCOLOREFFECT {
        let cycler = &ColorEffects::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(_,v)| *v) .unwrap_or (COLOR_EFF__FALLBACK_DEFAULT)
    }
    pub fn name (&self) -> &'static str {
        let cycler = &ColorEffects::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(s,_)| s.as_str()) .unwrap_or("")
    }
}

impl From <&ColorEffectAtomic> for ColorEffect {
    fn from (eff: &ColorEffectAtomic) -> Self {
        ColorEffect (eff.0 .load(Ordering::Relaxed))
    }
}

impl ColorEffectAtomic {

    pub fn new (effect : ColorEffect) -> ColorEffectAtomic {
        ColorEffectAtomic (AtomicUsize::new (effect.0))
    }
    pub fn store (&self, effect : ColorEffect) {
        self.0.store (effect.0, Ordering::Release);
    }

    pub fn get (&self) -> MAGCOLOREFFECT {
        ColorEffect::from(self).get()
    }

    pub fn cycle (&self, forward: bool) -> ColorEffect {
        let cyc_len = ColorEffects::instance().cycle_order.len();
        // now there no adding negatives for usize, but we can substract positives, hence using incr+1 below
        let incr_plus_one = if forward { 2 } else { 0 };   // either [1 or -1], then add 1
        let prior_idx = self.0.fetch_update (
            Ordering::AcqRel, Ordering::Acquire,
            |cur| Some ((cur + cyc_len + incr_plus_one - 1) % cyc_len)
        );
        let new_idx = (prior_idx.unwrap() + cyc_len + incr_plus_one - 1) % cyc_len;
        ColorEffect (new_idx)
    }
    pub fn cycle_next (&self) -> ColorEffect { self.cycle (true) }
    pub fn cycle_prev (&self) -> ColorEffect { self.cycle (false) }
}
