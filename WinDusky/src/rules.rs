#![ allow (non_camel_case_types, non_snake_case, non_upper_case_globals) ]

use crate::effects::ColorEffect;
use crate::types::*;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{OnceLock, RwLock};
use windows::core::PSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameA, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};





#[derive (Debug, Clone, Hash, Ord, PartialOrd, Eq, PartialEq)]
pub enum RulesKey {
    Rule_ClassId (String),
    Rule_Exe (String),
    //Rule_Exe_Title { exe: String, title: Option<String> },
}
// ^^ todo : constructing title checks for all windows is too expensive, just cant do that ..
// .. so instead, could make a table where we do just the exe check first, and if it suggests title lookup, then we do title check



#[derive (Debug, Copy, Clone)]
pub struct RulesValue {
    pub enabled : bool,
    pub effect  : Option <ColorEffect>,
}

const effect_none : RulesValue = RulesValue { enabled: false, effect: None };




pub struct RulesMonitor {
    rules : RwLock <HashMap <RulesKey, RulesValue>>,
    eval_cache : RwLock <HashMap <Hwnd, RulesValue>>,
}


impl RulesMonitor {

    pub fn instance () -> &'static RulesMonitor {
        static INSTANCE : OnceLock <RulesMonitor> = OnceLock::new();
        let dm = INSTANCE .get_or_init ( ||
            RulesMonitor {
                rules      : RwLock::new (HashMap::default()),
                eval_cache : RwLock::new (HashMap::default()),
            }
        );
        dm.load_default_rules();
        dm
    }

    fn load_default_rules (&self) {

        let mut rules = self.rules.write().unwrap();

        rules .insert (
            RulesKey::Rule_ClassId ("#32770".into()),    // Dialog hwnd classes
            RulesValue { enabled: true, effect: Some (ColorEffect::default()) }
        );

        for exe in ["msinfo32.exe", "regedit.exe", "mmc.exe", "WinaeroTweaker.exe"] {
            rules .insert (
                RulesKey::Rule_Exe (exe.into()),
                RulesValue { enabled: true, effect: Some (ColorEffect::default()) }
            );
        }
    }

    pub fn register_user_unapplied (&self, hwnd:Hwnd) {
        let mut eval_cache = self.eval_cache.write().unwrap();
        eval_cache .insert (hwnd, effect_none);
    }
    pub fn clear_rule_overrides (&self) {
        let mut eval_cache = self.eval_cache.write().unwrap();
        eval_cache .clear();
    }

    pub fn _check_rule (&self, hwnd: Hwnd) -> RulesValue {
        let mut eval_cache = self.eval_cache.write().unwrap();
        if let Some (result) = eval_cache .get (&hwnd) {
            *result
        } else {
            let result = self.eval_rules (hwnd);
            eval_cache .insert (hwnd, result);
            result
        }
    }
    pub fn check_rule_cached (&self, hwnd: Hwnd) -> Option <RulesValue> {
        let eval_cache = self.eval_cache.read().unwrap();
        eval_cache .get (&hwnd) .copied()
    }
    pub fn re_check_rule (&self, hwnd: Hwnd) -> RulesValue {
        let mut eval_cache = self.eval_cache.write().unwrap();
        let result = self.eval_rules (hwnd);
        eval_cache .insert (hwnd, result);
        result
    }

    fn eval_rules (&self, hwnd:Hwnd) -> RulesValue {

        if !check_window_visible(hwnd) || check_window_cloaked(hwnd) {
            return effect_none
        }

        let class_id = get_win_class_by_hwnd(hwnd);
        if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_ClassId (class_id)) {
            return *result;
        }

        if let Some(exe) = get_exe_by_hwnd(hwnd) {
            if let Some(result) = self.rules.read().unwrap() .get (& RulesKey::Rule_Exe(exe)) {
                return *result;
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
    let out_ptr = &mut cloaked_state as *mut isize as *mut c_void;
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











