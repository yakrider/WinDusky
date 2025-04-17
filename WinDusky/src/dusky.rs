//use no_deadlocks::RwLock;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, FALSE, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_NOREPEAT, MOD_WIN, VK_I, VK_OEM_COMMA, VK_OEM_PERIOD};
use windows::Win32::UI::Magnification::{MagInitialize, MagSetColorEffect, MagSetWindowSource, MagUninitialize, MAGCOLOREFFECT, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowLongW, KillTimer, PostMessageW, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, HCURSOR, HICON, HWND_TOPMOST, MSG, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WM_CLOSE, WM_HOTKEY, WM_TIMER, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};



use crate::effects::*;
use crate::tray;

const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host Window";

const TIMER_TICK_MS : u32 = 16;

const HOTKEY_ID__TOGGLE      : usize = 1;
const HOTKEY_ID__NEXT_EFFECT : usize = 2;
const HOTKEY_ID__PREV_EFFECT : usize = 3;



#[derive (Default, Debug)]
struct Overlay {
    host   : Hwnd,
    mag    : Hwnd,
    target : Hwnd,
    effect : ColorEffect,
    marked : Flag,
    is_top : Flag,
}



//#[derive (Debug)]
pub struct WinDusky {
    inited    : Flag,
    overlays  : RwLock <HashMap <Hwnd, Overlay>>,
    cur_timer : AtomicUsize,
    ov_top    : HwndAtomic,
}




impl Overlay {

    fn new (target:HWND) -> Result <Overlay, String> { unsafe {

        let h_inst : Option<HINSTANCE> = GetModuleHandleW(None) .ok() .map(|h| h.into());

        // Create the host for the magniier control
        let Ok(host) = CreateWindowExW (
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw (wide_string(HOST_WINDOW_CLASS_NAME).as_ptr()),
            PCWSTR::from_raw (wide_string(HOST_WINDOW_TITLE).as_ptr()),
            WS_POPUP, 0, 0, 0, 0, None, None, h_inst, None
        ) else {
            return Err(format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
        };

        // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
        let Ok(mag) = CreateWindowExW(
            WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, Some(host), None, h_inst, None,
        ) else {
            return Err(format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
        };

        // we have enough to create the new overlay now
        let overlay = Overlay {
            host   : host.into(),
            mag    : mag.into(),
            target : target.into(),
            effect : ColorEffect::default(),
            marked : Flag::new(false),
            is_top : Flag::new(false),
        };

        // we'll apply the default smart inversion color-effect .. can ofc be cycled through via hotkeys later
        apply_color_effect (mag, overlay.effect.get());

        // we'll mark the overlay which will make our main loop timer-handler sync dimensions and position with the target
        overlay.marked.set();

        Ok(overlay)
    } }


    pub fn update (&self, wd: &WinDusky) { unsafe {

        //println!("updating overlay {:?}", self);

        // we'll size both the host and mag to fit the target hwnd when hotkey was invoked

        let mut rect = RECT::default();
        let (host, mag, target) = (self.host.into(), self.mag.into(), self.target.into());

        //let _ = GetWindowRect (fgnd, &mut rect) .is_err();
        // ^^ getting window-rect incluedes (often transparent) padding, which we dont want to invert, so we'll use window frame instead
        if DwmGetWindowAttribute (target, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as _, size_of::<RECT>() as u32) .is_err() {
            eprintln!( "DwmGetWindowAttribute (frame) on target failed with error: {:?}", GetLastError());
        }
        let (x, y, w, h) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);

        if MagSetWindowSource (mag, rect) .as_bool() == false {
            eprintln!( "MagSetWindowSource on mag-hwnd failed with error: {:?}", GetLastError());
        }
        if SetWindowPos (mag, None, 0, 0, w, h, Default::default()) .is_err() {
            eprintln!( "SetWindowPos (w,h) on mag-hwnd failed with error: {:?}", GetLastError());
        }

        // for overlay host z-positioning .. we want the overlay to usually be just above the target hwnd, but not topmost
        // (the hope is to keep maintaining that such that other windows can come in front normally as well)
        // however .. while its fgnd, we'll make it top to avoid flashing etc (while the host and target switch turns being in front)
        // (and so then to keep these from lingering on top, we've added also sanitation to event listener itself)
        let fgnd = GetForegroundWindow().into();
        if self.target == fgnd {
            // now if some other overlay was previously on-top, we'll want to un-top it first
            let ov_top = wd.ov_top.load();
            if ov_top.is_valid() && ov_top != self.target {
                let overlays = wd.overlays.read().unwrap();
                if let Some(overlay) = overlays .get (&ov_top) {
                    overlay.resync_ov_z_order()
                }
            }
            // Now we'd like the host to sit not just in front of target, but just 'topmost' it while target is fgnd
            let _ = SetWindowPos (host, None,               x, y, w, h, SWP_SHOWWINDOW);
            let _ = SetWindowPos (host, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW);
            //dbg!(win_check_if_topmost(host.into()));
            // ^^ to use TOPMOST, we must let it activate, otherwise windows set-fgnd rules will prevent it being put top
            // .. (so for reliability, we'll do regular show-window first, then follow up with topmost which might get ignored)
            self.is_top.set();
            wd.ov_top .store(target);
        } else {
            // and if we're not fgnd, we want to instead go sit just in front of target
            let _ = SetWindowPos (host, Some(target), x, y, w, h, SWP_NOACTIVATE);
            self.resync_ov_z_order();
        }
        self.marked.clear();

    } }

    pub fn resync_ov_z_order (&self) { unsafe {
        let (host, target) = (self.host.into(), self.target.into());
        self.is_top.clear();
        let _ = SetWindowPos (host, Some(target), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOMOVE);
        let _ = ShowWindow (host, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos (target, Some(host), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOMOVE);
    } }

}


impl Drop for Overlay {
    fn drop (&mut self) { unsafe {
        let _ = DestroyWindow (self.host.into());
    } }
}





impl WinDusky {

    pub fn instance() -> &'static WinDusky {
        static INSTANCE : OnceLock <WinDusky> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            WinDusky {
                inited    : Flag::new(false),
                overlays  : RwLock::new(HashMap::default()),
                cur_timer : AtomicUsize::default(),
                ov_top    : HwndAtomic::default(),
            }
        )
        // ^^ NOTE that init is not called here, and the user should to it at their convenience !!
    }


    pub fn start_monitor (&self) -> Result<(), String> { unsafe {

        if self.inited.is_set() {
            return Ok(())
        };
        self.inited.set();

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

        // register the toggle hotkey (Alt + Win + I),  and the effect-cycler hotkey (Alt + Win + '>'/'<')
        let _ = RegisterHotKey (None, HOTKEY_ID__TOGGLE as _,       MOD_ALT | MOD_WIN | MOD_NOREPEAT,  VK_I.0 as u32);
        let _ = RegisterHotKey (None, HOTKEY_ID__NEXT_EFFECT as _,  MOD_ALT | MOD_WIN | MOD_NOREPEAT,  VK_OEM_PERIOD.0 as u32);
        let _ = RegisterHotKey (None, HOTKEY_ID__PREV_EFFECT as _,  MOD_ALT | MOD_WIN | MOD_NOREPEAT,  VK_OEM_COMMA.0 as u32);


        // lets also setup a win-event hook to monitor fgnd change so we can maintain the overlay z-ordering
        /*
            We want to cover at least :
                0x03   : EVENT_SYSTEM_FOREGROUND

                0x08   : EVENT_SYSTEM_CAPTURESTART
                0x09   : EVENT_SYSTEM_CAPTUREEND
                // ^^ w/o these, the target can end up z-ahead of overlay upon titlebar click etc
                0x0A   : EVENT_SYSTEM_MOVESIZESTART
                0x0B   : EVENT_SYSTEM_MOVESIZEEND

                0x16   : EVENT_SYSTEM_MINIMIZESTART
                0x17   : EVENT_SYSTEM_MINIMIZEEND

                0x8000 : EVENT_OBJECT_CREATE
                0x8001 : EVENT_OBJECT_DESTROY
                0x8002 : EVENT_OBJECT_SHOW
                0x8003 : EVENT_OBJECT_HIDE
                0x800B : EVENT_OBJECT_LOCATIONCHANGE

                0x8017 : EVENT_OBJECT_CLOAKED
                0x8018 : EVENT_OBJECT_UNCLOAKED
         */
        let _ = SetWinEventHook ( 0x0003, 0x0003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x0008, 0x000B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x0016, 0x0017, None, Some(win_event_proc), 0, 0, 0 );

        let _ = SetWinEventHook ( 0x8000, 0x8003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x800B, 0x800B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook ( 0x8017, 0x8018, None, Some(win_event_proc), 0, 0, 0 );


        // finally we just babysit the message loop
        let mut msg: MSG = std::mem::zeroed();

        loop {
            if GetMessageW(&mut msg, None, 0, 0) == false {
                let _ = MagUninitialize();
                return Err(format!("GetMessageW failed with error: {:?}", GetLastError()));
            }
            else if msg.message == WM_TIMER {
                let overlays = self.overlays.read().unwrap();
                for overlay in overlays.values() {
                    if overlay.marked.is_set() {
                        overlay.update(self);
                    }
                    let _ = InvalidateRect (Some(overlay.mag.into()), None, false);
                }
            }
            else if msg.message == WM_HOTKEY {

                let target = GetForegroundWindow().into();
                let mut overlays = self.overlays.write().unwrap();

                if let Some(overlay) = overlays.get(&target) {
                    match msg.wParam.0 {
                        HOTKEY_ID__TOGGLE => {
                            if let Some(overlay) = overlays.remove (&target) {
                                // ^^ the returned value is dropped and so its hwnds will get cleaned up
                                if overlay.target == self.ov_top.load() { self.ov_top.clear(); }
                            }
                            if overlays.is_empty() { self.disable_timer() }
                            tray::update_tray__overlay_count (overlays.len());
                        },
                        HOTKEY_ID__NEXT_EFFECT => {
                            apply_color_effect (overlay.mag, overlay.effect.cycle_next());
                        },
                        HOTKEY_ID__PREV_EFFECT => {
                            apply_color_effect (overlay.mag, overlay.effect.cycle_prev());
                        },
                        _ => { }
                    }
                }
                else if msg.wParam.0 == HOTKEY_ID__TOGGLE {
                    if let Ok(overlay) = Overlay::new (target.into()) {
                        if overlays.is_empty() { self.ensure_timer_running() }
                        overlays.insert (target, overlay);
                        tray::update_tray__overlay_count (overlays.len());
                    }
                }
                //dbg!(overlays);
            }
            else {
                //let _ = TranslateMessage(&msg);
                // ^^ not needed as we dont do any gui w text etc
                DispatchMessageW(&msg);
            }
        }

    } }


    pub fn ensure_timer_running (&self) { unsafe {
        let timer_id = SetTimer (None, 0, TIMER_TICK_MS, None);
        self.cur_timer .store (timer_id, Ordering::Release);
    } }

    pub fn disable_timer (&self) { unsafe {
        let _ = KillTimer (None, self.cur_timer .load(Ordering::Acquire));
    } }


    pub(crate) fn clear_overlays (&self) { unsafe {
        let mut overlays = self.overlays.write().unwrap();
        let hwnds : Vec<_> = overlays.keys().copied().collect();
        for hwnd in hwnds {
            if let Some(overlay) = overlays.remove(&hwnd) {
                let _ = PostMessageW (Some(overlay.host.into()), WM_CLOSE, Default::default(), Default::default());
                let _ = InvalidateRect (Some(hwnd.into()), None, true);
            }
        }
        self.ov_top.clear();
        self.disable_timer();
        tray::update_tray__overlay_count(0);
    } }


    pub(crate) fn overlays_count (&self) -> usize {
        let overlays = self.overlays.read().unwrap();
        overlays.len()
    }

}





fn apply_color_effect (mag: impl Into<HWND>, effect: MAGCOLOREFFECT) { unsafe {
    if MagSetColorEffect (mag.into(), &effect as *const _ as _) == false {
        eprintln! ("Setting Color Effect failed with error: {:?}", GetLastError());
        let _ = MagUninitialize();
    }
} }




// Helper function to convert Rust string slices to null-terminated UTF-16 Vec<u16>
fn wide_string(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}


#[allow (dead_code)] pub fn win_check_if_topmost (hwnd: Hwnd) -> bool { unsafe {
    GetWindowLongW (hwnd.into(), GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == WS_EX_TOPMOST.0
} }




// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    host: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    // we'll ust leave default message handling
    DefWindowProcW (host, msg, wparam, lparam)
}




// Callback handling for our win-event hook
unsafe extern "system" fn win_event_proc (
    _hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32,
    _id_child: i32, _event_thread: u32, _event_time: u32,
) {
    if id_object != OBJID_WINDOW.0 { return; }
    // ^^ we only care about window level events

    let wd = WinDusky::instance();
    let mut overlays = wd.overlays.write().unwrap();

    //// debug printout of all events .. useful during dev
    //if !hwnd.is_invalid() && wd.overlays.lock().unwrap().contains_key(&hwnd.into()) {
    //    println!("{:#06x}",event);
    //} // ^^ debug printouts (enable all events first)

    if let Some(overlay) = overlays .get (&hwnd.into()) {
        //println!("got event {:#06x} for hwnd {:?}, id-object {:#06x}, id-child {:#06x}", event, hwnd, id_object, _id_child);
        if event == EVENT_OBJECT_DESTROY ||
            event == EVENT_OBJECT_HIDE ||
            event == EVENT_OBJECT_CLOAKED
        {
            if let Some(overlay) = overlays .remove (&hwnd.into()) {
                if overlay.target == wd.ov_top.load() { wd.ov_top.clear(); }
            }
            if overlays.is_empty() { wd.disable_timer() }
            tray::update_tray__overlay_count (overlays.len());
        }
        else {
            overlay.marked.set();
        }
    }
    else if event == EVENT_SYSTEM_FOREGROUND {
        // i.e non overlaid window came to fgnd
        if let Some(overlay) = overlays .get (&wd.ov_top.load()) {
            wd.ov_top.clear();
            overlay.resync_ov_z_order();
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

