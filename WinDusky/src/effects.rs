#![ allow (dead_code) ]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use itertools::Itertools;
use tracing::info;
use windows::Win32::UI::Magnification::MAGCOLOREFFECT;
use crate::config;




// we'll define the simple inversion in code itself as fallback default for missing configs etc
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
    effects     : RwLock <HashMap <String, MAGCOLOREFFECT>>,
    cycle_order : RwLock <Vec <(String, MAGCOLOREFFECT)>>,
    default     : ColorEffectAtomic,
}



impl ColorEffects {

    pub fn instance() -> &'static ColorEffects {
        static INSTANCE: OnceLock <ColorEffects> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            ColorEffects {
                effects     : RwLock::new (HashMap::new()),
                cycle_order : RwLock::new (Vec::new()),
                default     : ColorEffectAtomic::default(),
            }
        )
    }

    pub fn load_effects_from_conf (&self, conf: &config::Config) {
        // lets load all the color-effects specified in conf first
        let mut effects = self.effects.write().unwrap();
        for effect in conf.get_color_effects() {
            let _ = effects .insert ( effect.name, MAGCOLOREFFECT { transform: effect.transform } );
        }
        // if no effects were defined, we'll at least populate with a default (simple inversion)
        if effects.is_empty() {
            let _ = effects .insert ( "Identity".into(), COLOR_EFF__FALLBACK_DEFAULT );
        }

        // next we'll load the defined cycle-order
        let mut cycle_order = self.cycle_order.write().unwrap();
        *cycle_order = conf.get_effects_cycle_order() .into_iter()
            .filter_map (|s| effects .get(&s) .map (|v| (s,*v))) .collect();

        // but it it was empty or not specified, lets just put all defined effects in cycle-order
        if cycle_order.is_empty() {
            for (name, effect) in effects.iter() {
                cycle_order .push ((name.clone(), *effect));
            }
        }
        info! ("loaded color-effects in cycle-order: {:?}", cycle_order.iter().map(|(s,_)| s).collect::<Vec<_>>());
        drop (cycle_order);

        let default_effect = &conf.get_effects_default();
        let default_id = self.get_by_name(default_effect);
        info! ("loaded default color-effect as : {:?}", (default_id, &default_effect));
        self.default .store (self.get_by_name (default_effect));
    }

    pub fn get_by_name (&self, name: &str) -> ColorEffect {
        // if the conf specifies a valid default effect, we'll use that, else we'll use the first entry in cycle order
        let cycle_order = self.cycle_order.read().unwrap();
        let idx = cycle_order.iter().find_position (|(s,_)| s == name) .map (|(idx, _)| idx) .unwrap_or_default();
        ColorEffect(idx)
    }

    pub fn get_default (&self) -> ColorEffect {
        (&self.default).into()
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
        let effs = ColorEffects::instance();
        let cycler = effs.cycle_order.read().unwrap();
        cycler .get (self.0 % cycler.len()) .map (|(_,v)| *v) .unwrap_or (COLOR_EFF__FALLBACK_DEFAULT)
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
        let effs = ColorEffects::instance();
        let cycler = effs.cycle_order.read().unwrap();
        let cyc_len = cycler.len();

        // now there no adding negatives for usize, but we can substract positives, hence using incr+1 below
        let incr_plus_one = if forward { 2 } else { 0 };   // either [1 or -1], then add 1
        let prior_idx = self.0.fetch_update (
            Ordering::AcqRel, Ordering::Acquire,
            |cur| Some ((cur + cyc_len + incr_plus_one - 1) % cyc_len)
        );
        let new_idx = (prior_idx.unwrap() + cyc_len - incr_plus_one + 1) % cyc_len;
        ColorEffect (new_idx)
    }
    pub fn cycle_next (&self) -> ColorEffect { self.cycle (true) }
    pub fn cycle_prev (&self) -> ColorEffect { self.cycle (false) }
}
