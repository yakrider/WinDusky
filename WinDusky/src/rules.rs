#![ allow (non_camel_case_types, non_snake_case, non_upper_case_globals) ]


use itertools::Itertools;
use tracing::info;

use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

use windows::core::PSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameA, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

use crate::config::Config;
use crate::effects::{ColorEffect, ColorEffects};
use crate::tray;
use crate::types::*;

#[derive (Debug, Clone, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum RulesKey {
    Rule_ClassId (String),
    Rule_Exe (String),
    //Rule_Exe_Title { exe: String, title: Option<String> },
}
// ^^ todo : constructing title checks for all windows is too expensive, just cant do that ..
// .. so instead, could make a table where we do just the exe check first, and if it suggests title lookup, then we do title check



#[derive (Debug, Clone)]
pub struct RulesValue {
    pub enabled   : bool,
    pub effect    : Option <ColorEffect>,
    pub excl_exes : Option<HashSet<String>>,
}


#[derive (Debug, Copy, Clone)]
pub struct RulesResult {
    pub enabled    : bool,
    pub overridden : bool,
    pub effect     : Option <ColorEffect>,
}
impl From<&RulesValue> for RulesResult {
    fn from (rv: &RulesValue) -> Self {
        RulesResult { enabled: rv.enabled, overridden: false, effect: rv.effect }
    }
}

const effect_none      : RulesResult = RulesResult { enabled: false, overridden: false, effect: None};
const effect_overriden : RulesResult = RulesResult { enabled: false, overridden: true,  effect: None};




pub struct RulesMonitor {
    rules : RwLock <HashMap <RulesKey, RulesValue>>,
    eval_cache : RwLock <HashMap <Hwnd, RulesResult>>,
}


impl RulesMonitor {

    pub fn instance () -> &'static RulesMonitor {
        static INSTANCE : OnceLock <RulesMonitor> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            RulesMonitor {
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

    pub fn _check_rule (&self, hwnd: Hwnd) -> RulesResult {
        let mut eval_cache = self.eval_cache.write().unwrap();
        if let Some (result) = eval_cache .get (&hwnd) {
            *result
        } else {
            let result = self.eval_rules (hwnd);
            eval_cache .insert (hwnd, result);
            result
        }
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
            result = RulesResult {enabled: true, overridden: false, effect}
        }
        eval_cache .insert (hwnd, result);
        result
    }

    fn eval_rules (&self, hwnd:Hwnd) -> RulesResult {

        if !check_window_visible(hwnd) || check_window_cloaked(hwnd) {
            return effect_none
        }

        let (class, exe) = (get_win_class_by_hwnd (hwnd), get_exe_by_hwnd (hwnd));

        if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_ClassId (class)) {
            if exe.is_some() && result.excl_exes.as_ref().is_some_and (|h| h.contains(&exe.unwrap())) {
                return effect_none
            }
            return result.into();
        }

        if let Some(exe) = get_exe_by_hwnd(hwnd) {
            if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_Exe(exe)) {
                return result.into();
            }
        }

        effect_none
    }


}





pub fn check_window_visible (hwnd:Hwnd) -> bool { unsafe {
    IsWindowVisible (hwnd.into()) .as_bool()
} }

pub fn check_window_cloaked (hwnd:Hwnd) -> bool { unsafe {
    let mut cloaked_state: isize = 0;
    let out_ptr = &mut cloaked_state as *mut isize as *mut _;
    let _ = DwmGetWindowAttribute (hwnd.into(), DWMWA_CLOAKED, out_ptr, size_of::<isize>() as u32);
    cloaked_state != 0
} }


#[allow (dead_code)]
pub fn get_win_title (hwnd:Hwnd) -> String { unsafe {
    const MAX_LEN : usize = 512;
    let mut lpstr : [u16; MAX_LEN] = [0; MAX_LEN];
    let copied_len = GetWindowTextW (hwnd.into(), &mut lpstr);
    String::from_utf16_lossy (&lpstr[..(copied_len as _)])
} }


pub fn get_win_class_by_hwnd (hwnd:Hwnd) -> String { unsafe {
    let mut lpstr: [u16; 120] = [0; 120];
    let len = GetClassNameW (hwnd.into(), &mut lpstr);
    String::from_utf16_lossy(&lpstr[..(len as _)])
} }


pub fn get_exe_by_hwnd (hwnd:Hwnd) -> Option<String> {
    get_exe_by_pid ( get_pid_by_hwnd (hwnd))
}

pub fn get_exe_by_pid (pid:u32) -> Option<String> { unsafe {
    let handle = OpenProcess (PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    let mut lpstr: [u8; 256] = [0; 256];
    let mut lpdwsize = 256u32;
    if handle.is_err() { return None }
    let _ = QueryFullProcessImageNameA ( HANDLE (handle.as_ref().unwrap().0), PROCESS_NAME_WIN32, PSTR::from_raw(lpstr.as_mut_ptr()), &mut lpdwsize );
    if let Ok(h) = handle { let _ = CloseHandle(h); }
    PSTR::from_raw(lpstr.as_mut_ptr()).to_string() .ok() .and_then (|s| s.split("\\").last().map(|s| s.to_string()))
} }

pub fn get_pid_by_hwnd (hwnd:Hwnd) -> u32 { unsafe {
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId (hwnd.into(), Some(&mut pid));
    pid
} }











