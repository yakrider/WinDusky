#![ allow (non_snake_case, non_upper_case_globals) ]

use std::ops::Not;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use std::{fs, io};

use tracing::metadata::LevelFilter;
use tracing::{info, warn, Level};
use tracing_appender::non_blocking;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::time::LocalTime;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload::Handle;
use tracing_subscriber::{reload, Layer, Registry};

use windows::Win32::UI::Input::KeyboardAndMouse::HOT_KEY_MODIFIERS;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::keys::VKey;



#[derive (Debug)]
pub struct Config {
    pub toml     : RwLock <Option <DocumentMut>>,
    pub default  : DocumentMut,
    pub loglevel : RwLock <Option <Handle <LevelFilter, Registry>>>,
}



#[derive (Debug)]
pub struct HotKey {
    pub key : VKey,
    pub modifiers : Vec <VKey>,
}
impl HotKey {
    pub fn hk_mod (&self) -> HOT_KEY_MODIFIERS {
        self.modifiers .iter() .flat_map (|&k| HOT_KEY_MODIFIERS::try_from(k).ok()) .reduce (|a, e| a | e) .unwrap_or_default()
    }
}


#[derive (Debug)]
pub struct AutoOverlayExe {
    pub exe : String,
    pub effect : Option<String>,
}


#[derive (Debug)]
pub struct AutoOverlayClass {
    pub class : String,
    pub effect : Option<String>,
    pub exclusion_exes : Vec<String>,
}


#[derive (Debug)]
pub struct ColorEffectSpec {
    pub name : String,
    pub transform : [f32; 25],
}



// first some module level helper functions ..
/// Returns the directory of the currently running executable
fn get_app_dir () -> Option<PathBuf> {
    std::env::current_exe().ok() .and_then (|p| p.parent() .map (|p| p.to_path_buf()))
}

/// Checks whether a path is readonly .. note: this is not a good indicator of whether the path is writeable by a user
fn _is_writeable (path: &Path) -> bool {
    if let Ok(metadata) = fs::metadata(path) {
        metadata.permissions().readonly().not()
        // ^^ doesnt work .. its a direct read-only flag check, orthogonal to user-role based permissions
    } else { false }
}

/// Checks whether a path is writeable by the current user by attempting to open/create a file in write mode
fn is_writeable (path: &Path) -> bool {
    fs::OpenOptions::new().write(true).create(true).truncate(false).open(path).is_ok()
    // note that ^^ this is similar to 'touch' and will create an empty file if it doesnt exist
}




impl Config {

    pub fn instance () -> &'static Config {
        static INSTANCE: OnceLock <Config> = OnceLock::new();
        INSTANCE .get_or_init ( || {
            let conf = Config {
                toml    : RwLock::new (None),
                default : DocumentMut::from_str (include_str!("../WinDusky.conf.toml")).unwrap(),
                // ^^ our switche.conf.toml is at root of project, the include_str macro will load the contents at compile time
                loglevel : RwLock::new (None),
            };
            conf.load();
            conf
        } )
    }

    pub const CONF_FILE_NAME  : &'static str = "WinDusky.conf.toml";


    fn get_config_file (&self) -> Option<PathBuf> {
        let app_dir_loc = get_app_dir() .map (|p| p.join(Self::CONF_FILE_NAME));
        //println! ("app_dir_loc: {:?}", app_dir_loc);
        //win_apis::write_win_dbg_string (&format!("SWITCHE : app_dir_loc: {:?}", &app_dir_loc));
        if app_dir_loc.as_ref() .is_some_and (|p| is_writeable(p)) {
            return app_dir_loc
        }
        let app_data_dir = dirs::data_local_dir() .map (|p| p.join("WinDusky"));
        if app_data_dir .as_ref() .is_some_and (|p| !p.exists()) {
            let _ = fs::create_dir (app_data_dir.as_ref().unwrap());
        }
        let app_data_dir_loc = app_data_dir .map (|p| p.join(Self::CONF_FILE_NAME));
        //println! ("app_data_dir_loc: {:?}", app_data_dir_loc);
        //win_apis::write_win_dbg_string (&format!("WINDUSKY : app_data_dir_loc: {:?}", &app_data_dir_loc));

        if app_data_dir_loc .as_ref() .is_some_and (|p| is_writeable(p)) {
            return app_data_dir_loc
        }
        None
    }
    pub fn get_log_loc (&self) -> Option<PathBuf> {
        if let Some(conf_path) = self.get_config_file() {
            if let Some(conf_loc) = conf_path.parent() {
                return Some(conf_loc.to_path_buf())
        } }
        None
    }


    pub fn trigger_config_file_edit (&'static self) {
        if let Some(conf_path) = self.get_config_file() {
            let _ = std::process::Command::new("cmd").arg("/c").arg("start").arg(conf_path).spawn();
        }
    }
    pub fn trigger_config_file_reset (&self) {
        self.toml.write().unwrap() .replace (self.default.clone());
        self.write_back_toml();
    }


    pub fn load (&self) {
        if let Some(conf_path) = self.get_config_file().as_ref() {
            if let Ok(cfg_str) = fs::read_to_string(conf_path) {
                if !cfg_str.trim().is_empty() {
                    if let Ok(toml) = DocumentMut::from_str(&cfg_str) {
                        // successfully read and parsed a writeable non-empty toml, we'll use that
                        self.toml.write().unwrap().replace(toml);
                        return
        }   }   }  }
        // there's no writeable location, or the file was empty, or we failed to read or parse it .. load default and write back
        self.trigger_config_file_reset();
    }

    pub fn _reload_log_level (&self) {
        // we'll revisit log-subscriber setup in case there was a switch from disabled to enabled
        // (but note we still want to have the initial setup_log_subscriber called direct from main first to keep around the flush guard)
        let _ = self.setup_log_subscriber();
        let log_level = self.get_log_level();
        warn! ("Setting log-level to {:?}", log_level.into_level());
        self.loglevel.write().unwrap().as_ref() .map (|h| {
            h.modify (|f| *f = log_level)
        } );
    }

    // ^^ todo .. this is weird .. looks like it was setup so logging could be switched to enabled later, but doing so doesnt save the guard
    // .. either check to ensure all's still well, or make it such that 'disabled' simply means log-level  error, or disallow runtime changes

    #[ allow (clippy::result_unit_err) ]
    pub fn setup_log_subscriber (&self) -> Result <WorkerGuard, ()> {
        // todo .. ^^ prob use actual errors, though little utility here

        if self.check_flag__logging_enabled().not() ||  self.loglevel.read().unwrap().is_some() {
            return Err(())
        }

        if let Some(log_loc) = self.get_log_loc() {

            let log_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("WinDusky_log")
                .filename_suffix("log")
                .max_log_files(7)
                .build(log_loc)
                .map_err (|_e| ())?;

            let (nb_log_appender, guard) = non_blocking (log_appender);

            let (level_filter, filter_handle) = reload::Layer::new(self.get_log_level());

            *self.loglevel.write().unwrap() = Some(filter_handle.clone());

            let timer = LocalTime::new ( ::time::format_description::parse (
                "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"
            ).unwrap() );

            let file_subscriber = tracing_subscriber::fmt::Layer::new()
                .with_writer(nb_log_appender)
                .with_ansi(false) .with_timer(timer.clone()) .with_filter(level_filter);

            let console_subscriber = tracing_subscriber::fmt::Layer::new()
                .with_ansi(true) .with_timer(timer.clone()) //.with_filter(filter_handle)
                .with_writer ( io::stderr.with_min_level(Level::WARN) .or_else(io::stdout) );

            tracing_subscriber::registry() .with (file_subscriber) .with (console_subscriber) .init();

            return Ok(guard)
        }
        Err(())
    }


    fn write_back_toml (&self) {
        //debug! ("write_back_toml");
        let conf_path = self.get_config_file();
        if conf_path.is_none() { return }
        let _ = fs::write (
            conf_path.as_ref().unwrap(),
            self.toml.read().unwrap().as_ref() .map (|d| d.to_string()).unwrap_or_default()
        );
    }
    #[allow (dead_code)] fn write_back_toml_if_changed (&'static self) {
        let conf_path = self.get_config_file();
        if conf_path.is_none() { return }
        let toml_str = self.toml.read().unwrap().as_ref() .map (|d| d.to_string()) .unwrap_or_default();
        let old_toml_str = fs::read_to_string (conf_path.as_ref().unwrap()) .unwrap_or_default();
        if toml_str != old_toml_str {
            let _ = fs::write (conf_path.as_ref().unwrap(), toml_str);
        }
    }



    fn check_flag (&self, flag_name:&str) -> bool {
        self.toml.read().unwrap().as_ref()
            .and_then (|t| t.get(flag_name))
            .and_then (|t| t.as_bool())
            .unwrap_or (self.default.get(flag_name).unwrap().as_bool().unwrap())
    }

    fn get_float (&self, key:&str) -> f32 {
        self.toml.read().unwrap().as_ref()
            .and_then (|t| t.get(key))
            .and_then (|t| t.as_float().map(|n| n as f32))
            .unwrap_or ( self.default.get(key) .and_then (|t| t.as_float().map(|n| n as f32)) .unwrap_or_default() )
    }

    fn get_string (&self, key:&str) -> String {
        self.toml.read().unwrap().as_ref()
            .and_then (|t| t.get(key))
            .and_then (|t| t.as_str()) .map (|s| s.to_string())
            .unwrap_or ( self.default.get(key) .and_then (|t| t.as_str()) .map (|s| s.to_string()) .unwrap_or_default() )
    }

    fn get_string_array (&self, key:&str) -> Vec<String> {
        self.toml.read().unwrap() .as_ref()
            .and_then (|t| t.get(key))
            .and_then (|t| t.as_array())
            .map (|t| t.iter() .filter_map (|v| v.as_str().map(|s| s.to_string())) .collect())
            .unwrap_or (
                self.default.get(key) .and_then (|t| t.as_array())
                    .map (|t| t.iter() .filter_map (|v| v.as_str().map(|s| s.to_string())) .collect())
                    .unwrap_or_default()
            )
    }


    pub fn check_dusky_conf_version_match (&self) -> bool {
        let conf_version = self.toml.read().unwrap().as_ref().unwrap() .get("dusky_conf_version")
            .and_then (|t| t.as_float().map(|n| n as f32)) .unwrap_or_default();
        info! ("Using user conf WinDusky.conf.toml with version string : {:?}", conf_version);

        let default_conf_version = self.default.get("dusky_conf_version")
            .and_then (|t| t.as_float().map(|n| n as f32)) .unwrap_or_default();

        if conf_version != default_conf_version {
            warn! (" !!! WARNING !!! CONF VERSION MISMATCH : Expected {:?} .. Found {:?}", default_conf_version, conf_version);
            return false
        }
        true
    }


    pub fn check_flag__logging_enabled (&self) -> bool {
        self.check_flag ("logging_enabled")
    }
    pub fn get_log_level (&self) -> LevelFilter {
        if !self.check_flag__logging_enabled() {
            return LevelFilter::OFF;
        }
        match self.get_string("logging_level").as_str() {
            "TRACE" => LevelFilter::TRACE,
            "DEBUG" => LevelFilter::DEBUG,
            "WARN"  => LevelFilter::WARN,
            "ERROR" => LevelFilter::ERROR,
            "OFF"   => LevelFilter::OFF,
        //  "INFO"  => LevelFilter::INFO,
            _       => LevelFilter::INFO,
        }
    }



    fn get_hotkey (&self, conf_key:&str) -> Option<HotKey> {
        if let Some(toml) = self.toml.read().unwrap() .as_ref() {
            if let Some(key) = {
                toml .get(conf_key) .and_then (|t| t.get("key")) .and_then (|k| k.as_str()) .and_then (|s| VKey::from_str(s).ok())
            } {
                let modifiers = toml .get(conf_key) .and_then (|t| t.get("modifiers"))
                    .and_then (|ms| ms.as_array())
                    .map (|t| t.iter() .filter_map (|m| m.as_str() .and_then (|s| VKey::from_str(s).ok())) .collect())
                    .unwrap_or_default();
                return Some ( HotKey {key, modifiers} )
            }
        }
        None
    }

    pub fn get_hotkey__dusky_toggle (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__dusky_toggle") }

    pub fn get_hotkey__fullscreen_toggle (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__fullscreeen_toggle") }

    pub fn get_hotkey__next_effect  (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__next_effect") }
    pub fn get_hotkey__prev_effect  (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__prev_effect") }

    pub fn get_hotkey__clear_overlays  (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__clear_overlays") }
    pub fn get_hotkey__clear_overrides (&self)  -> Option<HotKey> { self.get_hotkey ("hotkey__clear_overrides") }




    pub fn get_auto_overlay_luminance__threshold (&self) -> u8 {
        let lum_fl = self.get_float ("auto_overlay_luminance__threshold");
        (u8::MAX as f32 * lum_fl.clamp(0.0, 1.0)) as u8
    }

    pub fn get_auto_overlay_luminance__delay_ms (&self) -> u32 {
        self.get_float ("auto_overlay_luminance__delay_ms") as u32
    }

    pub fn get_auto_overlay_luminance__exclusion_exes (&self) -> Vec<String> {
        self.get_string_array ("auto_overlay_luminance__exclusion_exes")
    }

    pub fn get_auto_overlay_luminance__use_alternate (&self) -> bool {
        self.check_flag ("auto_overlay_luminance__use_alternate_method")
    }


    fn parse_auto_overlay_exe (v : &Value) -> Option <AutoOverlayExe> {
        if let Some(entry) = v .as_inline_table() {
            if let Some(exe) = entry .get("exe") .and_then (|s| s.as_str() .map (|s| s.to_string())) {
                let effect = entry .get("effect") .and_then (|s| s.as_str() .map (|s| s.to_string())) .filter (|eff| eff != "default");
                let result = AutoOverlayExe {exe, effect};
                //tracing::debug! ("parsed auto-overlay-exe entry: {:?}", &result);
                return Some ( result )
            }
        }
        None
    }
    pub fn get_auto_overlay_exes (&self) -> Vec<AutoOverlayExe> {
        if let Some(toml) = self.toml.read().unwrap().as_ref() {
            return toml .get ("auto_overlay_exes") .and_then (|t| t.as_array())
                .map (|t| t.iter() .filter_map (Self::parse_auto_overlay_exe) .collect())
                .unwrap_or_default()
        }
        vec![]
    }


    fn parse_auto_overlay_window_class (v : &Value) -> Option <AutoOverlayClass> {
        if let Some(entry) = v .as_inline_table() {
            if let Some(class) = entry .get("class_name") .and_then (|s| s.as_str() .map (|s| s.to_string())) {
                let effect = entry .get("effect")
                    .and_then (|s| s.as_str() .map (|s| s.to_string()))
                    .filter (|eff| eff != "default");
                let exclusion_exes = entry .get("exclusion_exes")
                    .and_then (|s| s.as_array())
                    .map (|a| a.iter() .filter_map (|s| s.as_str().map(|s| s.to_string())) .collect::<Vec<_>>())
                    .unwrap_or_default();
                let result = AutoOverlayClass { class, effect, exclusion_exes };
                //tracing::debug! ("parsed auto-overlay-class entry: {:?}", &result);
                return Some (result)
            }
        }
        None
    }
    pub fn get_auto_overlay_window_classes (&self) -> Vec<AutoOverlayClass> {
        if let Some(toml) = self.toml.read().unwrap().as_ref() {
            return toml .get ("auto_overlay_window_classes") .and_then (|t| t.as_array())
                .map (|t| t.iter() .filter_map (Self::parse_auto_overlay_window_class) .collect())
                .unwrap_or_default()
        }
        vec![]
    }



    pub fn parse_color_effect (table : &Table) -> Option <ColorEffectSpec> {
        if let Some (name) = table .get("effect") .and_then (|s| s.as_str() .map (|s| s.to_string())) {
            if let Some (Item::Value (Value::Array(arr))) = table .get("transform") {
                let matrix: Vec<f32> = arr.iter() .filter_map (|v|
                    v.as_float() .or_else (|| v.as_integer() .map (|i| i as f64)) .map (|f| f as f32)
                ) .collect();
                if matrix.len() == 25 {
                    let mut transform = [0.0f32; 25];
                    transform.copy_from_slice (&matrix);
                    return Some ( ColorEffectSpec { name, transform } )
                }
            }
        }
        None
    }
    pub fn get_color_effects (&self) -> Vec<ColorEffectSpec> {
        if let Some(toml) = self.toml.read().unwrap().as_ref() {
            return toml .get ("effects") .and_then (|t| t.as_array_of_tables())
                .map (|t| t.iter() .filter_map (Self::parse_color_effect) .collect())
                .unwrap_or_default()
        }
        vec![]
    }


    pub fn get_effects_cycle_order (&self) -> Vec<String> {
        self.get_string_array ("effects_cycle_order")
    }

    pub fn get_effects_default (&self) -> String {
        self.get_string ("effects_default")
    }



}
