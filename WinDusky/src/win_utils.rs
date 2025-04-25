#![allow (dead_code, non_snake_case)]

use std::collections::HashSet;
use std::ffi::OsStr;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use crate::types::Hwnd;
use std::os::windows::prelude::OsStrExt;
use windows::core::{BOOL, PSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, GetDC, ReleaseDC, BitBlt, SRCCOPY};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::Storage::Xps::{PrintWindow, PW_CLIENTONLY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameA, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetClientRect, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOPMOST};





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






/// Capture window pixels using PrintWindow
// Using PrintWindow has the advantage that it should get the window capture even when partially obscured etc
// Otoh, for hwnds with MDI child (e.g. Device Manager aka mmc.exe), the MDI child content is not captured
// (and in theory its prob slower than using the BitBlt alternative below)
fn capture_hwnd__PrintWindow (hwnd: Hwnd) -> Option<(Vec<u8>, i32, i32)> { unsafe {

    let hwnd_win: HWND = hwnd.into();
    let mut rect = RECT::default();
    if GetClientRect (hwnd_win, &mut rect).is_err() {
        return None;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    if width <= 0 || height <= 0 {
        return None; // Invalid dimensions
    }

    // Create a device context (DC) compatible with the window
    let hdc_screen = GetDC(None);
    if hdc_screen.is_invalid() { return None; }
    let hdc_mem = CreateCompatibleDC (Some(hdc_screen));

    if hdc_mem.is_invalid() {
        let _ = ReleaseDC(None, hdc_screen);
        return None;
    }

    // Create a bitmap compatible with the window DC
    let h_bitmap = CreateCompatibleBitmap(hdc_screen, width, height); // Use screen DC for compatibility

    let _ = ReleaseDC(None, hdc_screen);

    if h_bitmap.is_invalid() {
        let _ = DeleteDC(hdc_mem);
        return None;
    }

    // Select the bitmap into the memory DC
    let h_old_bitmap = SelectObject(hdc_mem, h_bitmap.into());
    if h_old_bitmap.is_invalid() {
        let _ = DeleteObject(h_bitmap.into());
        let _ = DeleteDC(hdc_mem);
        return None;
    }

    // Use PrintWindow to draw the window into the memory DC
    // PW_RENDERFULLCONTENT might be needed for some UI frameworks, but start without it
    //let print_result = PrintWindow (hwnd_win, hdc_mem, PRINT_WINDOW_FLAGS::default());
    let print_result = PrintWindow (hwnd_win, hdc_mem, PW_CLIENTONLY);

    // Deselect the bitmap
    let _ = SelectObject(hdc_mem, h_old_bitmap);

    if !print_result.as_bool() {
        let _ = DeleteObject(h_bitmap.into());
        let _ = DeleteDC(hdc_mem);
        return None;
    }

    // Prepare to get bitmap data
    let bmih = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // Negative height indicates top-down DIB
        biPlanes: 1,
        biBitCount: 32, // Request 32-bit BGRA format
        biCompression: BI_RGB.0,
        biSizeImage: 0, // Can be 0 for BI_RGB
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut bmi = BITMAPINFO {
        bmiHeader: bmih,
        bmiColors: Default::default(), // No color table needed for BI_RGB
    };

    let buffer_size = (width * height * 4) as usize; // 4 bytes per pixel (BGRA)
    let mut buffer: Vec<u8> = vec![0; buffer_size];

    // Get the actual bitmap data
    let result = GetDIBits (
        hdc_mem, h_bitmap,
        0, height as u32,   // scan lines to start from, and number of scan lines
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Clean up GDI objects
    let _ = DeleteObject(h_bitmap.into());
    let _ = DeleteDC(hdc_mem);

    if result == 0 || result == windows::Win32::Foundation::ERROR_INVALID_PARAMETER.0 as i32 {
         None // Failed to get bits
    } else {
         Some((buffer, width, height)) // Return BGRA buffer, width, height
    }
} }






/// Capture window pixels using BitBlt
// This copies pixels for the rect from the eqv of screen buffer .. so isnt impacted by MDI etc, and should be faster
// However, if there are other windows obscuring the target, e.g. because of some TOPMOST widdget etc, those will be captured too
fn capture_hwnd__BitBlt (hwnd: Hwnd) -> Option<(Vec<u8>, i32, i32)> { unsafe {
    let hwnd: HWND = hwnd.into();
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return None;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    if width <= 0 || height <= 0 {
        return None; // Invalid dimensions
    }

    // Get the device context (DC) for the window
    let hdc_win = GetDC(Some(hwnd));
    if hdc_win.is_invalid() { return None }

    // Create a memory DC compatible with the window DC
    let hdc_mem = CreateCompatibleDC(Some(hdc_win));
    if hdc_mem.is_invalid() {
        let _ = ReleaseDC(Some(hwnd), hdc_win);
        return None;
    }

    // Create a bitmap compatible with the window DC
    let h_bitmap = CreateCompatibleBitmap(hdc_win, width, height);
    if h_bitmap.is_invalid() {
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(Some(hwnd), hdc_win);
        return None;
    }

    // Select the bitmap into the memory DC
    let h_old_bitmap = SelectObject(hdc_mem, h_bitmap.into());
    if h_old_bitmap.is_invalid() {
        let _ = DeleteObject(h_bitmap.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(Some(hwnd), hdc_win);
        return None;
    }

    // Use BitBlt to copy from window DC to memory DC
    let bitblt_result = BitBlt(hdc_mem, 0, 0, width, height, Some(hdc_win), 0, 0, SRCCOPY);

    // Deselect the bitmap
    let _ = SelectObject(hdc_mem, h_old_bitmap);

    // Release the window DC *before* checking BitBlt result
    let _ = ReleaseDC(Some(hwnd), hdc_win);

    if bitblt_result.is_err() {
        let _ = DeleteObject(h_bitmap.into());
        let _ = DeleteDC(hdc_mem);
        return None; // BitBlt failed
    }

    // Prepare to get bitmap data (same as PrintWindow version)
    let bmih = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // Negative height indicates top-down DIB
        biPlanes: 1,
        biBitCount: 32, // Request 32-bit BGRA format
        biCompression: BI_RGB.0,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut bmi = BITMAPINFO {
        bmiHeader: bmih,
        bmiColors: Default::default(),
    };

    let buffer_size = (width * height * 4) as usize;
    let mut buffer: Vec<u8> = vec![0; buffer_size];

    let result = GetDIBits (
        hdc_mem, h_bitmap,
        0, height as u32,
        Some(buffer.as_mut_ptr() as *mut _),
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Clean up GDI objects
    let _ = DeleteObject(h_bitmap.into());
    let _ = DeleteDC(hdc_mem);

    if result == 0 || result == windows::Win32::Foundation::ERROR_INVALID_PARAMETER.0 as i32 {
         None
    } else {
         Some((buffer, width, height))
    }
} }



/// Calculates the average luminance of an hwnd
pub fn calculate_avg_luminance (hwnd: Hwnd) -> Option<u8> {

    //let (buffer, width, height) = capture_hwnd__PrintWindow (hwnd)?;
    let (buffer, width, height) = capture_hwnd__BitBlt (hwnd)?;

    if width <= 0 || height <= 0 || buffer.len() != (width * height * 4) as usize {
        return None;
    }

    let num_pixels = (width * height) as usize;
    let mut total_luminance: f64 = 0.0;

    for pixel_index in 0..num_pixels {
        let base_idx = pixel_index * 4;
        let b = buffer[base_idx] as f64 / 255.0;
        let g = buffer[base_idx + 1] as f64 / 255.0;
        let r = buffer[base_idx + 2] as f64 / 255.0;
        //let a = buffer[base_idx + 3] as f64 / 255.0;
        // ^^ ignore alpha as it isnt even consistently specified for non-layered hwnds

        // Use the BT.709 formula to add up human-eye luminance of R/G/B colors
        let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        total_luminance += luminance;
    }

    Some ((total_luminance / num_pixels as f64 * u8::MAX as f64) as u8)
}
