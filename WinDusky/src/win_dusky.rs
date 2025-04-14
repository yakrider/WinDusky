use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Magnification::{MagInitialize, MagSetColorEffect, MagSetWindowSource, MagUninitialize, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW, KillTimer, RegisterClassExW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HCURSOR, HICON, HWND_TOPMOST, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};



const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host Window";

const TIMER_ID : usize = 0xdeadbeef;
const TIMER_TICK_MS : u32 = 16;

use crate::color_matrices::*;

pub unsafe fn run_it() -> Result<(), String> {
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
        WS_POPUP, 0, 0, 1600, 1200, None, None, h_inst, None
    ) else {
        let _ = MagUninitialize();
        return Err(format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
    };

    // we'll setup a user data in the host window where we can later store/retrieve hwnd_mag
    let mut mag_data = MagWindowData { hwnd_mag : HWND::default() };
    SetWindowLongPtrW (hwnd_host, GWLP_USERDATA, &mut mag_data as *mut _ as isize);

    // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
    let Ok(hwnd_mag) = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        WC_MAGNIFIERW,
        PCWSTR::default(), // No title
        WS_CHILD | WS_VISIBLE,
        0, 0, 1600, 1200,
        Some(hwnd_host), None, h_inst, None,
    ) else {
        let _ = MagUninitialize();
        return Err(format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
    };

    // we'll want to store this so hwnd_host can access it and invalidate hwnd_mag in its loop
    mag_data.hwnd_mag = hwnd_mag; // Store magnifier handle

    // Set source rect for transformation
    let source_rect = RECT { left: 0, top: 0, right: 1600, bottom: 1200 };
    if MagSetWindowSource (hwnd_mag, source_rect) == false {
        eprintln!("Warning: MagSetFullscreenSource failed with error: {:?}", GetLastError());
    }

    // apply color effect
    if MagSetColorEffect (hwnd_mag, &COLOR_EFF_GRAYSCALE as *const _ as _) == false {
        let _ = MagUninitialize();
        return Err(format!("MagSetColorEffect failed with error: {:?}", GetLastError()));
    }

    // lets setup a timer so it keeps getting repainted
    SetTimer (Some(hwnd_host), TIMER_ID, TIMER_TICK_MS, None);

    // and now we're ready to make this visible
    let _ = SetWindowPos ( hwnd_mag, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE, );
    let _ = ShowWindow (hwnd_host, SW_SHOWNOACTIVATE);

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
}



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
        _ => DefWindowProcW(hwnd, msg, wparam, lparam), // Default handling for other messages
    }
}


// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}
