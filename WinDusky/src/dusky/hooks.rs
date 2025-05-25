

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, SetWindowsHookExW, HC_ACTION, WH_MOUSE_LL, WM_MOUSEMOVE};
use crate::dusky::WinDusky;
use crate::types::Hwnd;
use crate::win_utils;


impl WinDusky {

    pub(super) fn setup_win_events_hooks (&self) { unsafe {

        // Note this this must be called from some thread that will be monitoring its msg queue
        // Here, we'll setup a win-event hook to monitor fgnd change so we can maintain the overlay z-ordering
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
                // ^^ needed if want overlay to try keeping sync during window drag .. but will be laggy still

                0x8017 : EVENT_OBJECT_CLOAKED
                0x8018 : EVENT_OBJECT_UNCLOAKED
         */


        // we'll first setup simple callback handling for our win-event hook then register all callbacks

        unsafe extern "system" fn win_event_proc (
            _hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32,
            _id_child: i32, _event_thread: u32, _event_time: u32,
        ) {
            // we'll filter out non-window level events and pass up the rest
            use windows::Win32::UI::WindowsAndMessaging::OBJID_WINDOW;
            if id_object != OBJID_WINDOW.0 { return; }
            WinDusky::instance() .handle_win_hook_event (hwnd.into(), event);
        }


        let _ = SetWinEventHook (0x0003, 0x0003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook (0x0008, 0x000B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook (0x0016, 0x0017, None, Some(win_event_proc), 0, 0, 0 );

        let _ = SetWinEventHook (0x8000, 0x8003, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook (0x800B, 0x800B, None, Some(win_event_proc), 0, 0, 0 );
        let _ = SetWinEventHook (0x8017, 0x8018, None, Some(win_event_proc), 0, 0, 0 );

    } }


    fn handle_win_hook_event (&'static self, hwnd:Hwnd, event:u32) {
        use windows::Win32::UI::WindowsAndMessaging::*;

        // Note, only hwnd level events make it this far (child or non-window-obj events are filtered out)

        // // debug printout of all events .. useful during dev.. (enable all events first if so desired)
        // let overlays = self.overlays.read().unwrap();
        // if !hwnd.is_invalid() { //&& overlays.contains_key(&hwnd.into()) {
        //     let ov = if overlays.contains_key(&hwnd.into()) { "ov" } else { "  " };
        //     tracing::debug!("got event {:#06x} for {} hwnd {:?}, id-object {:#06x}, id-child {:#06x}", event, ov, hwnd, id_object, _id_child);
        // }

        // first off, lets ignore our own overlay hosts
        if self.hosts.read().unwrap() .contains (&hwnd) { return }

        // in full screen mode, there are no events that require any response
        if self.check_fs_mode() { return }
        // ^^ todo .. could prob just turn unhook events when toggling full-screen mode

        match event {

            EVENT_OBJECT_HIDE | EVENT_OBJECT_CLOAKED | EVENT_OBJECT_DESTROY => {
                // we treat hidden/closed/cloaked similarly by removing the overlay if there was any
                if self.overlays .read().unwrap() .contains_key (&hwnd) {
                    self.remove_overlay (hwnd)
                }
                self.occl_marked.set();
            }

            EVENT_SYSTEM_FOREGROUND => {
                // first, since we drive dusky from krusty-qbar, we'll ignore any fgnds from interacting there
                if win_utils::get_exe_by_hwnd (hwnd) .is_some_and (|exe| exe == "krustyboard.exe") {
                    return
                }
                // else we'll cache this as target for hotkeys actions (GetForegroundWindow  can return null during transitions etc)
                self.fgnd_cache.store (hwnd);

                // and we'll mark any other fgnd change as worth triggering occlusion updates
                self.occl_marked.set();

                let overlays = self.overlays.read().unwrap();

                // now if this hwnd already had overlays, we just mark it for udpate and request one
                if let Some(overlay) = overlays .get (&hwnd) {
                    overlay.marked.set();
                    return
                }
                // so we got a non-overlain hwnd to fgnd .. so if we had any overlain hwnds on-top, we should clear them
                if let Some(overlay) = overlays .get (&self.ov_topmost.load()) {
                    self.ov_topmost.clear();
                    overlay.resync_ov_z_order();
                }
                // finally we'll see if auto-overlay rules should apply to this
                self.auto.handle_auto_overlay (hwnd, self);
            }

            EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND | EVENT_SYSTEM_MOVESIZESTART | EVENT_SYSTEM_MOVESIZEEND |
            EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW | EVENT_OBJECT_UNCLOAKED | EVENT_OBJECT_LOCATIONCHANGE =>
            {
                // for these, we'll mark for occlusion update regardless of whether they were our hwnds
                self.occl_marked.set();

                if let Some(overlay) = self.overlays .read().unwrap() .get (&hwnd) {
                    overlay.marked.set();
                    self.post_req__refresh();
                }
            }
            _ => {
                // for all other registered events, we only process if hwnd had overlay, and if so we trigger an update
                if let Some(overlay) = self.overlays .read().unwrap() .get (&hwnd) {
                    self.occl_marked.set();
                    overlay.marked.set();
                    self.post_req__refresh();
                }
            }
        }
    }




    // Mouse hook for pointer movement
    pub fn setup_pointer_move_hook (&self) { unsafe {
        use windows::Win32::Foundation::{LRESULT};

        unsafe extern "system" fn mouse_proc (
            code: i32, wparam: WPARAM, lparam: LPARAM
        ) -> LRESULT {
            if code == HC_ACTION as i32  &&  wparam.0 == WM_MOUSEMOVE as usize {
                let wd = WinDusky::instance();
                if wd.mag_overlay.active.is_set() { wd.post_req__mag_refresh(); }
            }
            CallNextHookEx (None, code, wparam, lparam)
        }

        let _ = SetWindowsHookExW (WH_MOUSE_LL, Some(mouse_proc), None, 0);

    } }


}
