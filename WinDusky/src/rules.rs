#![ allow (non_camel_case_types, non_snake_case, non_upper_case_globals) ]


use itertools::Itertools;
use tracing::info;

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

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
    pub excl_exes : Option<HashSet<String>>,
}


#[derive (Debug, Copy, Clone)]
pub struct RulesResult {

    pub enabled : bool,
    pub effect  : Option <ColorEffect>,

    pub overridden : bool,
    // ^^ we'll set these for manually un-toggled overlays, and treat as hwnd-exclusions from then on

    pub elev_excl : bool,
    // ^^ we calc this for hwnds if we're not-elevated, so we can print out warnings on impossible overlay attempts
}

impl From<&RulesValue> for RulesResult {
    fn from (rv: &RulesValue) -> Self {
        RulesResult { enabled:rv.enabled, effect: rv.effect, overridden:false, elev_excl:false }
    }
}

const effect_none      : RulesResult = RulesResult { enabled: false, effect: None, overridden: false, elev_excl: false };
const effect_overriden : RulesResult = RulesResult { enabled: false, effect: None, overridden: true,  elev_excl: false };




pub struct RulesMonitor {
    elevated   : Flag,
    rules      : RwLock <HashMap <RulesKey, RulesValue>>,
    eval_cache : RwLock <HashMap <Hwnd, RulesResult>>,
}


impl RulesMonitor {

    pub fn instance () -> &'static RulesMonitor {
        static INSTANCE : OnceLock <RulesMonitor> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            RulesMonitor {
                elevated   : Flag::new (check_cur_proc_elevated().unwrap_or_default()),
                rules      : RwLock::new (HashMap::default()),
                eval_cache : RwLock::new (HashMap::default()),
            }
        )
    }

    pub fn load_conf_rules (&self, conf: &Config, effects:&ColorEffects) {
        let mut rules = self.rules.write().unwrap();

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
            let excl_exes =  if !class.exe_exclusions.is_empty() {
                Some ( class.exe_exclusions.into_iter().collect::<HashSet<String>>() )
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


    pub fn register_user_unapplied (&self, hwnd:Hwnd) {
        info! ("Registering user un-toggle of overlay: {:?} .. (Override added!)", hwnd);
        let mut eval_cache = self.eval_cache.write().unwrap();
        eval_cache .insert (hwnd, effect_overriden);
        let n_overrides = eval_cache .iter() .filter (|(_,r)| r.overridden) .count();
        tray::update_tray__overrides_count(n_overrides);
    }
    pub fn clear_user_overrides (&self) {
        let mut eval_cache = self.eval_cache.write().unwrap();
        let n_overrides = eval_cache .iter() .filter (|(_,r)| r.overridden) .count();
        info! ("Clearing all {:?} user-initiated rules overrides!", n_overrides);
        eval_cache .clear();
        tray::update_tray__overrides_count(0);
    }


    pub fn check_rule_cached (&self, hwnd: Hwnd) -> Option <RulesResult> {
        let eval_cache = self.eval_cache.read().unwrap();
        eval_cache .get (&hwnd) .copied()
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
            return effect_none
        }
        let (class, exe) = (get_win_class_by_hwnd (hwnd), get_exe_by_hwnd (hwnd));
        let elev_excl = self.elevated.is_clear() && check_hwnd_elevated(hwnd).unwrap_or_default();

        if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_ClassId (class)) {
            if exe.is_some() && result.excl_exes.as_ref().is_some_and (|h| h.contains(&exe.unwrap())) {
                return effect_none
            }
            return RulesResult { overridden: false, elev_excl, ..result.into() };
        }

        if let Some(exe) = get_exe_by_hwnd(hwnd) {
            if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_Exe(exe)) {
                return RulesResult { overridden: false, elev_excl, ..result.into() };
            }
        }

        effect_none
    }


}







