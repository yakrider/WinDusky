
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, OnceLock};

use itertools::Itertools;
use tracing::info;

use crate::config::{self, GammaPresetSpec};
use crate::gamma;




pub static PRESET_NORMAL : LazyLock <GammaPresetSpec> = LazyLock::new ( ||
    GammaPresetSpec {
        preset: "Normal".into(),
        gbc: gamma::GBC::default(),
        color_temp: 6500,
    }
);




#[derive (Debug)]
pub struct GammaPresets {
    //pub presets     : HashMap <String, GammaPresetSpec>,
    // ^^ can add back if we need by-name lookup .. for now, we'll just put the data into cycle-order vec itself
    pub cycle_order : Vec <(String, GammaPresetSpec)>,
    pub default     : GammaPreset,
}



static GAMMA_PRESETS: OnceLock<GammaPresets> = OnceLock::new();


impl GammaPresets {

    pub fn instance() -> &'static GammaPresets {
        GAMMA_PRESETS .get() .expect("GammaPresets not initialized yet!")
    }

    pub fn init (conf: &config::Config) -> &'static GammaPresets {

        // Load all gamma presets defined in the configuration.
        let mut presets : HashMap <String, GammaPresetSpec> = HashMap::new();
        for preset in conf .get_gamma_presets() {
            let _ = presets .insert (preset.preset.clone(), preset);
        }
        // If no presets are found in the config, add the fallback "Normal" preset.
        if presets.is_empty() {
            presets.insert ( PRESET_NORMAL.preset.clone(), PRESET_NORMAL.clone() );
        }
        info! ("Loaded gamma presets (alphabetically) : {:?}", presets.keys().sorted());

        // next we'll load the defined cycle-order
        let mut cycle_order = conf .get_gamma_cycle_order() .into_iter()
            .filter_map (|s| presets .get(&s) .map (|v| (s.clone(), v.clone()))) .collect::<Vec<_>>();

        // but if it was empty or not specified, lets just pull all the defined effects in cycle-order
        if cycle_order .is_empty() {
            for (name, preset) in presets.iter() {
                cycle_order .push ((name.clone(), preset.clone()));
            }
            cycle_order .sort_by (|a,b| a.0.cmp(&b.0));
        }
        info! ("gamma presets cycle-order: {:?}", cycle_order .iter() .map (|(s, _)| s) .collect::<Vec<_>>());

        let default_preset = &conf.get_gamma_default();
        let default_idx = cycle_order .iter() .find_position(|(s, _)| s == default_preset) .map(|(idx, _)| idx).unwrap_or(0);
        let default = GammaPreset (default_idx);
        info! ("loaded default gamma-preset as : {:?}", (default, &default_preset));

        GAMMA_PRESETS .get_or_init ( || GammaPresets { cycle_order, default })
    }

}





#[derive (Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct GammaPreset (pub usize);

#[derive (Debug, Default)]
pub struct GammaPresetAtomic (AtomicUsize);



impl GammaPreset {
    pub fn get(&self) -> GammaPresetSpec {
        let cycler = &GammaPresets::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(_, spec)| spec.clone()) .unwrap_or (PRESET_NORMAL.clone())
    }
    pub fn name(&self) -> &'static str {
        let cycler = &GammaPresets::instance().cycle_order;
        cycler .get (self.0 % cycler.len()) .map (|(name, _)| name.as_str()) .unwrap_or ("")
    }
}

impl From <&GammaPresetAtomic> for GammaPreset {
    fn from (preset: &GammaPresetAtomic) -> Self {
        GammaPreset (preset.0 .load (Ordering::Relaxed))
    }
}


impl GammaPresetAtomic {

    #[allow (dead_code)]
    pub fn new (preset: GammaPreset) -> GammaPresetAtomic {
        GammaPresetAtomic (AtomicUsize::new(preset.0))
    }
    pub fn store (&self, preset: GammaPreset) {
        self.0.store (preset.0, Ordering::Release);
    }

    pub fn cycle (&self, forward: bool) -> GammaPreset {
        let cyc_len = GammaPresets::instance().cycle_order.len();
        let incr = if forward { cyc_len + 1 } else { cyc_len - 1 };
        let update_fn = |cur| Some ((cur + incr) % cyc_len);
        let prior_idx = self.0.fetch_update (Ordering::AcqRel, Ordering::Acquire, update_fn);
        let new_idx = update_fn (prior_idx.unwrap_or_else(|e| e)) .unwrap();
        GammaPreset (new_idx)
    }

}
