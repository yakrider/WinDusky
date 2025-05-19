use std::ffi::OsString;
use std::mem::zeroed;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use tracing::{info, warn};
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, EnumDisplayMonitors, GetDC, GetMonitorInfoW, ReleaseDC, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};





#[derive (Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct GBC {
    gamma    : f32,   // default 1.0 .. expectation 0.0 and up
    bright   : f32,   // default 0.0 .. expectation -1.0 to 1.0
    contrast : f32,   // default 1.0 .. expectation  0.0 and up
    // todo .. ^^ update these comments with the clamp values we use
}
impl Default for GBC {
    fn default() -> GBC {
        GBC { gamma : 1.0, bright : 0.0, contrast : 1.0 }
    }
}





#[allow (dead_code)]
#[derive (Debug, Clone)]
pub struct MonitorInfo {
    pub hmonitor: HMONITOR,
    pub device_name: String,
    pub rect: RECT,
}



impl GBC {

    pub fn new (gamma: f32, bright: f32, contrast: f32) -> GBC {
        GBC { gamma, bright, contrast }
    }

    /// Gamma ramp are arrays of 256 mapping values for each channel (r,g,b)
    pub fn create_gamma_ramp (&self) -> [[u16; 256]; 3] {
        let max_gamma = 4.4f32;
        let min_gamma = 0.3f32;
        let gamma = self.gamma.clamp(min_gamma, max_gamma);

        let max_bright = 1.0f32;
        let min_bright = -1.0f32;
        let bright = self.bright.clamp(min_bright, max_bright);

        let max_contrast = 100.0f32;
        let min_contrast = 0.1f32;
        let contrast = self.contrast.clamp(min_contrast, max_contrast);

        let inv_gamma = 1.0 / gamma as f64;
        let norm = 255.0f64.powf(inv_gamma - 1.0);
        let mut ramp = [[0u16; 256]; 3];
        for i in 0..256 {
            let mut val = i as f64 * contrast as f64 - (contrast as f64 - 1.0) * 127.0;
            if (gamma - 1.0).abs() > 1e-6 {
                val = val.powf(inv_gamma) / norm;
            }
            val += bright as f64 * 128.0;
            let v = (val * 256.0).round() as i32;
            let v = v.clamp(0, 65535) as u16;
            (ramp[0][i], ramp[1][i], ramp[2][i]) = (v, v, v);
        }
        ramp
    }
}





/// Color temperature to RGB (0..1 floats)
pub fn color_temp_to_rgb (kelvin: u32) -> [f32; 3] {
    let temp = kelvin.clamp (2000, 10000) as f32 / 100.0;
    let mut r: f32;
    let mut g: f32;
    let mut b: f32;
    if temp <= 66.0 {
        r = 255.0;
    } else {
        r = 329.70 * (temp - 60.0) .powf (-0.133);
        r = r .clamp (0.0, 255.0);
    }
    if temp <= 66.0 {
        g = 99.47 * temp.ln() - 161.12;
        g = g .clamp (0.0, 255.0);
    } else {
        g = 288.12 * (temp - 60.0) .powf (-0.075);
        g = g .clamp (0.0, 255.0);
    }
    if temp >= 66.0 {
        b = 255.0;
    } else if temp <= 19.0 {
        b = 0.0;
    } else {
        b = 138.5 * (temp - 10.0).ln() - 305.05;
        b = b .clamp (0.0, 255.0);
    }
    [r / 255.0, g / 255.0, b / 255.0]
}



/// Blend/apply color temperature to a gamma ramp
pub fn apply_color_temp_to_ramp (ramp: &mut [[u16; 256]; 3], color_temp: u32) {
    let rgb_std = color_temp_to_rgb (6500);
    let rgb_target = color_temp_to_rgb (color_temp);
    for c in 0..3 {
        let mult = rgb_target[c] / rgb_std[c].max(1e-6);
        for i in 0..256 {
            let v = ( ramp[c][i] as f32 * mult ) .round() as i32;
            ramp[c][i] = v .clamp (0, 65535) as u16;
        }
    }
}





/// Enumerate all monitors and return their info <br>
/// (For now, just a stub until we add per-monitor support)
#[allow (dead_code)]
pub fn enumerate_monitors() -> Vec<MonitorInfo> {

    let mut monitors = Vec::new();

    unsafe extern "system" fn monitor_enum_proc (
        hmonitor: HMONITOR, _hdc: HDC, _lprc: *mut RECT, lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam.0 as *mut Vec<MonitorInfo>);
        let mut mi: MONITORINFOEXW = zeroed();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW (hmonitor, &mut mi.monitorInfo as *mut _ as *mut _) .as_bool() {
            let name = OsString::from_wide(&mi.szDevice) .to_string_lossy() .trim_end_matches('\0') .to_string();
            let info = MonitorInfo { hmonitor, device_name: name, rect: mi.monitorInfo.rcMonitor };
            monitors.push (info);
        }
        true.into()
    }

    let lparam = LPARAM (&mut monitors as *mut _ as isize);
    unsafe {
        let _ = EnumDisplayMonitors (None, None, Some(monitor_enum_proc), lparam);
    }
    monitors
}


/// Get a device context for the entire screen
pub fn get_screen_dc () -> Option <HDC> { unsafe {
    let hdc = GetDC (None);
    if hdc.is_invalid() {
        warn! ("Failed to get screen DC for gamma toggle");
        return None;
    }
    Some (hdc)
} }


/// Get a device context for a given device name (monitor)
#[allow (dead_code)]
pub fn get_monitor_dc (device_name: &str) -> Option<HDC> { unsafe {
    let device: Vec<u16> = OsString::from (device_name) .encode_wide() .chain([0]) .collect();
    let hdc = CreateDCW (PCWSTR::default(), PCWSTR::from_raw (device.as_ptr()), PCWSTR::default(), None);
    (!hdc.is_invalid()) .then_some (hdc)
} }


/// Set the gamma ramp for a given DC (expects ramp as [[u16; 256]; 3])
pub fn set_gamma_ramp_for_dc (hdc: HDC, ramp: &[[u16; 256]; 3]) -> bool { unsafe {
    SetDeviceGammaRamp (hdc, ramp.as_ptr() as *const _) .as_bool()
} }

/// Get the current gamma ramp from the OS for a given DC
pub fn get_current_gamma_ramp (hdc: HDC) -> Option<[[u16; 256]; 3]> { unsafe {
    let mut ramp = [[0u16; 256]; 3];
    if !GetDeviceGammaRamp (hdc, ramp.as_mut_ptr() as *mut _) .as_bool() {
        warn! ("Failed to receive current gamma-ramp for screen DC");
        return None;
    }
    Some (ramp)
} }


#[allow (dead_code)]
pub fn delete_dc (hdc: HDC) { unsafe {
    let _ = DeleteDC (hdc);
} }

pub fn release_dc (hdc:HDC) { unsafe {
    let _ = ReleaseDC (None, hdc);
} }


pub fn calc_gbct_ramp (gbc: &GBC, t: u32) -> [[u16; 256]; 3] {
    let mut ramp = gbc.create_gamma_ramp();
    apply_color_temp_to_ramp (&mut ramp, t);
    ramp
}

pub fn set_screen_ramp_gbct (gbc: &GBC, t: u32) -> bool {
    let Some(hdc) = get_screen_dc() else { return false };
    info! ("Applying Gamma Ramp for screen to {:?}, temp: {:?}", &gbc, t);
    let ramp = calc_gbct_ramp (gbc, t);
    let succeeded = set_gamma_ramp_for_dc (hdc, &ramp);
    if !succeeded { warn! ("Failure setting gamma ramp {:?} t:{:?} for screen DC", &gbc, t) }
    release_dc (hdc);
    succeeded
}
pub fn reset_screen_ramp () {
    set_screen_ramp_gbct (&GBC::default(), 6500);
}

pub fn check_active_gamma_match (gbc: &GBC, t: u32) -> Option <bool> {
    let hdc = get_screen_dc()?;
    let cur_ramp = get_current_gamma_ramp(hdc)?;
    let expected_ramp = calc_gbct_ramp (gbc, t);
    release_dc (hdc);
    Some (cur_ramp == expected_ramp)
}





#[cfg(test)]
mod tests {
    use super::*;

    fn test_dc_gamma (hdc: HDC) {
        println!("Setting gamma ramp to GBC (1.1, -0.25, 0.9) .. ");
        let ramp = GBC { gamma: 1.1, bright: -0.25, contrast: 0.9 }.create_gamma_ramp();
        assert! (set_gamma_ramp_for_dc (hdc, &ramp), "Failed to set gamma ramp to custom GBC");
        std::thread::sleep (std::time::Duration::from_millis(2000));

        println!("Restoring gamma ramp to default GBC (1,0,1)");
        let ramp = GBC { gamma: 1.0, bright: 0.0, contrast: 1.0 }.create_gamma_ramp();
        assert! (set_gamma_ramp_for_dc (hdc, &ramp), "Failed to set gamma ramp to default GBC");
        //std::thread::sleep (std::time::Duration::from_millis(1000));

        println!("Setting gamma ramp to color temp 5000 .. ");
        let mut ramp = GBC::default().create_gamma_ramp();
        apply_color_temp_to_ramp (&mut ramp, 5000);
        assert! (set_gamma_ramp_for_dc (hdc, &ramp), "Failed to set gamma ramp to color temp 6000");
        std::thread::sleep (std::time::Duration::from_millis(2000));

        println!("Restoring gamma ramp to color temp 6500");
        let mut ramp = GBC::default().create_gamma_ramp();
        apply_color_temp_to_ramp (&mut ramp, 6500);
        assert! (set_gamma_ramp_for_dc (hdc, &ramp), "Failed to set gamma ramp to color temp 6500");
        //std::thread::sleep (std::time::Duration::from_millis(3000));
    }

    #[test]
    fn test_gamma_monitors () {
        let monitors = enumerate_monitors();
        println!("Found {} monitors", monitors.len());

        for (i, mon) in monitors.iter().enumerate() {
            println!("Monitor {}: {} rect: {:?}", i, mon.device_name, mon.rect);

            let Some(hdc) = get_monitor_dc (&mon.device_name) else {
                println!("Could not get DC for monitor {}", mon.device_name);
                return
            };
            test_dc_gamma (hdc);
            delete_dc (hdc);
        }
    }

    #[test]
    fn test_gamma_screen () {
        let Some(hdc) = get_screen_dc() else {
            println! ("Could not get Screen DC");
            return
        };
        test_dc_gamma (hdc);
        delete_dc (hdc);
    }

}
