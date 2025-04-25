#![ allow (non_camel_case_types, non_snake_case, non_upper_case_globals) ]

use itertools::Itertools;
use tracing::info;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{LazyLock, OnceLock, RwLock};
use atomic_refcell::AtomicRefCell;
use crate::config::Config;
use crate::effects::{ColorEffect, ColorEffects};
use crate::tray;
use crate::types::*;
use crate::win_utils::*;



#[derive (Debug, Clone, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum RulesKey {
    Rule_ClassId (String),
    Rule_Exe (String),
}


#[derive (Debug, Clone)]
pub struct RulesValue {
    pub enabled   : bool,
    pub effect    : Option <ColorEffect>,
    pub excl_exes : Option <HashSet <String>>,
}


#[derive (Debug, Default, Copy, Clone)]
pub struct RulesResult {

    pub enabled : bool,
    pub effect  : Option <ColorEffect>,
    // ^^ these are typically derived from looked up rules .. (whether to apply overlay, and what effect to apply)

    pub overridden : bool,
    // ^^ we'll set these for manually un-toggled overlays, and treat as hwnd-exclusions from then on

    pub elev_excl : bool,
    // ^^ we calc this for hwnds if we're not-elevated, so we can print out warnings on impossible overlay attempts
}

impl From<&RulesValue> for RulesResult {
    fn from (rv: &RulesValue) -> Self {
        RulesResult { enabled:rv.enabled, effect: rv.effect, ..RulesResult::default() }
    }
}

static effect_none : LazyLock<RulesResult> = LazyLock::new (RulesResult::default);

static effect_overriden : LazyLock<RulesResult> = LazyLock::new (|| RulesResult { overridden: true, ..RulesResult::default() });






pub struct RulesMonitor {

    elevated : Flag,
    // ^^ we can capture elevation at init, so we can decide to ignore elevated hwnds if we're not elevated

    auto_overlay_enabled : Flag,
    // ^^ whether luminance/rules based auto-overlay is to be enabled .. otherwise, only manual toggles will be available

    auto_overlay_lum__thresh : AtomicU8,
    // ^^ if enabled, we'll capture and calculate avg luminance of hwnd when first seen (and trigger auto overlay if over threshold)
    // .. A value of 0 means luminance based auto-overlay is to be disabled

    auto_overlay_lum__excl_exes : AtomicRefCell <HashSet <String>>,
    // ^^ we'll load exclusions to luminance based auto-overlay rule here
    // .. and since this is only loaded at init time, we'll use AtomicRefCell instead of RwLock for efficiency

    auto_overlay_lum__delay_ms : AtomicU32,
    // ^^ since many windows even for dark-mode apps come up white before they get painted, we'll add a configurable delay

    rules : AtomicRefCell <HashMap <RulesKey, RulesValue>>,
    // ^^ other rules based on exe or window class names can be loaded from config (to trigger auto overlays)

    eval_cache : RwLock <HashMap <Hwnd, RulesResult>>,
    // ^^ the results from evaluation of rules and/or luminance will be cached for efficiency

}


impl RulesMonitor {

    pub fn instance () -> &'static RulesMonitor {
        static INSTANCE : OnceLock <RulesMonitor> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            RulesMonitor {
                elevated : Flag::new (check_cur_proc_elevated().unwrap_or_default()),
                auto_overlay_enabled : Flag::new (true),

                auto_overlay_lum__thresh    : AtomicU8::default(),
                auto_overlay_lum__excl_exes : AtomicRefCell::new (HashSet::default()),
                auto_overlay_lum__delay_ms  : AtomicU32::default(),

                rules : AtomicRefCell::new (HashMap::default()),

                eval_cache : RwLock::new (HashMap::default()),
            }
        )
    }

    pub fn load_conf_rules (&self, conf: &Config, effects:&ColorEffects) {

        self.auto_overlay_lum__thresh .store (
            conf.get_auto_overlay_luminance__threshold(), Ordering::Relaxed
        );
        self.auto_overlay_lum__delay_ms .store (
            conf.get_auto_overlay_luminance__delay_ms(), Ordering::Relaxed
        );
        let mut lum_excl_exes = self.auto_overlay_lum__excl_exes .borrow_mut();
        conf.get_auto_overlay_luminance__exclusion_exes() .into_iter() .for_each (|s| { lum_excl_exes .insert(s); } );
        info! ("lum auto-ov excl exes: {:?}", &lum_excl_exes);

        let mut rules = self.rules.borrow_mut();
        for exe in conf.get_auto_overlay_exes() {
            //tracing::debug! ("loading auto-overlay exe rule : {:?}", exe);
            let effect = exe.effect .as_ref() .map (|s| effects.get_by_name(s));
            let _ = rules .insert (
                RulesKey::Rule_Exe (exe.exe),
                RulesValue { enabled: true, effect, excl_exes: None }
            );
        }
        for class in conf.get_auto_overlay_window_classes() {
            //tracing::debug! ("loading auto-overlay exe rule : {:?}", class);
            let excl_exes =  if !class.exclusion_exes.is_empty() {
                Some ( class.exclusion_exes.into_iter().collect::<HashSet<String>>() )
            } else { None };
            let effect = class.effect .as_ref() .map (|s| effects.get_by_name(s));
            let _ = rules .insert (
                RulesKey::Rule_ClassId (class.class),
                RulesValue { enabled: true, effect, excl_exes }
            );
        }
        info! ("The following auto-overlay rules were loaded :");
        rules .iter() .sorted_by_key (|t| t.0) .enumerate() .for_each (|(i,t)| info!("{:?}.{:?}", i+1, t));
    }

    pub fn check_auto_overlay_enabled (&self) -> bool {
        self.auto_overlay_enabled.is_set()
    }
    pub fn toggle_auto_overlay_enabled (&self) -> bool {
        ! self.auto_overlay_enabled.toggle()
    }

    pub fn get_auto_overlay_delay_ms (&self) -> u32 {
        self.auto_overlay_lum__delay_ms.load(Ordering::Relaxed)
    }


    pub fn register_user_unapplied (&self, hwnd:Hwnd) {
        info! ("Registering user un-toggle of overlay: {:?} .. (Override added!)", hwnd);
        let mut eval_cache = self.eval_cache.write().unwrap();
        if let Some(result) = eval_cache .get_mut (&hwnd) {
            result.enabled = false; result.overridden = true;
        } else {
            eval_cache .insert (hwnd, *effect_overriden);
        }
        let n_overrides = eval_cache .iter() .filter (|(_,r)| r.overridden) .count();
        tray::update_tray__overrides_count(n_overrides);
    }
    pub fn clear_user_overrides (&self) {
        let mut eval_cache = self.eval_cache.write().unwrap();
        let n_overrides = eval_cache .iter() .filter (|(_,r)| r.overridden) .count();
        info! ("Clearing all {:?} user-initiated rules overrides (and {:?} hwnd eval results)!", n_overrides, eval_cache.len());
        eval_cache .clear();
        tray::update_tray__overrides_count(0);
    }


    pub fn check_rule_cached (&self, hwnd: Hwnd) -> Option <RulesResult> {
        let eval_cache = self.eval_cache.read().unwrap();
        eval_cache .get (&hwnd) .copied()
    }
    pub fn update_cached_rule_result_effect (&self, hwnd: Hwnd, effect:ColorEffect) {
        if let Some(result) = self.eval_cache.write().unwrap().get_mut(&hwnd) {
            //tracing::debug!("found cached result for {:?} .. {:?}", hwnd, &result);
            result.effect.replace(effect);
        }
    }

    pub fn re_check_rule (&self, hwnd: Hwnd) -> RulesResult {
        let mut eval_cache = self.eval_cache.write().unwrap();

        let mut result = self.eval_rules (hwnd);

        if result.enabled && result.effect.is_none() {
            let effect = Some (ColorEffects::instance().get_default());
            result = RulesResult { overridden: false, effect, ..result }
        }
        eval_cache .insert (hwnd, result);
        result
    }

    fn eval_rules (&self, hwnd:Hwnd) -> RulesResult {

        if !check_window_visible(hwnd) || check_window_cloaked(hwnd) {
            return *effect_none
        }
        let elev_excl = self.elevated.is_clear() && check_hwnd_elevated(hwnd).unwrap_or_default();

        let exe = get_exe_by_hwnd(hwnd);

        if !self.auto_overlay_lum__excl_exes .borrow() .contains (exe.as_ref().unwrap()) {
            let lum_thresh = self.auto_overlay_lum__thresh .load (Ordering::Relaxed);
            if lum_thresh > 0 {
                if let Some (lum) = calculate_avg_luminance (hwnd) {
                    //tracing::debug! ("got luminance {:?} for {:?}", lum, hwnd);
                    if lum > lum_thresh {
                        info! ("Found avg luminance of {:?} for {:?} .. will auto-apply an overlay!", lum, hwnd);
                        return RulesResult { enabled:true, elev_excl, ..RulesResult::default() }
                    }
                }
            }
        }

        let class = get_win_class_by_hwnd (hwnd);

        if let Some(result) = self.rules.borrow() .get (& RulesKey::Rule_ClassId (class)) {
            if exe.is_some() && result.excl_exes.as_ref().is_some_and (|h| h.contains(&exe.unwrap())) {
                return *effect_none
            }
            return RulesResult { elev_excl, ..result.into() };
        }

        if let Some(exe) = get_exe_by_hwnd(hwnd) {
            if let Some(result) = self.rules.borrow() .get (& RulesKey::Rule_Exe(exe)) {
                return RulesResult { elev_excl, ..result.into() };
            }
        }

        *effect_none
    }


}







