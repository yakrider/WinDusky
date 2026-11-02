
use std::sync::OnceLock;
use rand::RngExt;
use windows::core::{w, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, HWND_MESSAGE, MSG, WNDCLASSW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{RegisterPowerSettingNotification, RegisterSuspendResumeNotification, POWERBROADCAST_SETTING};
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::System::SystemServices::GUID_SESSION_DISPLAY_STATUS;

use crate::dusky::WinDusky;
use crate::types::Flag;


#[derive (Debug, Clone, Copy)]
pub enum SysEvent {
    Suspend, Resume, Lock, Unlock, Logon, Logoff, DisplayOn, DisplayOff
}


pub struct SysEventsListener {
    is_hooked : Flag,
}


impl SysEventsListener {

    pub fn instance () -> &'static SysEventsListener {
        static INSTANCE: OnceLock <SysEventsListener> = OnceLock::new();
        let inst = INSTANCE .get_or_init ( || {
            SysEventsListener {
                is_hooked : Flag::default(),
            }
        } );
        // we could in theory, set up the listener here directly, but we'll go with convention in rest of code and let user do it
        //inst.setup_system_events_listener();
        inst
    }


    pub fn setup_system_events_listener (&'static self) {

        // if we already tried to start once, we dont try to do so again
        if self.is_hooked.is_clear() { self.is_hooked.set(); } else { return }

        tracing::info! ("Starting System Events Listener..");

        std::thread::spawn ( move || {
            if let Err(e) = self.attempt_system_events_listener_setup() {
                tracing::error! ("FAILURE setting up system events listener : {}", e);
            }
        } );
    }

    fn attempt_system_events_listener_setup (&'static self) -> Result<()> { unsafe {
        let hinst = GetModuleHandleW (None)?;
        let class = w!("dusky_sysevents_listener");
        RegisterClassW (&WNDCLASSW {
            lpfnWndProc: Some (Self::win_evs_proc_cb),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW (
            Default::default(), class, w!(""), Default::default(),
            0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst.into()), None,
        )?;

        WTSRegisterSessionNotification (hwnd, NOTIFY_FOR_THIS_SESSION)?;
        let _susp = RegisterSuspendResumeNotification (hwnd.into(), DEVICE_NOTIFY_WINDOW_HANDLE)?;
        let _disp = RegisterPowerSettingNotification (
            hwnd.into(), &GUID_SESSION_DISPLAY_STATUS, DEVICE_NOTIFY_WINDOW_HANDLE,
        )?;

        let mut msg = MSG::default();
        while GetMessageW (&mut msg, None, 0, 0) .as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok (())
    } }



    fn process_sys_event (&'static self, event: SysEvent) {
        use SysEvent::*;
        //tracing::debug! ("received system event: {event:?}");
        match event {
            Logon | Resume | Unlock | DisplayOn => {
                std::thread::spawn (|| {
                    let jitter_ms = 300 + rand::rng().random_range(0..500) as u64;
                    std::thread::sleep (std::time::Duration::from_millis (jitter_ms));
                    WinDusky::instance().post_req__gamma_guard_recheck();
                });
            },
            _ => {}
        }
    }


    unsafe extern "system" fn win_evs_proc_cb (hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT { unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;   // for msg constants
        use SysEvent::*;                                  // for SysEv_ enums

        let sel = SysEventsListener::instance();

        match msg {

            WM_POWERBROADCAST => {
                match wp.0 as u32 {

                    PBT_POWERSETTINGCHANGE => {
                        let s = &*(lp.0 as *const POWERBROADCAST_SETTING);
                        if s.PowerSetting == GUID_SESSION_DISPLAY_STATUS {
                            match s.Data[0] {
                                0 => sel.process_sys_event (DisplayOff),
                                1 => sel.process_sys_event (DisplayOn),
                                _ => {}
                    }  }   }

                    PBT_APMSUSPEND         => sel.process_sys_event (Suspend),
                    PBT_APMRESUMEAUTOMATIC => sel.process_sys_event (Resume),

                    _ => {}
                }
                LRESULT (0)
            }

            WM_WTSSESSION_CHANGE => {
                match wp.0 as u32 {
                    WTS_SESSION_LOCK    => sel.process_sys_event (Lock),
                    WTS_SESSION_UNLOCK  => sel.process_sys_event (Unlock),
                    WTS_SESSION_LOGON   => sel.process_sys_event (Logon),
                    WTS_SESSION_LOGOFF  => sel.process_sys_event (Logoff),
                    _ => {}
                }
                LRESULT (0)
            }

            _ => DefWindowProcW (hwnd, msg, wp, lp),
        }
    } }


}
