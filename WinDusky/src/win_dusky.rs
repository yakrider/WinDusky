use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_NOREPEAT, MOD_WIN, VK_I};
use windows::Win32::UI::Magnification::{MagInitialize, MagSetColorEffect, MagSetWindowSource, MagUninitialize, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, IsWindowVisible, KillTimer, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HICON, HWND_BOTTOM, HWND_TOP, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WM_HOTKEY, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};



use crate::color_matrices::*;


const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host Window";

const TIMER_ID : usize = 0xdeadbeef;
const TIMER_TICK_MS : u32 = 16;

const HOTKEY_ID__TOGGLE : i32 = 1;



pub fn run_it() -> Result<(), String> { unsafe {

    let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    // Initialize Magnification API
    if MagInitialize() == FALSE {
        return Err(format!("MagInitialize failed with error: {:?}", GetLastError()));
    }


    // Register Host Window Class
    let Ok(instance) = GetModuleHandleW(None) else {
        return Err(format!("GetModuleHandleW failed with error: {:?}", GetLastError()));
    };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(host_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance.into(),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: PCWSTR::from_raw (wide_string(HOST_WINDOW_CLASS_NAME).as_ptr()),
        hIconSm: HICON::default(),
    };
    if RegisterClassExW(&wc) == 0 {
        if GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!("RegisterClassExW failed with error: {:?}", GetLastError()));
        }
    }


    let h_inst : Option<HINSTANCE> = GetModuleHandleW(None) .ok() .map(|h| h.into());

    // Create the host for the magniier control
    let Ok(hwnd_host) = CreateWindowExW (
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        PCWSTR::from_raw (wide_string(HOST_WINDOW_CLASS_NAME).as_ptr()),
        PCWSTR::from_raw (wide_string(HOST_WINDOW_TITLE).as_ptr()),
        WS_POPUP, 0, 0, 0, 0, None, None, h_inst, None
    ) else {
        let _ = MagUninitialize();
        return Err(format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
    };

    // we'll setup a user data in the host window where we can later store/retrieve hwnd_mag
    let mut mag_data = MagWindowData { hwnd_mag : HWND::default() };
    SetWindowLongPtrW (hwnd_host, GWLP_USERDATA, &mut mag_data as *mut _ as isize);


    // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
    let Ok(hwnd_mag) = CreateWindowExW(
        WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
        0, 0, 0, 0, Some(hwnd_host), None, h_inst, None,
    ) else {
        let _ = MagUninitialize();
        return Err(format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
    };

    // we'll want to store this so hwnd_host can access it and invalidate hwnd_mag in its loop
    mag_data.hwnd_mag = hwnd_mag;


    // apply color effect
    if MagSetColorEffect (hwnd_mag, &COLOR_EFF__SMART_INVERSION_ALT2 as *const _ as _) == false {
        let _ = MagUninitialize();
        return Err(format!("MagSetColorEffect failed with error: {:?}", GetLastError()));
    }


    // lets setup a timer so it keeps getting repainted
    SetTimer (Some(hwnd_host), TIMER_ID, TIMER_TICK_MS, None);


    // Register the hotkey (Alt + Win + I)
    if RegisterHotKey (Some(hwnd_host), HOTKEY_ID__TOGGLE, MOD_ALT | MOD_WIN | MOD_NOREPEAT, VK_I.0 as u32) .is_err() {
        eprintln!("Warning: Hotkey Registration failed with error: {:?}", GetLastError());
    }
    // Note that we'll only do positioning/sizing/sourcing of the overlay when hotkey enables the overlay


    // finally we just babysit the host hwnd
    let mut msg: MSG = std::mem::zeroed();
    loop {
        let ret = GetMessageW(&mut msg, None, 0, 0);
         if ret == false {
             let _ = MagUninitialize();
             let _ = KillTimer(None, TIMER_ID);
            return Err(format!("GetMessageW failed with error: {:?}", GetLastError()));
        } else {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

} }


fn toggle_overlay (host:HWND, mag:HWND) { unsafe {

    if IsWindowVisible(host).as_bool() {
        let _ = ShowWindow (host, SW_HIDE);
        return
    }

    // we'll size both the host and mag to fit the fgnd hwnd when hotkey was invoked

    let fgnd = GetForegroundWindow();
    let mut rect = RECT::default();

    //let _ = GetWindowRect (fgnd, &mut rect) .is_err();
    let _ = DwmGetWindowAttribute (fgnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as _, size_of::<RECT>() as u32);
    // ^^ getting window-rect incluedes (often transparent) padding, which we dont want to invert, so we'll use window frame instead

    let _ = MagSetWindowSource (mag, rect);

    let (x, y, w, h) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
    let _ = SetWindowPos (mag, None, 0, 0, w, h, Default::default());

    // for overlay host z-positioning .. we want the overlay to be just above the target hwnd, but not topmost
    // (the hope is to keep maintaining that such that other windows can come in front normally as well)

    let _ = SetWindowPos (host, Some(fgnd), x, y, w, h, SWP_NOACTIVATE);
    let _ = ShowWindow (host, SW_SHOW);
    let _ = SetWindowPos (fgnd, Some(host), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOMOVE);

    // todo : prob want to enable/disable timer etc here too

} }


// Structure to hold magnifier window handle, associated with host window
struct MagWindowData {
    hwnd_mag: HWND,
}


// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == TIMER_ID => {
            let mag_data = GetWindowLongPtrW (hwnd, GWLP_USERDATA) as *mut MagWindowData;
            let _ = InvalidateRect(Some((&*mag_data).hwnd_mag), None, false);
            LRESULT(0)
        },
        WM_HOTKEY if wparam.0 == HOTKEY_ID__TOGGLE as _ => {
            let mag_data = GetWindowLongPtrW (hwnd, GWLP_USERDATA) as *mut MagWindowData;
            toggle_overlay (hwnd, (*mag_data).hwnd_mag);
            LRESULT(0)
        },
        _ => DefWindowProcW(hwnd, msg, wparam, lparam), // Default handling for other messages
    }
}


// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}
