#![allow (non_snake_case)]

use std::collections::LinkedList;
use std::io::Cursor;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::str::FromStr;
use std::sync::{LazyLock, OnceLock};
use std::thread;
use std::time::Duration;

use image::{ImageFormat, ImageReader};
use itertools::Itertools;
use linked_hash_map::LinkedHashMap;
use strum_macros::{AsRefStr, EnumString};
use tracing::{error, warn};

use tao::event::{DeviceEvent, DeviceId, ElementState, Event, MouseScrollDelta};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::EventLoopBuilderExtWindows;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS};

use crate::dusky::{WinDusky, MagEffect, MAG_EFFECT_IDENTITY};
use crate::effects::{ColorEffect};
use crate::presets::GammaPreset;
use crate::types::{Flag, Hwnd};
use crate::{autostart, win_utils};
use crate::win_utils::{check_window_exists, check_window_visible, check_window_cloaked};

const ICON_BYTES: &[u8] = include_bytes!("../WinDusky_128.png");

fn get_dusky_icon() -> Icon {
    let icon = ImageReader::with_format (Cursor::new (ICON_BYTES.to_vec()), ImageFormat::Png).decode().unwrap();
    Icon::from_rgba (icon.into_bytes(), 128, 128).unwrap()
}



// We'd like to have a way to update the tray menu entries upon internal events ..
// However, the menus have Rc internally, and cant be made Sync .. so they cant be stored in an instance etc .. jeez
// So instead, we make a proxy to pump custom events to the runloop, and store a clone of that ..
// Then we can send custom events for the menu to act on!

static TRAY_EVENTS_PROXY : OnceLock <EventLoopProxy <DuskyEvent>> = OnceLock::new();



#[derive (Debug)]
pub enum DuskyEvent {
    MenuAction (MenuEvent),

    AutoOverlayEnable (bool),

    UpdateAutoStart { elevated: bool, autostart_enabled: bool, elev_autostart_enabled: bool, elev_link_created: bool },

    #[allow (unused)]
    // ^^ we stopped using 'effect' below when centralizing effects-update which looks into fs-overlay itself
    // .. (but we'd rather keep it around than rip it out just from clippy lints annoyance)
    FullScreenMode { enabled: bool, effect: Option <ColorEffect> },

    OverlayCreated { hwnd: Hwnd, effect: ColorEffect },
    OverlayCycled  { hwnd: Hwnd, effect: ColorEffect },
    OverlayRemoved { hwnd: Hwnd },
    OverlaysCount  { n_active : usize },

    OverrideAdded   { hwnd: Hwnd },
    OverrideRemoved { hwnd: Hwnd },
    OverridesCount  { n_overrides : usize },

    GammaState { applied: bool, succeeded: bool, preset: Option <GammaPreset> },
    MagLevel { level: Option <MagEffect> },

    FgndHwnd { hwnd: Option<Hwnd>, has_overlay: bool },

    RefreshMenus,
}

impl DuskyEvent {
    fn send (self) {
        if let Some(proxy) = TRAY_EVENTS_PROXY.get() {
            let _ = proxy.send_event (self);
        }
    }
}




#[derive (Debug, EnumString, AsRefStr)]
#[strum (serialize_all = "snake_case")]
enum MenuId {
    FullScreenMode,
    AutoOverlayEnabled,

    OverlaysSubmenu,
    OverridesSubmenu,
    FgRecentsSubmenu,
    EffectsSubmenu,
    PresetsSubmenu,
    MagSubmenu,

    OverlaysSubNone,
    OverridesSubNone,
    EffectsSubNone,
    PresetsSubNone,
    MagSubNone,

    Configs,
    Version,
    AutoStart,
    AutoStartElev,
    GenElevLink,
    EditConf,
    ResetConf,
    OpenConfLoc,

    Restart,
    Exit,
}


impl MenuId {
    fn text (&self) -> &str {
        use MenuId::*;
        static VERSION : LazyLock <String> = LazyLock::new (|| format! ("(WinDusky v{:})", env!("CARGO_PKG_VERSION")));
        match self {
            FullScreenMode     => "Enable Full Screen Effect",
            AutoOverlayEnabled => "Auto Overlay Enabled",

            OverlaysSubmenu   => "Active Overlays : 0",
            OverridesSubmenu  => "User Overrides : 0",
            FgRecentsSubmenu  => "Recent Windows",
            EffectsSubmenu    => "Color Effect : None",
            PresetsSubmenu    => "Gamma Preset : None",
            MagSubmenu        => "Magnification Level : None",

            OverlaysSubNone   => "(Overlays: None)",
            OverridesSubNone  => "(Overrides: None)",
            EffectsSubNone    => "(Effect: None)",
            PresetsSubNone    => "(Preset: None)",
            MagSubNone        => "(Level: None)",

            Configs        => "Configs",
            Version        => &VERSION,
            AutoStart      => "Auto Start on Login",
            AutoStartElev  => "Auto Start as Admin",
            GenElevLink    => "Create Elev Launch Link",
            OpenConfLoc    => "Open Config Location",
            EditConf       => "Edit Config File",
            ResetConf      => "Reset Config File",

            Restart  => "Restart",
            Exit     => "Exit",
        }
    }
}



#[derive (Debug, EnumString, AsRefStr)]
#[strum (serialize_all = "snake_case")]
enum MenuPrefix {
    Overlay,
    FgRecent,
    Override,
    Effect,
    Preset,
    Mag,
}

impl MenuPrefix {
    fn id <T: std::fmt::Display> (&self, suffix: T) -> String {
        format! ("{}-{}", self.as_ref(), suffix)
    }
    fn pfx_matches (&self, id: &str) -> bool {
        id .starts_with (self.as_ref())
    }
}




// we'll bundle all the data we need for tray fns into one wrapper struct .. (just code sugar)

struct PresetsSubMenuEntry   { id: String, menu_item: CheckMenuItem, preset: GammaPreset }
struct MagSubMenuEntry       { id: String, menu_item: CheckMenuItem, level:  MagEffect   }
struct EffectsSubMenuEntry   { id: String, menu_item: CheckMenuItem, effect: ColorEffect }
struct OverridesSubMenuEntry { id: String, menu_item: CheckMenuItem, hwnd:Hwnd, exe:String, title:String }
struct OverlaysSubMenuEntry  { id: String, menu_item: CheckMenuItem, hwnd:Hwnd, exe:String, title:String, effect:ColorEffect }
struct FgRecentsSubMenuEntry { id: String, menu_item: CheckMenuItem, hwnd:Hwnd, exe:String, title:String }


struct TrayDat {
    wd : &'static WinDusky,

    #[allow (unused)]
    tray : TrayIcon,
    // ^^ we might not reference this in code, but it must remain in scope to avoid dropping the tray-icon itself!

    full_screen_mode : CheckMenuItem,
    auto_ov_enabled  : CheckMenuItem,

    overlays   : Vec <OverlaysSubMenuEntry>,
    effects    : Vec <EffectsSubMenuEntry>,
    overrides  : Vec <OverridesSubMenuEntry>,
    presets    : Vec <PresetsSubMenuEntry>,
    mag_levels : Vec <MagSubMenuEntry>,

    fg_recents : LinkedHashMap <Hwnd, FgRecentsSubMenuEntry>,
    // ^^ we'll use a limited cache of recent hwnds to let the user toggle effects directly
    // Note that we'll keep a larger buffer than what we intend to display, partly to serve as cache for hwnd's exe etc

    overlays_submenu   : Submenu,
    fg_recents_submenu : Submenu,
    effects_submenu    : Submenu,
    overrides_submenu  : Submenu,
    presets_submenu    : Submenu,
    mag_submenu        : Submenu,

    // we'll keep a bunch of flags to indicate submenus need rebuilding (instead of immediately rebuilding them)
    // (note that we dont need these for gamma/mag as there are no auto/hook events of relevance there)
    overlays_subs_dirty   : Flag,
    fg_recents_subs_dirty : Flag,
    effects_subs_dirty    : Flag,
    overrides_subs_dirty  : Flag,

    overlays_none  : CheckMenuItem,
    effects_none   : CheckMenuItem,
    overrides_none : CheckMenuItem,
    presets_none   : CheckMenuItem,
    mag_none       : CheckMenuItem,

    auto_start       : CheckMenuItem,
    auto_start_elev  : CheckMenuItem,
    elev_launch_link : CheckMenuItem,
}




fn build_tray (wd: &'static WinDusky) -> TrayDat {

    fn make_menu_item (m:MenuId, enabled:bool) -> MenuItem {
        MenuItem::with_id (m.as_ref(), m.text(), enabled, None)
    }
    fn make_menu_check (m:MenuId, enabled:bool, checked:bool) -> CheckMenuItem {
        CheckMenuItem::with_id (m.as_ref(), m.text(), enabled, checked, None)
    }
    fn make_sub_menu (m:MenuId, enabled:bool) -> Submenu {
        Submenu::with_id (m.as_ref(), m.text(), enabled)
    }

    let sep = PredefinedMenuItem::separator();

    let full_screen_mode = make_menu_check (MenuId::FullScreenMode, true, false);
    let auto_ov_enabled  = make_menu_check (MenuId::AutoOverlayEnabled, true, true);

    let is_elev = win_utils::check_cur_proc_elevated().unwrap_or_default();
    let auto_start       = make_menu_check (MenuId::AutoStart,     !is_elev, false);
    let auto_start_elev  = make_menu_check (MenuId::AutoStartElev,  is_elev, false);
    let elev_launch_link = make_menu_check (MenuId::GenElevLink,    is_elev, false);

    let version       = make_menu_item (MenuId::Version, false);
    let open_conf_loc = make_menu_item (MenuId::OpenConfLoc, true);
    let edit_conf     = make_menu_item (MenuId::EditConf, true);
    let reset_conf    = make_menu_item (MenuId::ResetConf, true);

    let conf_submenu  = make_sub_menu (MenuId::Configs, true);
    let _ = conf_submenu .append_items ( &[ &version, &sep ] );
    let _ = conf_submenu .append_items ( &[ &auto_start, &auto_start_elev, &elev_launch_link, &sep ] );
    let _ = conf_submenu .append_items ( &[ &open_conf_loc, &edit_conf, &reset_conf ] );

    let restart = make_menu_item (MenuId::Restart, true);
    let exit    = make_menu_item (MenuId::Exit, true);



    // For overlays, since they are dynamic, we'll need them to be mutable, and restrict usage to events_handler
    let overlays : Vec <OverlaysSubMenuEntry> = vec![];
    let overlays_submenu = make_sub_menu (MenuId::OverlaysSubmenu, true);
    let overlays_none = make_menu_check  (MenuId::OverlaysSubNone, true, false);
    let _ = overlays_submenu.append (&overlays_none as _);

    // the same for recent-fgnd hwnds
    let fg_recents : LinkedHashMap <Hwnd, FgRecentsSubMenuEntry> = LinkedHashMap::new();
    let fg_recents_submenu = make_sub_menu (MenuId::FgRecentsSubmenu, true);
    let _ = fg_recents_submenu .append (&overlays_none as _);

    // and similar for overrides
    let overrides : Vec <OverridesSubMenuEntry> = vec![];
    let overrides_submenu = make_sub_menu (MenuId::OverridesSubmenu, true);
    let overrides_none = make_menu_check  (MenuId::OverridesSubNone, true, false);
    let _ = overrides_submenu.append (&overrides_none as _);



    // for effects, presets, mag .. the menu-items are static, and we can directly build them here
    let effects = wd.effects .cycle_order .iter().enumerate() .map ( |(i,p)| {
        let effect = ColorEffect(i);
        let id = MenuPrefix::Effect.id(i);
        let menu_item = CheckMenuItem::with_id ( id.clone(), &p.0, true, false, None);
        EffectsSubMenuEntry { id, effect, menu_item }
    } ) .collect::<Vec<_>>();

    let effects_submenu = make_sub_menu (MenuId::EffectsSubmenu, false);
    let effects_none = make_menu_check  (MenuId::EffectsSubNone, true, false);
    let _ = effects_submenu.append (&effects_none as _);
    let _ = effects_submenu.append_items (effects .iter() .map (|p| &p.menu_item as _) .collect::<Vec<_>>() .as_slice());



    let presets = wd.presets .cycle_order .iter().enumerate() .map ( |(i,p)| {
        let preset = GammaPreset(i);
        let id = MenuPrefix::Preset.id(i);
        let menu_item = CheckMenuItem::with_id ( id.clone(), &p.preset, true, false, None);
        PresetsSubMenuEntry { id, preset, menu_item }
    } ) .collect::<Vec<_>>();

    let presets_submenu = make_sub_menu (MenuId::PresetsSubmenu, true);
    let presets_none = make_menu_check  (MenuId::PresetsSubNone, true, false);
    let _ = presets_submenu.append (&presets_none as _);
    let _ = presets_submenu.append_items (presets .iter() .map (|p| &p.menu_item as _) .collect::<Vec<_>>() .as_slice());



    let mag_levels = [2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 24] .iter() .map ( |l| {
        let level = MagEffect (*l as _);
        let id = MenuPrefix::Mag.id(l);
        let disp = format! ("Level : {l:?}  ({:.2}x)", level.get());
        let menu_item = CheckMenuItem::with_id ( id.clone(), &disp, true, false, None);
        MagSubMenuEntry { id, level, menu_item }
    } ) .collect::<Vec<_>>();

    let mag_submenu = make_sub_menu (MenuId::MagSubmenu, true);
    let mag_none = make_menu_check  (MenuId::MagSubNone, true, false);
    let _ = mag_submenu.append (&mag_none as _);
    let _ = mag_submenu.append_items (mag_levels .iter() .map (|p| &p.menu_item as _) .collect::<Vec<_>>() .as_slice());



    let tray_menu = Menu::new();
    let _ = tray_menu .append_items ( &[
        &full_screen_mode, &auto_ov_enabled, &sep, &sep,
        &fg_recents_submenu, &overlays_submenu, &effects_submenu, &overrides_submenu, &sep, &sep,
        &presets_submenu, &sep, &sep,
        &mag_submenu, &sep, &sep,
        &conf_submenu, &sep,
        &restart, &exit
    ] );

    let tray = TrayIconBuilder::new()
        .with_menu (Box::new(tray_menu))
        .with_tooltip ("WinDusky")
        .with_icon (get_dusky_icon())
        .build() .unwrap();

    let (overlays_subs_dirty, fg_recents_subs_dirty, effects_subs_dirty, overrides_subs_dirty) = (
        Flag::default(), Flag::default(), Flag::default(), Flag::default()
    );

    TrayDat {
        wd, tray, full_screen_mode, auto_ov_enabled,
        overlays, fg_recents, effects, overrides, presets, mag_levels,
        overlays_submenu, fg_recents_submenu, effects_submenu, overrides_submenu, presets_submenu, mag_submenu,
        overlays_subs_dirty, fg_recents_subs_dirty, effects_subs_dirty, overrides_subs_dirty,
        overlays_none, effects_none, overrides_none, presets_none, mag_none,
        auto_start, auto_start_elev, elev_launch_link,
    }
}





// we'll set up a bunch of helper fns to handle the various actions/events

fn auto_start__update_state (
    tray: &TrayDat, elevated: bool, autostart_enabled: bool, elev_autostart_enabled: bool, elev_link_created: bool,
) {
    tray.auto_start       .set_checked (autostart_enabled);
    tray.auto_start_elev  .set_checked (elev_autostart_enabled);
    tray.elev_launch_link .set_checked (elev_link_created);

    tray.auto_start_elev  .set_enabled (elevated);
    tray.elev_launch_link .set_enabled (elevated);
    // ^^ elev parts of menu are always enabled when we're elev

    tray.auto_start .set_enabled (autostart_enabled || !elev_autostart_enabled);
    // ^^ non-elev menu is enabled only when it can be acted on (either its already enabled, or elev-task is not enabled)
}


fn eff_str (effect: Option <ColorEffect>) -> String {
    format! ("🗲(Effect: {:.50})", effect .map (|e| e.name()) .unwrap_or("None"))
}

fn overlays_menu__exec_action (id:&str, tray: &TrayDat) {
    if let Some(overlay) = tray.overlays .iter() .find (|e| e.id == id) {
        tray.wd.post_req__overlay_remove (overlay.hwnd);
    }
}

fn overlays_menu__handle_overlay_created (hwnd: Hwnd, effect: ColorEffect, tray: &mut TrayDat) {
    let exe = win_utils::get_exe_by_hwnd(hwnd) .unwrap_or_default();
    let title = win_utils::get_win_title(hwnd);
    let id = MenuPrefix::Overlay .id (hwnd.0);
    let menu_item = CheckMenuItem::with_id (&id, "", true, true, None);
    let sub_entry = OverlaysSubMenuEntry { id: id.clone(), hwnd, exe, title, effect, menu_item };

    if let Some((idx, _)) = tray.overlays .iter() .find_position (|ov| ov.id == id) {
        tray.overlays .remove (idx);
    }
    tray.overlays .push (sub_entry);
    tray.overlays .sort_by (|a,b| {
        a.exe.cmp(&b.exe) .then_with (|| a.title.cmp(&b.title))
    });
    tray.overlays_subs_dirty.set();

    update_tray__fgnd_hwnd (Some(hwnd), true);
    // ^^ will circle back and update text etc
}

fn overlays_menu__handle_overlay_cycled (hwnd: Hwnd, effect: ColorEffect, tray: &mut TrayDat) {
    if let Some(entry) = tray.overlays .iter_mut() .find (|ov| ov.hwnd == hwnd) {
        entry.effect = effect;
    }
    tray.effects_subs_dirty.set();
    tray.fg_recents_subs_dirty.set();
}

fn overlays_menu__handle_overlay_removed (hwnd: Hwnd, tray: &mut TrayDat) {
    if let Some((idx, _entry)) = tray.overlays .iter() .find_position (|ov| ov.hwnd == hwnd) {
        tray.overlays .remove (idx);
    }
    tray.overlays_subs_dirty.set();
    tray.effects_subs_dirty.set();
    tray.fg_recents_subs_dirty.set();
}

fn overlays_menu__handle_overlays_cleared (tray : &mut TrayDat) {
    tray.overlays.clear();
    tray.overlays_subs_dirty.set();
    tray.effects_subs_dirty.set();
    tray.fg_recents_subs_dirty.set();
}

fn overlays_menu__rebuild_submenu (tray: &mut TrayDat) {
    tray.overlays_subs_dirty.clear();
    for item in tray.overlays_submenu.items() {
        if let Some(item) = item.as_check_menuitem() {
            let _ = tray.overlays_submenu .remove (item);
        }
    }
    tray.overlays_none .set_checked (tray.overlays.is_empty());
    let _ = tray.overlays_submenu.append (&tray.overlays_none as _);

    let ov_hwnd = tray.wd.fgnd_ov_cache.load();
    for ov in &tray.overlays {
        let prefix = if ov.hwnd == ov_hwnd {"🗲"} else {""};
        let disp = format! ("{prefix} {} : {}", &ov.exe, &ov.title);
        let disp = if disp.len() > 60 { format!("{}..", &disp[..58]) } else { disp };
        ov.menu_item.set_text (disp);
        let _ = tray.overlays_submenu .append (&ov.menu_item as _);
    }
}





fn fg_recents_menu__exec_action (id:&str, tray: &TrayDat) {
    if let Some((_hwnd, fge)) = tray.fg_recents .iter() .find (|(_k,v)| v.id == id) {
        if tray.wd.has_overlay(&fge.hwnd) {
            tray.wd.post_req__overlay_remove (fge.hwnd);
        } else {
            // Use the cached effect if possible, else default
            let effect = tray.wd.auto .check_rule_cached (fge.hwnd) .and_then (|r| r.effect)
                .filter (|eff| !eff.is_identity()) .unwrap_or (tray.wd.effects.default);
            tray.wd.post_req__overlay_create(fge.hwnd, effect);
        }
    }
}

fn fg_recents_menu__handle_fgnd (hwnd: Option<Hwnd>, has_overlay: bool, tray: &mut TrayDat) {
    //tracing::debug! ("fgnd: {hwnd:?} .. ov:{has_overlay} .. cache-len:{:?}", tray.fg_recents.len());
    let Some(hwnd) = hwnd else {
        // hook events for window-closing etc to send these .. we'll rebuild subs to clear cruft
        tray.fg_recents_subs_dirty.set();
        return
    };
    if has_overlay {
        tray.overlays_subs_dirty.set();
        tray.effects_subs_dirty.set();
    }
    if let Some (_prior) = tray.fg_recents .get_refresh (&hwnd) {
        tray.fg_recents_subs_dirty.set();
        return
    }
    let exe = win_utils::get_exe_by_hwnd(hwnd) .unwrap_or_default();
    let title = win_utils::get_win_title(hwnd);
    let id = MenuPrefix::FgRecent .id (hwnd.0);
    let menu_item = CheckMenuItem::with_id (&id, "", true, has_overlay, None);
    let sub_entry = FgRecentsSubMenuEntry { id: id.clone(), hwnd, exe, title, menu_item };
    tray.fg_recents .insert (hwnd, sub_entry);
    tray.fg_recents_subs_dirty.set();
    while tray.fg_recents.len() >= 64 {
        tray.fg_recents .pop_front();
    }
}

fn fg_recents_menu__rebuild_submenu (tray: &mut TrayDat) {
    tray.fg_recents_subs_dirty.clear();
    for item in tray.fg_recents_submenu.items() {
        if let Some(item) = item.as_check_menuitem() {
            let _ = tray.fg_recents_submenu.remove(item);
        }
    }
    let _ = tray.fg_recents_submenu .append (&tray.overlays_none as _);
    let fg_ov = tray.wd.fgnd_ov_cache.load();
    let mut count : isize = 0;
    let mut zombies : LinkedList <Hwnd> = LinkedList::new();

    for (&hwnd, entry) in tray.fg_recents .iter_mut().rev() {
        if count >= 16 {
            break;
        }
        if !check_window_exists (hwnd) {
            zombies.push_back (hwnd);
            continue;
        }
        if !check_window_visible (hwnd) || check_window_cloaked (hwnd) {
            continue;
        }
        count += 1;
        let has_overlay = tray.wd.has_overlay(&entry.hwnd);
        entry.menu_item.set_checked (has_overlay);
        let prefix = if has_overlay && entry.hwnd == fg_ov { "🗲" } else { "" };
        let disp = format!("{prefix} {} : {}", &entry.exe, &entry.title);
        let disp = if disp.len() > 60 { format!("{}..", &disp[..58]) } else { disp };
        entry.menu_item.set_text (disp);
        let _ = tray.fg_recents_submenu .append (&entry.menu_item);
    }
    for hwnd in zombies {
        tray.fg_recents .remove (&hwnd);
    }
}





fn overrides_menu__exec_action (id:&str, tray: &TrayDat) {
    if let Some(ov) = tray.overrides .iter() .find (|e| e.id == id) {
        tray.wd.auto.remove_override (ov.hwnd);
    }
}

fn overrides_menu__handle_override_added (hwnd: Hwnd, tray: &mut TrayDat) {
    let exe = win_utils::get_exe_by_hwnd(hwnd) .unwrap_or_default();
    let title = win_utils::get_win_title(hwnd);
    let id = MenuPrefix::Override .id (hwnd.0);

    let disp = format! ("{exe} : {title}");
    let disp = if disp.len() > 60 { format!("{}..", &disp[..58]) } else { disp };
    let menu_item = CheckMenuItem::with_id (&id, &disp, true, true, None);
    let sub_entry = OverridesSubMenuEntry { id: id.clone(), hwnd, exe, title, menu_item };

    if let Some((idx, _)) = tray.overrides .iter() .find_position (|ov| ov.id == id) {
        tray.overrides .remove (idx);
    }
    tray.overrides .push (sub_entry);
    tray.overrides .sort_by (|a,b| {
        a.exe.cmp(&b.exe) .then_with (|| a.title.cmp(&b.title))
    });
    tray.overrides_subs_dirty.set();
}

fn overrides_menu__handle_override_removed (hwnd: Hwnd, tray: &mut TrayDat) {
    if let Some((idx, _)) = tray.overrides .iter() .find_position (|ov| ov.hwnd == hwnd) {
        tray.overrides .remove (idx);
    }
    tray.overrides_subs_dirty.set();
}

fn overrides_menu__handle_overrides_cleared (tray : &mut TrayDat) {
    tray.overrides .clear();
    tray.overrides_subs_dirty.set();
}

fn overrides_menu__rebuild_submenu (tray: &mut TrayDat) {
    tray.overrides_subs_dirty.clear();
    for item in tray.overrides_submenu.items() {
        if let Some(item) = item.as_check_menuitem() {
            let _ = tray.overrides_submenu .remove (item);
        }
    }
    tray.overrides_none .set_checked (tray.overrides.is_empty());
    let _ = tray.overrides_submenu.append (&tray.overrides_none as _);
    for item in tray.overrides .iter() .map (|e| &e.menu_item) {
        let _ = tray.overrides_submenu .append (item);
    }
}





fn effects_menu__exec_action (id: &str, tray: &TrayDat) {
    // note below that actual checked-state etc updates are intended to happen when the actions below cycle back w tray-updates
    if let Some(effect) = tray.effects .iter() .find (|p| p.id == id) {
        tray.wd.post_req__toggle_selected_eff (effect.effect);
    }
}

fn effects_menu__refresh (tray: &TrayDat) {
    //tracing::debug! ("refreshing effects menu ..");

    fn effects_menu__update_checkmarks (effect: Option<ColorEffect>, tray: &TrayDat) {
        tray.effects_none .set_checked (effect.is_none());
        for entry in &tray.effects {
            entry.menu_item .set_checked (Some(entry.effect) == effect);
        }
    }

    tray.effects_subs_dirty.clear();

    if tray.wd.check_fs_mode() {
        tray.effects_submenu .set_enabled (true);
        let effect = tray.wd.get_fs_eff();
        tray.effects_submenu .set_text (eff_str (effect));
        effects_menu__update_checkmarks (effect, tray);
    }
    else if tray.overlays.is_empty() {
        tray.effects_submenu .set_enabled (false);
        tray.effects_submenu .set_text (eff_str (None));
        effects_menu__update_checkmarks (None, tray);
    } else {
        let target = tray.wd.fgnd_ov_cache.load();
        if let Some (entry) = tray.overlays .iter() .find (|ov| ov.hwnd == target) {
            tray.effects_submenu .set_enabled (true);
            tray.effects_submenu .set_text (eff_str (Some (entry.effect)));
            effects_menu__update_checkmarks (Some(entry.effect), tray);
        } else {
            tray.effects_submenu .set_enabled (false);
            tray.effects_submenu .set_text (eff_str (None));
            effects_menu__update_checkmarks (None, tray);
        }
    }
}





fn presets_menu__exec_action (id: &str, tray: &TrayDat) {
    if let Some(preset) = tray.presets .iter() .find (|p| p.id == id) {
        if preset.menu_item.is_checked() {
            // ^^ note that by the time we're processing this, the checked state has already been updated for this click!
            presets_menu__update_checkmarks (None, tray);
            tray.wd.post_req__apply_gamma_preset (preset.preset);
            // ^^ this will cycle back for tray update and will update the checked state
        } else {
            // i.e. we just got unchecked
            tray.wd.post_req__toggle_gamma_preset();
        }
    }
}

fn presets_menu__handle_preset_event (
    preset: Option <GammaPreset>, applied: bool, succeeded: bool, tray: &TrayDat
) {
    let prefix = if succeeded {""} else {"❌ <- "};
    let name = preset .map (|p| p.name()) .unwrap_or ("None");
    tray.presets_submenu .set_text ( format! ("{prefix}Gamma Preset: {name:.50}") );
    presets_menu__update_checkmarks (preset.filter (|_| applied), tray);
}

fn presets_menu__update_checkmarks (preset: Option<GammaPreset>, tray: &TrayDat) {
    tray.presets_none .set_checked (preset.is_none());
    for entry in &tray.presets {
        entry.menu_item .set_checked (Some(entry.preset) == preset);
    }
}





fn mag_menu__exec_action (id: &str, tray: &TrayDat) {
    if let Some(mag) = tray.mag_levels .iter() .find (|p| p.id == id) {
        if mag.menu_item.is_checked() {
            // ^^ note that by the time we're processing this, the checked state has already been updated for this click!
            mag_menu__update_checkmarks (None, tray);
            tray.wd.post_req__apply_mag_level (mag.level);
            tray.wd.post_req__mag_refresh();
            // ^^ this will cycle back for tray update and will update the checked state
        } else {
            tray.wd.post_req__toggle_mag_level();
        }
    }
}

fn mag_menu__handle_mag_event (level: Option<MagEffect>, tray: &TrayDat) {
    let level = level .unwrap_or (*MAG_EFFECT_IDENTITY);
    tray.mag_submenu.set_text ( format! ("Magnification Level : {:?}  ({:.2}x)", level.0, level.get()) );
    mag_menu__update_checkmarks (Some(level), tray);
}

fn mag_menu__update_checkmarks (level: Option<MagEffect>, tray: &TrayDat) {
    // Note that tray menu mag levels are non-exhaustive !!
    tray.mag_none .set_checked (level.is_none_or (|l| l.is_identity()));
    for mag in &tray.mag_levels {
        mag.menu_item .set_checked (Some(mag.level) == level);
    }
}





fn exec_menu_action (id: &str, tray: &TrayDat) {
    // Reminder that since tray runs in a separate thread, direct actions to dusky host/mag hwnds cant be triggered from here!!
    // .. ie all that has to go via posting messages to dusky thread msg queue
    use MenuId::*;

    match MenuId::from_str(id) {
        Ok (m) => match m {


            FullScreenMode     => { tray.wd.post_req__toggle_fs_mode(); }
            AutoOverlayEnabled => { tray.wd.auto.toggle_auto_overlay_enabled(); }

            OverlaysSubNone   => { tray.wd.post_req__overlay_clear_all(); }
            OverridesSubNone  => { tray.wd.auto.clear_user_overrides(); }
            EffectsSubNone    => { tray.wd.post_req__toggle_color_eff (tray.wd.fgnd_ov_cache.load()); }
            PresetsSubNone    => { tray.wd.post_req__toggle_gamma_preset(); }
            MagSubNone        => { tray.wd.post_req__toggle_mag_level(); }

            AutoStart      => { autostart::proc_req__toggle_dusky_autostart (false) }
            AutoStartElev  => { autostart::proc_req__toggle_dusky_autostart (true) }
            GenElevLink    => { autostart::proc_req__toggle_elev_launch_link_gen() }

            OpenConfLoc => { tray.wd.conf.trigger_config_loc_open(); }
            EditConf    => { tray.wd.conf.trigger_config_file_edit(); }
            ResetConf   => { tray.wd.conf.trigger_config_file_reset(); }

            Restart    => { handle_restart_request(tray.wd); }
            Exit       => { tray.wd.post_req__exit(); }

            _ => { error! ("Unrecognized menu-id : {id}") }
        },

        Err (_) => {
            use MenuPrefix::*;
            if      Overlay  .pfx_matches (id) { overlays_menu__exec_action   (id, tray); }
            else if FgRecent .pfx_matches (id) { fg_recents_menu__exec_action (id, tray); }
            else if Override .pfx_matches (id) { overrides_menu__exec_action  (id, tray); }
            else if Effect   .pfx_matches (id) { effects_menu__exec_action    (id, tray); }
            else if Preset   .pfx_matches (id) { presets_menu__exec_action    (id, tray); }
            else if Mag      .pfx_matches (id) { mag_menu__exec_action        (id, tray); }
            else {
                error! ("Unrecognized menu-id : {id}")
            }
        }
    };
}



#[allow (dead_code)]
fn check_pointer_in_traymenu (tray: &TrayDat) -> bool {
    // .. but we cant track pointer loc over menu items, nor get menu rect to calc ourselves ..
    // .. so instead, we'll generally verify pointer over menu rect and pick actions based on mod-key combos
    //
    // well .. the tray.rect() gives the rect of literally only the icon .. so thats useless !! ..
    // .. either way, we mostly only get this event while tray-menu is active
    // .. (however, it can remain active even after the tray menu is gone until something else takes fgnd!) .. oh well

    let Some(rect) = tray.tray.rect() else { return false };
    let pointer = win_utils::get_pointer_loc();
    pointer.x >= rect.position.x as i32  &&
        pointer.x <= (rect.position.x as i32 + rect.size.width as i32) &&
        pointer.y >= rect.position.y as i32 &&
        pointer.y <= (rect.position.y as i32 + rect.size.height as i32)
}


fn handle_wheel_action (is_fwd: bool, tray: &TrayDat) {
    use win_utils::*;
    //if !check_pointer_in_traymenu (tray) { return }
    match (check_ctrl_modkey_active(), check_shift_modkey_active(), is_fwd) {
        ( true, false,  true) => { tray.wd.post_req__cycle_mag_level (true); }
        ( true, false, false) => { tray.wd.post_req__cycle_mag_level (false); }
        (false,  true,  true) => { tray.wd.post_req__cycle_gamma_preset (true); }
        (false,  true, false) => { tray.wd.post_req__cycle_gamma_preset (false); }
        ( true,  true,  true) => { tray.wd.post_req__cycle_color_effect (true); }
        ( true,  true, false) => { tray.wd.post_req__cycle_color_effect (false); }
        (false, false,     _) => { }
    }
}
fn handle_tray_mod_click (tray: &TrayDat) {
    use win_utils::*;
    //if !check_pointer_in_traymenu (tray) { return }
    match (check_ctrl_modkey_active(), check_shift_modkey_active()) {
        ( true, false) => { tray.wd.post_req__toggle_mag_level (); }
        (false,  true) => { tray.wd.post_req__toggle_gamma_preset (); }
        ( true,  true) => { tray.wd.post_req__toggle_color_eff (tray.wd.fgnd_ov_cache.load()); }
        (false, false) => { }
    }
}




fn handle_events (event: DuskyEvent, tray: &mut TrayDat) {

    //tracing::debug!("dusky tray-event : {event:?}");

    match event {

        DuskyEvent::MenuAction (event) => {
            exec_menu_action (&event.id.0, tray);
        }
        DuskyEvent::UpdateAutoStart { elevated, autostart_enabled, elev_autostart_enabled, elev_link_created } => {
            auto_start__update_state (tray, elevated, autostart_enabled, elev_autostart_enabled, elev_link_created);
        }
        DuskyEvent::AutoOverlayEnable (enabled) => {
            tray.auto_ov_enabled.set_checked (enabled);
            tray.auto_ov_enabled.set_text (if enabled {"Auto Overlay Enabled"} else {"Enable Auto Overlay"})
        }

        DuskyEvent::FullScreenMode { enabled, effect:_ } => {
            tray.full_screen_mode.set_checked (enabled);
            tray.full_screen_mode.set_text (if enabled {"Full Screen Effect Enabled"} else {"Enable Full Screen Effect"});
            tray.auto_ov_enabled.set_enabled (!enabled);
            tray.overlays_submenu.set_enabled (!enabled);
            tray.fg_recents_submenu.set_enabled (!enabled);
            tray.overrides_submenu.set_enabled (!enabled);
            effects_menu__refresh (tray);
        }

        DuskyEvent::OverlayCreated { hwnd, effect }  =>  { overlays_menu__handle_overlay_created (hwnd, effect, tray); }
        DuskyEvent::OverlayCycled  { hwnd, effect }  =>  { overlays_menu__handle_overlay_cycled  (hwnd, effect, tray); }
        DuskyEvent::OverlayRemoved { hwnd }          =>  { overlays_menu__handle_overlay_removed (hwnd, tray); }
        DuskyEvent::OverlaysCount  { n_active }      =>  {
            if n_active == 0 { overlays_menu__handle_overlays_cleared (tray); }
            tray.overlays_submenu.set_text (format!("Active Overlays: {n_active:?}"));
        }

        DuskyEvent::OverrideAdded   { hwnd } => { overrides_menu__handle_override_added (hwnd, tray); }
        DuskyEvent::OverrideRemoved { hwnd } => { overrides_menu__handle_override_removed (hwnd, tray); }
        DuskyEvent::OverridesCount  { n_overrides } => {
            if n_overrides == 0 { overrides_menu__handle_overrides_cleared (tray); }
            tray.overrides_submenu.set_text (format!("User Overrides: {n_overrides:?}"));
        }

        DuskyEvent::GammaState { applied, succeeded, preset } => {
            presets_menu__handle_preset_event (preset, applied, succeeded, tray);
        }
        DuskyEvent::MagLevel { level } => {
            mag_menu__handle_mag_event (level, tray);
        }

        DuskyEvent::FgndHwnd { hwnd, has_overlay }  =>  {
            fg_recents_menu__handle_fgnd (hwnd, has_overlay, tray);
        }
        DuskyEvent::RefreshMenus => {
            // we do a round-about way of setting dirty flags, then checking that during some event and sending a refresh msg
            // this avoids sitting in background updating menus unnecessarily, and only rebuilds when menu is active
            // this can however cause some flickering, which we reduce by handling the req later here than immdtly w other events
            while [
                &tray.overlays_subs_dirty, &tray.overrides_subs_dirty, &tray.fg_recents_subs_dirty, &tray.effects_subs_dirty
            ] .iter() .any (|d| d.is_set()) {
                if tray.overlays_subs_dirty   .is_set() { overlays_menu__rebuild_submenu (tray) }
                if tray.overrides_subs_dirty  .is_set() { overrides_menu__rebuild_submenu (tray) }
                if tray.fg_recents_subs_dirty .is_set() { fg_recents_menu__rebuild_submenu (tray) }
                if tray.effects_subs_dirty    .is_set() { effects_menu__refresh (tray) }
            }
        }

    }
}





pub fn start_system_tray_monitor (wd: &'static WinDusky) {

    // mostly the tray woudlnt need to be mut, but for overlays, we actively we'll mutate the submenu themselves
    let mut tray = build_tray(wd);

    // we can now setup the event-loop to monitor events from the tray and tray-menu
    let event_loop : EventLoop<DuskyEvent> = EventLoopBuilder::with_user_event().with_any_thread(true).build();

    // we'll create an event-proxy that we'll store to use from external threads, as well as to proxy menu-events to our event-loop
    let event_loop_proxy = event_loop.create_proxy();
    let _ = TRAY_EVENTS_PROXY .set (event_loop_proxy.clone());
    MenuEvent::set_event_handler ( Some ( move |event:MenuEvent| {
        let _ = event_loop_proxy.send_event (DuskyEvent::MenuAction(event));
    } ) );


    // now finally we can kick off the event loop itself .. (from which we will not return!)
    event_loop .run ( move |event, _win_target, control_flow| {

        *control_flow = ControlFlow::Wait;
        // ^^ default is Poll which isnt necessary for us

        //tracing::debug! ("loop-event : {event:?}");

        if tray.overlays_subs_dirty.is_set() || tray.overrides_subs_dirty.is_set()
            || tray.fg_recents_subs_dirty.is_set() || tray.effects_subs_dirty.is_set()
        {
            DuskyEvent::RefreshMenus.send();
            // ^^ queuing up a menu refresh rather than directly rebuilding the menus here reduces (but wont quite eliminate) flickering
        }

        unsafe { match event {

            Event::UserEvent (dusky_ev) => {
                handle_events (dusky_ev, &mut tray);
            }

            // we'll also handle mouse wheel events for cycling, and mouse btn events for toggling
            Event::DeviceEvent { event, device_id, .. } if device_id == DeviceId::dummy() => {
                // .. but apparently we get events for a dummy-device ID (generic), and separately for src mouse, hence above filtering
                match event {
                    DeviceEvent::MouseWheel { delta: MouseScrollDelta::LineDelta (dx, dy), .. } => {
                        // we'll reverse the sense of 'fwd' for mouse to wheel scroll downwards
                        let is_fwd = dx < 0.0 || dy < 0.0;
                        handle_wheel_action (is_fwd, &tray);
                    }
                    DeviceEvent::Button { button: 1, state : ElementState::Pressed, .. } => {
                        // ^^ left-button is 1u32 .. enum matching has typing issues
                        handle_tray_mod_click (&tray);
                    }
                    _ => { }
                }
            }

            _ => { }

        } }

    } )

}





// we'll setup a bunch of fns to be called from external threads to inject events into sys-tray loop for updates etc

pub fn update_tray__full_screen_mode (enabled:bool, effect: Option <ColorEffect>) {
    DuskyEvent::FullScreenMode {enabled, effect} .send();
}
pub fn update_tray__auto_overlay_enable (enabled: bool) {
    DuskyEvent::AutoOverlayEnable (enabled) .send();
}
pub fn update_tray__gamma_state (applied:bool, succeeded:bool, preset: Option <GammaPreset>) {
    DuskyEvent::GammaState { applied, succeeded, preset } .send();
}
pub fn update_tray__mag_level (level: Option <MagEffect>) {
    DuskyEvent::MagLevel {level} .send();
}

pub fn update_tray__overlay_created (hwnd: Hwnd, effect: ColorEffect) {
    DuskyEvent::OverlayCreated {hwnd, effect} .send();
}
pub fn update_tray__overlay_removed (hwnd: Hwnd) {
    DuskyEvent::OverlayRemoved {hwnd} .send();
}
pub fn update_tray__overlay_cycled (hwnd: Hwnd, effect: ColorEffect) {
    DuskyEvent::OverlayCycled {hwnd, effect} .send();
}
pub fn update_tray__overlays_count (n_active: usize) {
    DuskyEvent::OverlaysCount { n_active } .send();
}

pub fn update_tray__override_added (hwnd: Hwnd) {
    DuskyEvent::OverrideAdded { hwnd } .send();
}
pub fn update_tray__override_removed (hwnd: Hwnd) {
    DuskyEvent::OverrideRemoved { hwnd } .send();
}
pub fn update_tray__overrides_count (n_overrides: usize) {
    DuskyEvent::OverridesCount { n_overrides } .send();
}
pub fn update_tray__fgnd_hwnd (hwnd: Option<Hwnd>, has_overlay: bool) {
    DuskyEvent::FgndHwnd { hwnd, has_overlay } .send();
}

pub fn update_tray__autostart_state () {
    // note that this is on a spawned thread because checking task-sched via the planif crate seems to change thread mode !!
    thread::spawn ( move || {
        let elevated = win_utils::check_cur_proc_elevated() .unwrap_or_default();
        let autostart_enabled = autostart::check_status_auto_start_task();
        let elev_autostart_enabled = autostart::check_status_auto_start_elev_task();
        let elev_link_created = autostart::check_elev_launch_link_exists();
        DuskyEvent::UpdateAutoStart { elevated, autostart_enabled, elev_autostart_enabled, elev_link_created } .send()
    } );
}





fn handle_restart_request (wd: &'static WinDusky) {

    thread::spawn (|| {

        warn! ("Attempting to Restart WinDusky !!");

        wd.post_req__un_register_hotkeys();
        thread::sleep (Duration::from_millis(100));

        let Ok(exe) = std::env::current_exe() else {
            error! ("Failed to get current exe path for restart");
            return;
        };
        let mut cmd = Command::new (exe);
        cmd .creation_flags ((DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW).0);

        match cmd .spawn() {
            Ok (proc) => warn!  ("Launched a new WinDusky process with pid: {:?}", proc.id()),
            Err (e)   => error! ("Failed to launch new WinDusky process: {e}"),
        }

        //thread::sleep (Duration::from_millis (100));
        //std::process::exit(0);

        wd.post_req__exit();

    });

}
