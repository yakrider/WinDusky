use std::sync::OnceLock;
use crate::dusky::WinDusky;
use crate::effects::{ColorEffect, ColorEffectAtomic, ColorEffects, COLOR_EFF__IDENTITY};
use crate::occlusion::Rect;
use crate::types::{Flag, Hwnd};
use crate::win_utils::wide_string;
use tracing::{error, info};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{InvalidateRect, MapWindowPoints, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Magnification::{MagSetColorEffect, MagSetFullscreenColorEffect, MagSetWindowSource, MAGCOLOREFFECT, WC_MAGNIFIERW};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindow, RegisterClassExW, SetWindowPos, CS_HREDRAW, CS_VREDRAW, GW_HWNDPREV, HCURSOR, HICON, HWND_TOP, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOW_EX_STYLE, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE};




//  ~~~ Thread Affinity Reminder ~~~
// .. The way Mag-API works, calls for setting color effect etc must be made from SAME thread that called MagInitialize !!



const HOST_WINDOW_CLASS_NAME : &str = "WinDuskyOverlayWindowClass";
const HOST_WINDOW_TITLE      : &str = "WinDusky Overlay Host";


pub unsafe fn register_overlay_class () -> Result <(), String> {

    let Ok(instance) = GetModuleHandleW(None) else {
        return Err (format!("GetModuleHandleW failed with error: {:?}", GetLastError()));
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

    if RegisterClassExW (&wc) == 0 {
        if GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err (format!("RegisterClassExW failed with error: {:?}", GetLastError()));
        }
    }

    Ok(())
}




#[derive (Debug, Default)]
pub struct FullScreenOverlay {
    pub enabled : Flag,
    pub active  : Flag,
    pub effect  : ColorEffectAtomic,
}


#[derive (Debug, Default)]
pub struct Overlay {
    pub host   : Hwnd,
    pub mag    : Hwnd,
    pub target : Hwnd,

    pub effect : ColorEffectAtomic,

    pub is_top : Flag,
    pub marked : Flag,

    pub viz_bounds : Option<Rect>
}



impl FullScreenOverlay {

    pub(super) fn instance() -> &'static FullScreenOverlay {
        static INSTANCE : OnceLock <FullScreenOverlay> = OnceLock::new();
        INSTANCE .get_or_init ( ||
            FullScreenOverlay {
                enabled : Flag::default(),
                active  : Flag::default(),
                effect  : ColorEffectAtomic::new (ColorEffects::instance().get_default()),
            }
        )
    }

    /// toggles full screen effect enabled state and returns the updated state
    pub(super) fn toggle (&self) -> bool {
        let enabled = !self.enabled.toggle();
        self.active .store (enabled);
        info! ("Setting FULL-SCREEN_OVERLAY mode to : {} !!", if enabled {"ON"} else {"OFF"} );
        self.apply_color_effect ( if enabled { self.effect.get() } else { COLOR_EFF__IDENTITY } );
        enabled
    }
    pub(super) fn set_enabled (&self, enabled: bool) {
        let prior = self.enabled.swap(enabled);
        self.active .store (enabled);
        if prior != enabled {
            info! ("Setting FULL-SCREEN_OVERLAY mode to : {} !!", if enabled {"ON"} else {"OFF"} );
            self.apply_color_effect ( if enabled { self.effect.get() } else { COLOR_EFF__IDENTITY } );
        }
    }

    /// toggles the effect applied full screen (does not affect the enabled state itself!)
    pub(super) fn toggle_effect (&self) -> Option<ColorEffect> {
        if self.active.is_set() {
            self.unapply_effect();
            return None
        }
        Some (self.apply_effect_cycled (None))
    }
    pub(super) fn apply_effect_next (&self) -> Option<ColorEffect> {
        if self.active.is_clear() { return None }
        Some ( self.apply_effect_cycled (Some(true)))
    }
    pub(super) fn apply_effect_prev (&self) -> Option<ColorEffect> {
        if self.active.is_clear() { return None }
        Some (self.apply_effect_cycled (Some(false)))
    }

    pub(super) fn unapply_effect (&self) {
        self.active.clear();
        info! ("Clearing Full Screen Overlay color effect .. (the mode remains active)!");
        self.apply_color_effect (COLOR_EFF__IDENTITY);
    }

    fn apply_color_effect (&self, effect: MAGCOLOREFFECT) { unsafe {
        if ! MagSetFullscreenColorEffect (&effect as *const _ as _) .as_bool() {
            error! ("Error settting Fullscreen Color Effect : {:?}", GetLastError());
        }
    } }
    fn apply_effect_cycled (&self, forward: Option<bool>) -> ColorEffect {
        let effect = if let Some(forward) = forward { self.effect.cycle (forward) } else { (&self.effect).into() };
        info! ("Setting Full Screen Overlay Color Effect to : {:?}", effect);
        self.apply_color_effect (effect.get());
        self.active.set();
        effect
    }

}


impl Overlay {

    // Reminder : Windows created by one thread can only be removed by the same thread
    // .. hence all calls to here are best made from some single Overlay-Manager thread

    pub(super) fn new (target:Hwnd, effect:ColorEffect) -> Result <Overlay, String> { unsafe {

        let h_inst : Option<HINSTANCE> = GetModuleHandleW(None) .ok() .map(|h| h.into());

        // Create the host for the magniier control
        let Ok(host) = CreateWindowExW (
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR::from_raw (wide_string (HOST_WINDOW_CLASS_NAME).as_ptr()),
            PCWSTR::from_raw (wide_string (&format!("{} for {:#x}", HOST_WINDOW_TITLE, target.0)).as_ptr()),
            WS_POPUP, 0, 0, 0, 0, None, None, h_inst, None,
        ) else {
            return Err (format!("CreateWindowExW (Host) failed with error: {:?}", GetLastError()));
        };

        // Create Magnifier Control as child window of host with class WC_MAGNIFIERW
        let Ok(mag) = CreateWindowExW (
            WINDOW_EX_STYLE::default(), WC_MAGNIFIERW, PCWSTR::default(), WS_CHILD | WS_VISIBLE,
            0, 0, 0, 0, Some(host), None, h_inst, None,
        ) else {
            return Err (format!("CreateWindowExW (Magnifier) failed with error: {:?}", GetLastError()));
        };

        // we have enough to create the new overlay now
        let overlay = Overlay {
            host   : host.into(),
            mag    : mag.into(),
            target,
            effect : ColorEffectAtomic::new (effect),
            is_top : Flag::new(false),
            marked : Flag::new(false),
            viz_bounds : None,
        };

        // we'll apply the default smart inversion color-effect .. can ofc be cycled through via hotkeys later
        overlay.apply_color_effect (overlay.effect.get());

        // we'll mark the overlay which will make our main loop timer-handler sync dimensions and position with the target
        overlay.marked.set();

        Ok(overlay)
    } }


    fn update (&self, wd: &WinDusky) { unsafe {

        //tracing::debug! ("updating overlay {:?}", self);

        // lets clear the flag upfront before we start changing stuff (so it can be marked dirtied in the mean time)
        self.marked.clear();

        // we'll size both the host and mag to fit the target hwnd when hotkey was invoked

        let mut rect = RECT::default();
        let (host, mag, target) = (self.host.into(), self.mag.into(), self.target.into());

        //let _ = GetWindowRect (fgnd, &mut rect) .is_err();
        // ^^ getting window-rect includes (often transparent) padding, which we dont want to invert, so we'll use window frame instead
        if DwmGetWindowAttribute (target, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut RECT as _, size_of::<RECT>() as u32) .is_err() {
            error!( "@ {:?} update: DwmGetWindowAttribute (frame) failed with error: {:?}", target, GetLastError());
        }
        let (x, y, w, h) = (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top);

        if MagSetWindowSource (mag, rect) .as_bool() == false {
            error!( "MagSetWindowSource on mag-hwnd failed with error: {:?}", GetLastError());
        }
        if SetWindowPos (mag, None, 0, 0, w, h, Default::default()) .is_err() {
            error!( "SetWindowPos (w,h) on mag-hwnd failed with error: {:?}", GetLastError());
        }

        // for overlay host z-positioning .. we want the overlay to usually be just above the target hwnd, but not topmost
        // (the hope is to keep maintaining that such that other windows can come in front normally as well)
        // however .. while its fgnd, we'll make it top to avoid flashing etc (while the host and target switch turns being in front)
        // (and so then to keep these from lingering on top, we've added also sanitation to event listener itself)

        let fgnd : Hwnd = GetForegroundWindow().into();
        if self.target == fgnd {
            // now if some other overlay was previously on-top, we'll want to un-top it first
            let ov_top = wd.ov_topmost.load();
            if ov_top.is_valid() && ov_top != self.target {
                //tracing::debug! ("found active ov_top {:?}, will reorder it.", ov_top);
                if let Some(overlay) = wd.overlays .read().unwrap() .get (&ov_top) {
                    overlay.resync_ov_z_order();
                }
            }
        }
        //let hts : std::collections::HashSet<Hwnd> = vec! (self.host, self.target) .into_iter() .collect();
        //tracing::debug! ("(host,target): {:?}", (self.host, self.target));
        //tracing::debug! ("... pre-order : {:?}", crate::win_utils::win_get_hwnds_ordered(&hts));

        // now first, we'll do the general z-order repositioning
        let hwnd_insert = GetWindow (target, GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (host, None,               x, y, w, h,  SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW);
        let _ = SetWindowPos (host, Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOREDRAW);
        // ^^ the two step appears necessary, as w hwnd-insert specified, it doesnt seem to move/reposition the window!

        // next, if we are actually fgnd, we'll also try and set topmost (which OS might or might not always allow)
        if self.target == fgnd {
            let _ = SetWindowPos (host, Some(HWND_TOP),      0, 0, 0, 0,  SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetWindowPos (host, Some(HWND_TOPMOST),  0, 0, 0, 0,  SWP_NOMOVE | SWP_NOSIZE);
            // ^^ having both seems to be required for robustness, esp after freshly closing some overlain windows etc ¯\_(ツ)_/¯
            self.is_top.set();
            wd.ov_topmost .store (self.target);
        }
        //tracing::debug! ("... post-order : {:?}", crate::win_utils::win_get_hwnds_ordered(&hts));
    } }


    pub(super) fn refresh (&self, wd: &WinDusky) { unsafe {
        if self.marked.is_set() {
            // if we were marked for update, we'll update then invalidate our full rect
            self.update(wd);
            let _ = InvalidateRect (Some (self.mag.into()), None, false);
        }
        else if let Some(bounds) = self.viz_bounds {
            // otherwise we'll invalidate based on prior calculated occlusion bounds (if any)
            let mag: HWND = self.mag.into();
            let mut lpp = [ POINT {x:bounds.left, y:bounds.top}, POINT {x:bounds.right, y:bounds.bottom} ];
            if MapWindowPoints (None, Some(mag), &mut lpp) != 0 {
                let dirty = RECT { left:lpp[0].x, top:lpp[0].y, right:lpp[1].x, bottom:lpp[1].y };
                let _ = InvalidateRect (Some(mag), Some(&dirty), false);
            }
        }
    } }

    pub(super) fn destroy (&self) { unsafe {
        info! ("Clearing overlay for {:?}", self.target);
        let _ = DestroyWindow (self.host.into());
    } }


    pub(super) fn resync_ov_z_order (&self) { unsafe {
        self.is_top.clear();
        let hwnd_insert = GetWindow (self.target.into(), GW_HWNDPREV) .unwrap_or(HWND_TOP);
        let _ = SetWindowPos (self.host.into(), Some(hwnd_insert),  0, 0, 0, 0,  SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE);
    } }


    fn apply_color_effect (&self, effect: MAGCOLOREFFECT) { unsafe {
        if ! MagSetColorEffect (self.mag.into(), &effect as *const _ as _) .as_bool() {
            error! ("Setting Color Effect failed with error: {:?}", GetLastError());
        }
    } }
    fn apply_effect_cycled (&self, forward: bool) -> ColorEffect {
        let effect = self.effect.cycle (forward);
        self.apply_color_effect (effect.get());
        effect
    }
    pub(super) fn apply_effect_next (&self) -> ColorEffect { self.apply_effect_cycled (true) }
    pub(super) fn apply_effect_prev (&self) -> ColorEffect { self.apply_effect_cycled (false) }

}




// Window Procedure for the Host Window
unsafe extern "system" fn host_window_proc (
    host: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    // we'll just leave default message handling
    DefWindowProcW (host, msg, wparam, lparam)
}

