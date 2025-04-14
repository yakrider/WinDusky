use once_cell::sync::Lazy;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_NOREPEAT, MOD_WIN, VK_I};
use windows::Win32::UI::Magnification::{MagInitialize, MagSetColorEffect, MagSetWindowSource, MagUninitialize, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, IsWindowVisible, KillTimer, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, HCURSOR, HICON, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WM_HOTKEY, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};



use crate::color_matrices::*;


const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host Window";

const TIMER_ID : usize = 0xdeadbeef;
const TIMER_TICK_MS : u32 = 16;

const HOTKEY_ID__TOGGLE : i32 = 1;



// Structure to hold magnifier window handle, associated with host window
#[derive (Default)]
struct OverlayDat {
    host   : HwndAtomic,
    mag    : HwndAtomic,
    target : HwndAtomic,
    marked : Flag,
}

#[allow (non_upper_case_globals)]
static overlay : Lazy<OverlayDat> = Lazy::new (OverlayDat::default);




pub fn start_overlay() -> Result<(), String> { unsafe {

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
    overlay.host.store(hwnd_host);


    // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
    let Ok(hwnd_mag) = CreateWindowExW(
        WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
        0, 0, 0, 0, Some(hwnd_host), None, h_inst, None,
    ) else {
        let _ = MagUninitialize();
        return Err(format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
    };
    overlay.mag.store(hwnd_mag);


    // apply color effect
    if MagSetColorEffect (hwnd_mag, &COLOR_EFF__SMART_INVERSION_ALT2 as *const _ as _) == false {
        let _ = MagUninitialize();
        return Err(format!("MagSetColorEffect failed with error: {:?}", GetLastError()));
    }

    // lets setup a timer so it keeps getting repainted
    SetTimer (Some(hwnd_host), TIMER_ID, TIMER_TICK_MS, None);


    // register the hotkey (Alt + Win + I)
    if RegisterHotKey (Some(hwnd_host), HOTKEY_ID__TOGGLE, MOD_ALT | MOD_WIN | MOD_NOREPEAT, VK_I.0 as u32) .is_err() {
        eprintln!("Warning: Hotkey Registration failed with error: {:?}", GetLastError());
    }
    // Note that we'll only do positioning/sizing/sourcing of the overlay when hotkey enables the overlay



    // lets also setup a win-event hook to monitor fgnd change so we can maintain the overlay z-ordering
    /*
        We want to cover at least :
            0x03   : EVENT_SYSTEM_FOREGROUND

            0x08   : EVENT_SYSTEM_CAPTURESTART
            0x09   : EVENT_SYSTEM_CAPTUREEND
            0x0A   : EVENT_SYSTEM_MOVESIZESTART
            0x0B   : EVENT_SYSTEM_MOVESIZEEND
            // ^^ w/o these, the target can end up z-ahead of overlay upon titlebar click

            0x16   : EVENT_SYSTEM_MINIMIZESTART
            0x17   : EVENT_SYSTEM_MINIMIZEEND

            0x8001 : EVENT_OBJECT_DESTROY
            0x8002 : EVENT_OBJECT_SHOW
            0x8003 : EVENT_OBJECT_HIDE
            0x800B : EVENT_OBJECT_LOCATIONCHANGE

            0x8017 : EVENT_OBJECT_CLOAKED
            0x8018 : EVENT_OBJECT_UNCLOAKED
     */
    let _ = SetWinEventHook ( 0x0003, 0x0017, None, Some(win_event_proc), 0, 0, 0 );
    let _ = SetWinEventHook ( 0x8001, 0x8018, None, Some(win_event_proc), 0, 0, 0 );



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




fn reset_overlay (host: impl Into<HWND>, mag: impl Into<HWND>, target: impl Into<HWND>) { unsafe {

    // we'll size both the host and mag to fit the target hwnd when hotkey was invoked

    let (host, mag, target) = (host.into(), mag.into(), target.into());
    let mut rect = RECT::default();

    //let _ = GetWindowRect (fgnd, &mut rect) .is_err();
    let _ = DwmGetWindowAttribute (target, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as _, size_of::<RECT>() as u32);
    // ^^ getting window-rect incluedes (often transparent) padding, which we dont want to invert, so we'll use window frame instead

    let _ = MagSetWindowSource (mag, rect);

    let (x, y, w, h) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);
    let _ = SetWindowPos (mag, None, 0, 0, w, h, Default::default());

    // for overlay host z-positioning .. we want the overlay to be just above the target hwnd, but not topmost
    // (the hope is to keep maintaining that such that other windows can come in front normally as well)

    let _ = SetWindowPos (host, Some(target), x, y, w, h, SWP_NOACTIVATE);
    let _ = ShowWindow (host, SW_SHOW);
    let _ = SetWindowPos (target, Some(host), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOMOVE);

    overlay.marked.clear();

    // todo : prob want to enable/disable timer etc here too

} }




// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}



// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == TIMER_ID => {
            if overlay.marked.is_set() {
                reset_overlay (overlay.host.load(), overlay.mag.load(), overlay.target.load());
            }
            let _ = InvalidateRect(Some(overlay.mag.load().into()), None, false);
            LRESULT(0)
        },
        WM_HOTKEY if wparam.0 == HOTKEY_ID__TOGGLE as _ => {
            if IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindow (hwnd, SW_HIDE);
            } else {
                overlay.target.store (GetForegroundWindow());
                overlay.marked.set();
            }
            LRESULT(0)
        },
        _ => DefWindowProcW(hwnd, msg, wparam, lparam), // Default handling for other messages
    }
}




// Callback handling for our win-event hook
unsafe extern "system" fn win_event_proc (
    _hook: HWINEVENTHOOK, event: u32, hwnd: HWND, _id_object: i32,
    _id_child: i32, _event_thread: u32, _event_time: u32,
) {
    //if !hwnd.is_invalid() && hwnd == overlay.target.load().into() {
    //    println!("{:#06x}",event);
    //} // ^^ debug printouts (enable all events first)

    if hwnd == overlay.target.load().into() {
        let host = overlay.host.load();
        if IsWindowVisible (host.into()) .as_bool() {
            if event == EVENT_OBJECT_DESTROY || event == EVENT_OBJECT_HIDE || event == EVENT_OBJECT_CLOAKED {
                let _ = ShowWindow (host.into(), SW_HIDE);
            } else {
                overlay.marked.set();
            }
        }
    }
}





// we'll define our own new-type of Hwnd mostly coz HWND doesnt implement Debug, Hash etc
# [ derive (Debug, Default, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash) ]
pub struct Hwnd (isize);

impl Hwnd {
    pub fn is_valid (&self) -> bool { self.0 != 0 }
}

impl From <HWND> for Hwnd {
    fn from (hwnd:HWND) -> Self { Hwnd(hwnd.0 as _) }
}
impl From <Hwnd> for HWND {
    fn from (hwnd:Hwnd) -> Self { HWND(hwnd.0 as _) }
}
impl From <Hwnd> for isize {
    fn from (hwnd:Hwnd) -> Self { hwnd.0 }
}
impl From <isize> for Hwnd {
    fn from (hwnd: isize) -> Self { Hwnd(hwnd) }
}




// and the atomic version of Hwnd for storage
# [ derive (Debug, Default) ]
pub struct HwndAtomic (AtomicIsize);

impl HwndAtomic {
    pub fn load (&self) -> Hwnd {
        self.0.load (Ordering::Acquire) .into()
    }
    pub fn store (&self, hwnd: impl Into<Hwnd>) {
        self.0 .store (hwnd.into().0, Ordering::Release)
    }
    pub fn clear (&self) {
        self.store (Hwnd(0))
    }
    pub fn contains (&self, hwnd: impl Into<Hwnd>) -> bool {
        self.load() == hwnd.into()
    }
    pub fn is_valid (&self) -> bool {
        self.load() != Hwnd(0)
    }
}
impl From <HwndAtomic> for Hwnd {
    fn from (h_at: HwndAtomic) -> Hwnd { h_at.load() }
}
impl From <HwndAtomic> for HWND {
    fn from (h_at: HwndAtomic) -> HWND { h_at.load().into() }
}




/// representation for all our atomic flags for states mod-states, modifier-keys, mouse-btn-state etc <br>
/// (Note that this uses Acquire/Release memory ordering semantics, and shouldnt be used as lock/mutex etc)
# [ derive (Debug, Default) ]
pub struct Flag (AtomicBool);
// ^^ simple sugar that helps reduce clutter in code

impl Flag {
    /* Note regarding Atomic Memory Ordering usage here ..
       - The Flag struct is intended for use as simple flags, not as synchronization primitives (i.e locks)
       - On x86, there is strong memory model and Acq/Rel is free .. so no benefit to using Relaxed
       - SeqCst however requires a memory fence that could be potentially be costly (flush writes before atomic op etc)
       - For the very rare cases that would require total global ordering with SeqCst, we should just use lib facilities instead!!
    */
    pub fn new (state:bool) -> Flag { Flag (AtomicBool::new(state)) }

    /// toggling returns prior state .. better to use this than to check and set
    pub fn toggle (&self) -> bool { self.0 .fetch_xor (true, Ordering::AcqRel) }

    /// swap stores new state and returns prior state .. better to use this than to update and check/load separately
    pub fn swap   (&self, state:bool) -> bool { self.0 .swap (state, Ordering::AcqRel) }

    pub fn set   (&self) { self.0 .store (true,  Ordering::Release) }
    pub fn clear (&self) { self.0 .store (false, Ordering::Release) }

    pub fn store  (&self, state:bool) { self.0.store (state, Ordering::Release) }

    pub fn is_set   (&self) -> bool {  self.0 .load (Ordering::Acquire) }
    pub fn is_clear (&self) -> bool { !self.0 .load (Ordering::Acquire) }
}
impl From<Flag> for bool {
    fn from (flag: Flag) -> bool { flag.is_set() }
}

