

use std::collections::{HashMap, HashSet};

use windows::core::{Result, BOOL};
use windows::Win32::Foundation::{FALSE, HWND, LPARAM, RECT, TRUE};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, IsWindowVisible};

use crate::dusky::WinDusky;
use crate::types::Hwnd;
use crate::win_utils;





/// Rect structure that supports decompositions into progressively non-intersecting sub-rects
#[derive (Debug, Default, Clone, Copy)]
pub struct Rect {
    pub left   : i32,
    pub top    : i32,
    pub right  : i32,
    pub bottom : i32,
}


impl From <Rect> for RECT {
    fn from (rect: Rect) -> Self {
        RECT { left: rect.left,  top: rect.top,  right: rect.right,  bottom: rect.bottom }
    }
}
impl From <RECT> for Rect {
    fn from (rect: RECT) -> Self {
        Rect { left: rect.left,  top: rect.top,  right: rect.right,  bottom: rect.bottom }
    }
}


impl Rect {

    fn is_empty (&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    fn intersect (&self, other: &Rect) -> Option <Rect> {
        let left   = self.left   .max (other.left);
        let top    = self.top    .max (other.top);
        let right  = self.right  .min (other.right);
        let bottom = self.bottom .min (other.bottom);

        if right > left && bottom > top {
            Some (Rect { left, top, right, bottom })
        } else {
            None
        }
    }

    fn bounding (&self, other: &Rect) -> Rect {
        Rect {
            left   : self.left   .min (other.left),
            top    : self.top    .min (other.top),
            right  : self.right  .max (other.right),
            bottom : self.bottom .max (other.bottom),
        }
    }

    /// Subtracts 'other' rect from 'self', adding the remaining pieces of 'self' to 'out'.
    /// Returns true if 'self' was changed (i.e., if there was an intersection).
    fn subtract_into (&self, other: &Rect, out: &mut Vec<Rect>) -> bool {

        if let Some (isect) = self.intersect(other) {
            if isect.top > self.top {   // top slice
                out.push ( Rect { bottom: isect.top,  ..*self });
            }
            if isect.bottom < self.bottom {   // bottom slice
                out.push ( Rect { top: isect.bottom,  ..*self });
            }
            if isect.left > self.left {   // left slice (within isect height)
                out.push ( Rect { left: self.left, right: isect.left,  ..isect });
            }
            if isect.right < self.right {   // right slice (within isect height)
                out.push ( Rect { left: isect.right, right: self.right,  ..isect });
            }
            // we return true to indicate instersection and substraction was performed
            return true
        }

        // else, there was nothing to do, so we put the orig rect as output and return false
        out.push(*self);
        false
    }
}





/// Struct to store data for each hwnd we're calculating occlusion for
#[derive (Debug, Default)]
struct HwndDat {
    rect : Rect,
    // ^^ we'll keep the full rect so we can quickly skip on non-intersecting hwnd report

    seen_self : bool,
    // ^^ whether our own hwnd has already been seen this enum call, ie can ignore rest of streamed enum hwnds

    viz_sects : Vec<Rect>,
    // ^^ sections of our window that are still visible

    sects_swap : Vec<Rect>,
    // ^^ reusable scratch area to avoid allocating new vectors to swap results into
}




// Struct to store data for all the hwnds we are calculating occlusion for in this pass
struct BatchDat {
    hosts : HashSet <Hwnd>,
    // ^^ we'll keep a local copy of hosts so we can skip them when calculating occlusion

    dats : HashMap <Hwnd, HwndDat>,
    // ^^ data for all the hwnds we're tracking

    n_viz : usize,
    // ^^ running count of how many targets are still at least minimally un-occluded

    n_self_unseen : usize,
    // ^^ running count of how many targets still have yet to show up in enum call (or we could just stop)
}




// The actual callback function
unsafe extern "system" fn enum_windows_proc (hwnd: HWND, lparam: LPARAM) -> BOOL {

    // Cast LPARAM back to our struct pointer
    if lparam.0 == 0 { return FALSE; }

    if !IsWindowVisible (hwnd).as_bool() { return TRUE; }
    if win_utils::check_window_cloaked (hwnd.into()) { return TRUE; }

    let batch = &mut *(lparam.0 as *mut BatchDat);
    if batch.hosts.contains (&hwnd.into()) { return TRUE; }
    // ^^ host hwnds always ovelay targets, no point processing them separately

    // Get the rectangle of src window causing potential occlusion
    let mut rect = RECT::default();
    if GetWindowRect (hwnd, &mut rect) .is_err() { return TRUE; }
    let src: Rect = rect.into();

    // Iterate through the target windows we are tracking
    for (&target, dat) in batch.dats.iter_mut() {

        if dat.viz_sects.is_empty() { continue; }
        // ^^ the hwnd were already completely occluded

        if dat.seen_self { continue; }
        // ^^ its own hwnd already came up before in enum list, so nothing afterwards can occlude it

        if target == hwnd.into() {
            // this is its own hwnd, so nothing else streaming after this can block it, we'll mark it for short-circuit
            dat.seen_self = true;
            batch.n_self_unseen = batch.n_self_unseen .saturating_sub(1);
            // further, if everyone in the batch has been seen, we can just exit the enum call itself
            if batch.n_self_unseen == 0 { return FALSE; }
            continue;
        }

        if dat.rect.intersect(&src) .is_none() { continue; }
        // ^^ if there isnt even any overlap, no need to go through section by section calcs

        let mut changed = false;
        dat.sects_swap.clear();

        // decompose every viz sect into sects un-occluded by the occlusion src
        for sect in dat.viz_sects.iter() {
            changed |= sect .subtract_into (&src, &mut dat.sects_swap)
        }

        if changed {
            // lets check if we got fully occluded after processing this occl src
            dat.sects_swap .retain (|r| !r.is_empty());
            if !dat.viz_sects.is_empty() && dat.sects_swap.is_empty() {
                batch.n_viz = batch.n_viz .saturating_sub(1);
            }
            // finally we can swap the updated list
            std::mem::swap (&mut dat.viz_sects, &mut dat.sects_swap);
        }
    }

    // if all targets are fully occluded, we can stop enumerating
    if batch.n_viz == 0 { return FALSE; }

    // if we havent short-circuited early, we keep continuing the enumeration
    TRUE
}





/// Calculates the un-occluded status of target HWNDs.
/// Returns a mapping of Hwnd to a bounding rect of the un-occluded regiions, if any
pub fn calc_viz_bounds (wd: &WinDusky, targets: &[Hwnd]) -> Result <Vec <(Hwnd, Option<Rect>)>> {

    // we'll init the batch data with the target rects
    let mut dats : HashMap <Hwnd, HwndDat> = HashMap::new();
    for &target in targets { unsafe {
        let mut viz_sects : Vec<Rect> = vec![];
        let mut rect = RECT::default();
        let _ = GetWindowRect (target.into(), &mut rect);
        let rect:Rect = rect.into();
        if !rect.is_empty() { viz_sects = vec![rect]; }
        dats.insert (target, HwndDat { rect, seen_self:false, viz_sects, sects_swap: Vec::new() });
    } }

    let n_viz = dats.values() .filter (|dat| !dat.viz_sects.is_empty()) .count();
    let n_self_unseen = targets.len();
    let hosts = wd.get_hosts();
    let mut batch = BatchDat { hosts, dats, n_viz, n_self_unseen };

    //tracing::debug! ("Starting occlusion check. Initial visible targets: {}", batch.n_viz);
    //tracing::debug! ("{:#?}", &batch.dats);

    if n_viz > 0 { unsafe {
        // we'll pass the batch-data via LPARAM in EnumWindows so the callback can have access to it
        let _ = EnumWindows (Some (enum_windows_proc), LPARAM (&mut batch as *mut _ as isize));
    } }

    // Convert the final lists of visible sections into bounding rects
    let mut result = vec![];
    for (hwnd, dat) in batch.dats.into_iter() {
        let bounding = dat.viz_sects .into_iter() .reduce (|a,b| a.bounding(&b));
        result .push ((hwnd, bounding));
    }
    //result.sort();
    // ^^ only for consistentcy during debug printouts

    Ok (result)
}
