#![allow (non_camel_case_types, non_snake_case, non_upper_case_globals)]

use itertools::Itertools;
use tracing::{info, warn};

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, OnceLock, RwLock};
use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::dusky::WinDusky;
use crate::effects::{ColorEffect, ColorEffects};
use crate::luminance::calculate_avg_luminance;
use crate::tray::*;
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






pub struct AutoOverlay {

    pub elevated : bool,
    // ^^ we can capture elevation at init, so we can decide to ignore elevated hwnds if we're not elevated

    pub auto_overlay_enabled : Flag,
    // ^^ whether luminance/rules based auto-overlay is to be enabled .. otherwise, only manual toggles will be available

    pub auto_overlay_lum__thresh : u8,
    // ^^ if enabled, we'll capture and calculate avg luminance of hwnd when first seen (and trigger auto overlay if over threshold)
    // .. A value of 0 means luminance based auto-overlay is to be disabled

    pub auto_overlay_lum__use_bitblt : bool,
    // ^^ whether the confs specify to use BitBlt (the altternate method) instead of the default PrintWindow

    pub auto_overlay_lum__delay_ms : u32,
    // ^^ since many windows even for dark-mode apps come up white before they get painted, we'll add a configurable delay

    auto_overlay_lum__excl_exes : HashSet <String>,
    // ^^ we'll load exclusions to luminance based auto-overlay rule here
    // .. and since this is only loaded at init time, we'll use AtomicRefCell instead of RwLock for efficiency

    rules : HashMap <RulesKey, RulesValue>,
    // ^^ other rules based on exe or window class names can be loaded from config (to trigger auto overlays)

    eval_cache : RwLock <HashMap <Hwnd, RulesResult>>,
    // ^^ the results from evaluation of rules and/or luminance will be cached for efficiency

}


static AUTO_OVERLAY : OnceLock <AutoOverlay> = OnceLock::new();

impl AutoOverlay {

    #[allow (dead_code)]
    pub fn instance() -> &'static AutoOverlay {
        AUTO_OVERLAY .get() .expect ("AutoOverlay not initialised yet !!")
    }

    pub fn init (conf: &Config, effects:&ColorEffects) -> &'static AutoOverlay {

        let elevated = check_cur_proc_elevated().unwrap_or_default();

        let auto_overlay_lum__thresh     = conf.get_auto_overlay_luminance__threshold();
        let auto_overlay_lum__delay_ms   = conf.get_auto_overlay_luminance__delay_ms();
        let auto_overlay_lum__use_bitblt = conf.get_auto_overlay_luminance__use_alternate();

        let mut auto_overlay_lum__excl_exes : HashSet <String> = HashSet::new();
        conf.get_auto_overlay_luminance__exclusion_exes() .into_iter() .for_each (|s| { auto_overlay_lum__excl_exes .insert(s); } );
        info! ("lum auto-ov excl exes: {:?}", &auto_overlay_lum__excl_exes);

        let mut rules : HashMap <RulesKey, RulesValue> = HashMap::new();
        for exe in conf.get_auto_overlay_exes() {
            //tracing::debug! ("loading auto-overlay exe rule : {:?}", exe);
            let effect = exe.effect .as_ref() .map (|s| effects.find_by_name(s));
            let _ = rules .insert (
                RulesKey::Rule_Exe (exe.exe),
                RulesValue { enabled: true, effect, excl_exes: None }
            );
        }
        for class in conf.get_auto_overlay_window_classes() {
            //tracing::debug! ("loading auto-overlay exe rule : {:?}", class);
            let excl_exes =  (!class.exclusion_exes.is_empty()) .then_some (class.exclusion_exes.into_iter().collect::<HashSet<String>>());
            let effect = class.effect .as_ref() .map (|s| effects.find_by_name(s));
            let _ = rules .insert (
                RulesKey::Rule_ClassId (class.class),
                RulesValue { enabled: true, effect, excl_exes }
            );
        }
        info! ("The following auto-overlay rules were loaded :");
        rules .iter() .sorted_by_key (|t| t.0) .enumerate() .for_each (|(i,t)| info!("{:?}.{:?}", i+1, t));

        let auto_overlay_enabled = Flag::new (auto_overlay_lum__thresh > 0  ||  !rules.is_empty());

        let eval_cache = RwLock::new (HashMap::default());

        AUTO_OVERLAY.get_or_init ( move ||
            AutoOverlay {
                elevated, auto_overlay_enabled, auto_overlay_lum__thresh, auto_overlay_lum__excl_exes,
                auto_overlay_lum__use_bitblt, auto_overlay_lum__delay_ms, rules, eval_cache,
            }
        )

    }

    pub fn toggle_auto_overlay_enabled (&self) -> bool {
        let enabled = !self.auto_overlay_enabled.toggle();
        update_tray__auto_overlay_enable (enabled);
        enabled
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
        update_tray__overrides_count(n_overrides);
    }
    pub fn clear_user_overrides (&self) {
        let mut eval_cache = self.eval_cache.write().unwrap();
        let n_overrides = eval_cache .iter() .filter (|(_,r)| r.overridden) .count();
        info! ("Clearing all {:?} user-initiated rules overrides (and {:?} hwnd eval results)!", n_overrides, eval_cache.len());
        eval_cache .clear();
        update_tray__overrides_count(0);
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

        let mut result = self.eval_rules (hwnd);

        let mut eval_cache = self.eval_cache.write().unwrap();
        if result.enabled && result.effect.is_none() {
            let effect = Some (ColorEffects::instance().default);
            result = RulesResult { overridden: false, effect, ..result }
        }
        eval_cache .insert (hwnd, result);
        result
    }

    fn eval_rules (&self, hwnd:Hwnd) -> RulesResult {

        //tracing::debug! ("Evaluating rules for new {:?}", hwnd);

        if !check_window_visible(hwnd) || check_window_cloaked(hwnd) {
            return *effect_none
        }
        let Some(info) = get_proc_info(hwnd) else {
            return *effect_none
        };

        let elev_excl = !self.elevated && info.elev;

        if !self.auto_overlay_lum__excl_exes .contains (&info.exe) {
            if self.auto_overlay_lum__thresh > 0 {
                if let Some (lum) = calculate_avg_luminance (hwnd, self.auto_overlay_lum__use_bitblt) {
                    //tracing::debug! ("got luminance {:?} for {:?}", lum, hwnd);
                    if lum != u8::MIN  &&  lum != u8::MAX  &&  lum > self.auto_overlay_lum__thresh {
                        // ^^ we disable [0, 255] values as that typically means the window hasnt painted itself etc
                        // ^^ and since we do multiple evals for first-seen hwnds, we'll just come back to this later
                        info! ("Found avg luminance of {:?} for {:?} .. will auto-apply an overlay!", lum, hwnd);
                        return RulesResult { enabled:true, elev_excl, ..RulesResult::default() }
                    }
                }
            }
        }

        let class = get_win_class_by_hwnd (hwnd);

        if let Some(result) = self.rules .get (& RulesKey::Rule_ClassId (class)) {
            if result.excl_exes.as_ref().is_some_and (|h| h.contains(&info.exe)) {
                return *effect_none
            }
            return RulesResult { elev_excl, ..result.into() };
        }

        if let Some(result) = self.rules .get (& RulesKey::Rule_Exe (info.exe)) {
            return RulesResult { elev_excl, ..result.into() };
        }

        *effect_none
    }


    pub fn handle_auto_overlay (&'static self, hwnd:Hwnd, wd: &'static WinDusky) {

        // So we got an hwnd that doesnt have overlay yet, and we wanna see if auto-overlay rules apply to it

        if wd.check_fs_mode() { return }
        if !self.auto_overlay_enabled.is_set() { return }

        // next we'll check if we have have evaluated auto-overlay rules for this previously
        let result = self.check_rule_cached (hwnd);

        if let Some ( RulesResult { enabled: false, .. } ) = result {
            return;
        }
        else if let Some ( RulesResult { enabled: true, effect, ..} ) = result {
            wd.post_req__overlay_create (hwnd, effect.unwrap_or (wd.effects.default));
            return
        }


        // so looks like this is first ever fgnd for this, so we'd like to eval from scratch ..
        // .. but eval for luminance requies screen cap, so we'll spawn thread to do all that
        thread::spawn ( move || {
            //tracing::debug! ("Processing Auto-Overlay for new {:?}", hwnd);

            // further, doing a screen cap too early (esp with BitBlt) can capture not-quite-painted hwnds
            // .. so we'll put up a small delay before we go about the hwnd screen capture business
            thread::sleep (Duration::from_millis (self.auto_overlay_lum__delay_ms as _));

            let result = self.re_check_rule(hwnd);

            // but we'll ditch early if elevation restrictions apply (i.e this guy is elev but we're not)
            if let RulesResult { elev_excl: true, .. } = result {
                warn! ("!! WARNING !! .. WinDusky is NOT Elevated. Cannot overlay elevated {:?}", hwnd);
                return;
            }

            // otherwise, if it passed rules, we can go ahead and request an overlay creation
            if let RulesResult { enabled: true, effect, .. } = result {
                wd.post_req__overlay_create (hwnd, effect.unwrap_or (wd.effects.default));
            }

            // however, as seen before, it takes time for some windows to get all their properties after newly created hwnds report fgnd
            // .. so we'll just sit on delays and check it a couple times (just like done in switche/krusty etc)
            // The easiest way to test the utility of this is prob to start something like perfmon.exe w/ and w/o delay-waits

            thread::sleep (Duration::from_millis(300));

            if wd.has_overlay (&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.re_check_rule(hwnd) {
                wd.post_req__overlay_create (hwnd, effect.unwrap_or (wd.effects.default));
            }

            thread::sleep (Duration::from_millis(500));

            if wd.has_overlay(&hwnd) { return }
            if let RulesResult { enabled: true, effect, .. } = self.re_check_rule(hwnd) {
                wd.post_req__overlay_create (hwnd, effect.unwrap_or (wd.effects.default))
            }

        } );

    }


}







