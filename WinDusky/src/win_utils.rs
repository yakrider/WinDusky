#![allow (dead_code, non_snake_case)]

use std::collections::HashSet;
use std::ffi::OsStr;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use crate::types::Hwnd;
use std::os::windows::prelude::OsStrExt;
use windows::core::{BOOL, PSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameA, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, EnumWindows, GetClassNameW, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOPMOST};





// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
pub fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}


pub fn win_check_if_topmost (hwnd: Hwnd) -> bool { unsafe {
    GetWindowLongW (hwnd.into(), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == WS_EX_TOPMOST.0
} }

pub fn check_window_visible (hwnd:Hwnd) -> bool { unsafe {
    IsWindowVisible (hwnd.into()) .as_bool()
} }

pub fn check_window_cloaked (hwnd:Hwnd) -> bool { unsafe {
    let mut cloaked_state: isize = 0;
    let out_ptr = &mut cloaked_state as *mut isize as *mut _;
    let _ = DwmGetWindowAttribute (hwnd.into(), DWMWA_CLOAKED, out_ptr, size_of::<isize>() as u32);
    cloaked_state != 0
} }


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


pub fn get_pid_by_hwnd (hwnd:Hwnd) -> u32 { unsafe {
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId (hwnd.into(), Some(&mut pid));
    pid
} }

pub fn get_exe_by_pid (pid:u32) -> Option<String> { unsafe {
    let handle = OpenProcess (PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    let mut lpstr: [u8; 256] = [0; 256];
    let mut lpdwsize = 256u32;
    if handle.is_err() { return None }
    let _ = QueryFullProcessImageNameA ( HANDLE (handle.as_ref().unwrap().0), PROCESS_NAME_WIN32, PSTR::from_raw(lpstr.as_mut_ptr()), &mut lpdwsize );
    if let Ok(h) = handle { let _ = CloseHandle(h); }
    PSTR::from_raw(lpstr.as_mut_ptr()).to_string() .ok() .and_then (|s| s.split("\\").last().map(|s| s.to_string()))
} }

pub fn get_exe_by_hwnd (hwnd:Hwnd) -> Option<String> {
    get_exe_by_pid ( get_pid_by_hwnd (hwnd))
}


pub fn check_cur_proc_elevated () -> Option<bool> {
    check_proc_elevated ( unsafe { GetCurrentProcess() } )
}
pub fn check_hwnd_elevated (hwnd: Hwnd) -> Option<bool> { unsafe {
    let mut pid : u32 = 0;
    let _ = GetWindowThreadProcessId (hwnd.into(), Some(&mut pid));
    let h_proc = OpenProcess (PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
    h_proc .ok() .and_then (check_proc_elevated)
} }
pub fn check_proc_elevated (h_proc:HANDLE) -> Option<bool> { unsafe {
    let mut h_token = HANDLE::default();
    if OpenProcessToken (h_proc, TOKEN_QUERY, &mut h_token) .is_err() {
        return None;
    };
    let mut token_info : TOKEN_ELEVATION = TOKEN_ELEVATION::default();
    let mut token_info_len = size_of::<TOKEN_ELEVATION>() as u32;
    GetTokenInformation (
        h_token, TokenElevation, Some(&mut token_info as *mut _ as *mut _),
        token_info_len, &mut token_info_len
    ) .ok()?;
    Some (token_info.TokenIsElevated != 0)
} }






// we'll use a static rwlocked vec to store enum-windows from callbacks, and a mutex to ensure only one enum-windows call is active
#[allow(non_upper_case_globals)]
static enum_hwnds_lock : LazyLock <Arc <Mutex <()>>> = LazyLock::new (|| Arc::new ( Mutex::new(())));
#[allow(non_upper_case_globals)]
static enum_hwnds : LazyLock <Arc <RwLock <Vec <Hwnd>>>> = LazyLock::new (|| Arc::new ( RwLock::new (vec!()) ) );



type WinEnumCb = unsafe extern "system" fn (HWND, LPARAM) -> BOOL;

fn win_get_hwnds_w_filt (filt_fn: WinEnumCb) -> Vec<Hwnd> { unsafe {
    let lock = enum_hwnds_lock.lock().unwrap();
    *enum_hwnds.write().unwrap() = Vec::with_capacity(128);   // setting up some excess capacity to reduce reallocations
    let _ = EnumWindows ( Some(filt_fn), LPARAM::default() );
    let hwnds = enum_hwnds.write().unwrap().drain(..).collect();
    drop(lock);
    hwnds
} }

pub fn win_get_hwnds_ordered (hwnds:&HashSet<Hwnd>) -> Vec<(usize,Hwnd)> {
    win_get_no_filt_hwnds() .into_iter() .enumerate() .filter (|(_i,hwnd)| hwnds.contains(hwnd)) .collect()
}

fn win_get_no_filt_hwnds () -> Vec<Hwnd> {
    win_get_hwnds_w_filt (win_enum_cb_no_filt)
}
unsafe extern "system" fn win_enum_cb_no_filt (hwnd:HWND, _:LPARAM) -> BOOL {
    enum_hwnds.write().unwrap() .push (hwnd.into());
    BOOL (true as _)
}






/// Checks if a window has a direct child with the class name "MDICLIENT".
pub fn is_mdi_window (hwnd: Hwnd) -> bool { unsafe {
    // we'll set the LPARAM to point to a bool we expect the callback to fill if it finds MDICLIENT child hwnd
    let mut found_mdi = false;
    let found_mdi_ptr = &mut found_mdi as *mut bool;

    let _ = EnumChildWindows (Some(hwnd.into()), Some (enum_child_proc_check_mdi), LPARAM (found_mdi_ptr as _));
    // ^^ EnumChildWindows calls the callback for each child hwnd it finds

    if found_mdi { tracing::debug! ("Identified {:?} as MDI app window", hwnd); }
    found_mdi
} }

/// Callback for EnumChildWindows to check for "MDICLIENT" class.
unsafe extern "system" fn enum_child_proc_check_mdi (hwnd: HWND, lparam: LPARAM) -> BOOL {
    // if we found the mdi-child window, we set the LPARAM pointed boolean true (and stop enum by returning false)
    let found_mdi_ptr = lparam.0 as *mut bool;
    if found_mdi_ptr.is_null() {
        return false.into();
    }
    let mut class_name_buf: [u16; 64] = [0; 64];
    // ^^ just needs to be enough for "MDICLIENT" + null

    let len = GetClassNameW (hwnd, &mut class_name_buf);
    if len > 0 {
        let class_name = String::from_utf16_lossy (&class_name_buf [..len as _]);
        if class_name.eq_ignore_ascii_case ("MDIClient") {
            *found_mdi_ptr = true;
            return false.into();
        }
    }
    true.into()
}



