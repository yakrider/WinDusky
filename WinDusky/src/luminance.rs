#![allow (dead_code, non_snake_case)]

use crate::types::Hwnd;
use std::ops::Not;
use tracing::{info, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, SRCCOPY};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS, PW_CLIENTONLY};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;


#[cfg(debug_assertions)]
use minifb::{Key, KeyRepeat, Window};





// --- RAII Guards for GDI Resources ---

struct DcGuard (HDC);

impl Drop for DcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { let _ = DeleteDC (self.0); }
        }
    }
}

struct WindowDcGuard { hwnd: Option<HWND>, hdc: HDC }

impl Drop for WindowDcGuard {
    fn drop(&mut self) {
        if !self.hdc.is_invalid() {
            unsafe { let _ = ReleaseDC (self.hwnd, self.hdc); }
        }
    }
}

struct BitmapGuard (HBITMAP);

impl Drop for BitmapGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe { let _ = DeleteObject (self.0.into()); }
        }
    }
}

struct SelectGuard { hdc: HDC, h_old_obj: HGDIOBJ }

impl Drop for SelectGuard {
    fn drop(&mut self) {
        if !self.hdc.is_invalid() && !self.h_old_obj.is_invalid() {
            unsafe { let _ = SelectObject (self.hdc, self.h_old_obj); }
        }
    }
}






/// Helper to adjust scaling between PrintWindow output and our dpi-aware hwnd dimensions
fn calc_PrintWindow_scale_adj_for_hwnd (hwnd: HWND) -> Option<f64> { unsafe {

    let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let (mut mntr_dpi_x, mut mntr_dpi_y) = (0, 0);

    if hmonitor.is_invalid() { return None; }

    GetDpiForMonitor (hmonitor, MDT_EFFECTIVE_DPI, &mut mntr_dpi_x, &mut mntr_dpi_y) .ok()?;

    let hwnd_dpi = GetDpiForWindow(hwnd);
    if hwnd_dpi == 0 { return None; }

    Some ( hwnd_dpi as f64 / mntr_dpi_x as f64 )
} }




/// Capture window pixels using either the PrintWindow or the BitBlt method
fn capture_hwnd (hwnd:Hwnd, use_bitblt: bool) -> Option<(Vec<u8>, i32, i32)> { unsafe {

    let hwnd: HWND = hwnd.into();
    let capture_method = if use_bitblt { "BitBlt" } else { "PrintWindow" };

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect).ok()?;

    let (mut width, mut height) = (rect.right - rect.left, rect.bottom - rect.top);

    // PrintWindow processed by non-dpi-aware apps might come out unscaled so we should handle that
    if !use_bitblt {
        if let Some(scale_adj) = calc_PrintWindow_scale_adj_for_hwnd (hwnd) {
            if scale_adj != 1.0 {
                tracing::debug!("PrintWindow Target: {:?}, Dims: {}x{}, scale_adj: {}", hwnd, width, height, scale_adj);
            }
            width  = (width  as f64 * scale_adj) .round() as _;
            height = (height as f64 * scale_adj) .round() as _;
        }
    }

    let dc_hwnd = if use_bitblt { Some(hwnd) } else { None };
    let hdc = GetDC (dc_hwnd);
    let _guard = WindowDcGuard { hwnd: dc_hwnd, hdc };
    if hdc.is_invalid() { return None }

    let hdc_mem = CreateCompatibleDC (Some(hdc));
    let _mem_dc_guard = DcGuard (hdc_mem);
    if hdc_mem.is_invalid() { return None; }

    let h_bitmap = CreateCompatibleBitmap (hdc, width, height);
    let _bitmap_guard = BitmapGuard (h_bitmap);
    if h_bitmap.is_invalid() { return None; }

    let h_old_bitmap = SelectObject (hdc_mem, h_bitmap.into());
    let _select_guard = SelectGuard { hdc: hdc_mem, h_old_obj: h_old_bitmap };
    if h_old_bitmap.is_invalid() { return None; }

    let err = if use_bitblt {
        BitBlt (hdc_mem, 0, 0, width, height, Some(hdc), 0, 0, SRCCOPY) .is_err()
    } else {
        const PW_RENDERFULLCONTENT : u32 = 0x00000002;
        let pw_flags = PRINT_WINDOW_FLAGS (PW_CLIENTONLY.0 | PW_RENDERFULLCONTENT);
        PrintWindow (hwnd, hdc_mem, pw_flags).as_bool().not()
    };
    if err {
        warn!("Failed to capture {:?} using method: {}", hwnd, capture_method);
        return None;
    }

    let bmih = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
        biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0, biClrUsed: 0, biClrImportant: 0,
    };
    let mut bmi = BITMAPINFO { bmiHeader: bmih, bmiColors: Default::default() };

    let buffer_size = (width * height * 4) as usize;
    let mut buffer: Vec<u8> = vec![0; buffer_size];

    let result = GetDIBits (
        hdc_mem, h_bitmap, 0, height as u32,
        Some (buffer.as_mut_ptr() as *mut _),
        &mut bmi, DIB_RGB_COLORS,
    );

    if result == 0 || result == windows::Win32::Foundation::ERROR_INVALID_PARAMETER.0 as i32 {
        warn!("GetDIBits failed for capture of {:?} using method {:?}", hwnd, capture_method);
        return None
    }

    Some ((buffer, width, height))
} }




/// Calculates the average luminance of an hwnd
pub fn calculate_avg_luminance (hwnd: Hwnd, use_bitblt: bool) -> Option<u8> {

    // PrintWindow is generally preferred as it captures even if obscured etc by asking the window to paint itself to our DC
    // BitBlt is faster, but is more likely to capture un-painted hwnds when they first come-up, get-restored etc

    //let (buffer, width, height) = capture_hwnd (hwnd, true)?;
    //let (buffer, width, height) = capture_hwnd (hwnd, false)?;

    let (buffer, width, height) = capture_hwnd (hwnd, use_bitblt)?;

    //if hwnd == Hwnd(0xa71226) || hwnd == Hwnd(0x21360) || hwnd == Hwnd(0x6f12b0) {
    //    std::thread::spawn ( move || debug_display_hwnd_capture(hwnd,false) );
    //} // ^^ for debug on specific windows

    let num_pixels = (width * height) as usize;
    let mut total_luminance: f64 = 0.0;

    // we an subsample the pixels for efficiency .. say 1/20 pixels for 5% sampling etc
    const SAMPLING_STEP : usize = 20;

    for pixel_index in (0..num_pixels) .step_by (SAMPLING_STEP) {
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

    Some ((total_luminance / (num_pixels / SAMPLING_STEP) as f64 * u8::MAX as f64) as u8)
}





/// Displays the capture of a specific HWND in a window for debugging.
/// Only compiled in debug builds. Requires the `minifb` crate.
/// Intended to be ran in a spawned thread that wont return until the window is closed.
/// # Arguments
/// * `hwnd` - The handle of the window to capture.
/// * `use_bitblt` - If true, uses `BitBlt`; otherwise uses `PrintWindow`.
#[cfg(debug_assertions)]
pub fn debug_display_hwnd_capture(hwnd: Hwnd, use_bitblt: bool) {
    let capture_method = if use_bitblt { "BitBlt" } else { "PrintWindow" };
    info! ("Attempting debug capture for {:?} using {}", hwnd, capture_method);

    if let Some((buffer, width, height)) = capture_hwnd (hwnd, use_bitblt) {

        let (width, height) = (width as usize, height as usize);

        tracing::debug! ("Capture successful ({}x{}). Converting buffer format.", width, height);

        if width * height > 4096*4096 { return; }   // for safety

        // Convert BGRA u8 buffer to ARGB u32 buffer for minifb.
        // minifb expects 0xAARRGGBB or 0x00RRGGBB. We'll use 0xFFRRGGBB (opaque).
        let buffer_argb: Vec<u32> = buffer
            .chunks_exact(4)
            .map(|bgra| {
                let b = bgra[0] as u32;
                let g = bgra[1] as u32;
                let r = bgra[2] as u32;
                // let a = bgra[3] as u32; // ignore alpha
                (0xFF << 24) | (r << 16) | (g << 8) | b
            })
            .collect();

        let title = format! ("Debug Capture: {:?} ({}x{}) - via {}", hwnd, width, height, capture_method);

        let mut window = match Window::new (&title, width as _, height as _, Default::default()) {
            Ok(win) => win,
            Err(e) => {
                warn!("Failed to create debug window for {:?}: {}", hwnd, e);
                return;
            }
        };
        window.set_target_fps(60);

        info!("Displaying debug capture for {:?}. Press ESC to close.", hwnd);
        while window.is_open() && !window.is_key_pressed (Key::Escape, KeyRepeat::No) {
            if window.update_with_buffer (&buffer_argb, width, height) .is_err() { break }
        }
        info!("Closed debug capture window for {:?}.", hwnd);
    }
    else {
        warn!( "Debug capture failed for HWND {:?} using method: {}", hwnd, capture_method );
    }

}
