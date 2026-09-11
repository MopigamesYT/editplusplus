use std::path::PathBuf;

use edit::buffer::TextBuffer;
use edit::cell::{Ref, SemiRefCell};
use edit::json;
use edit::lsh::{LANGUAGES, Language};
use stdext::arena::{read_to_string, scratch_arena};
use stdext::arena_format;

use crate::apperr;

pub struct Settings {
    pub path: PathBuf,
    pub file_associations: Vec<(String, &'static Language)>,
    /// The leader key spec, if the user set one. Parsed by the keymap, which
    /// owns the binding syntax.
    pub leader: Option<String>,
    /// User keybindings as (spec, command name). A `None` command means the
    /// user explicitly unbound that key.
    pub keybindings: Vec<(String, Option<String>)>,
}

struct SettingsCell(SemiRefCell<Settings>);
unsafe impl Sync for SettingsCell {}
static SETTINGS: SettingsCell = SettingsCell(SemiRefCell::new(Settings::new()));

impl Settings {
    /// Fills the given settings.json text buffer with some initial contents for convenience.
    pub fn bootstrap(tb: &mut TextBuffer) {
        tb.set_crlf(false);
        tb.write_raw(b"{\n}\n");
        tb.cursor_move_to_logical(Default::default());
        tb.mark_as_clean();
    }

    const fn new() -> Self {
        Settings {
            path: PathBuf::new(),
            file_associations: Vec::new(),
            leader: None,
            keybindings: Vec::new(),
        }
    }

    pub fn borrow() -> Ref<'static, Settings> {
        SETTINGS.0.borrow()
    }

    pub fn reload() -> apperr::Result<()> {
        let s = &mut *SETTINGS.0.borrow_mut();

        // Reset all members if we had been loaded previously.
        if !s.path.as_os_str().is_empty() {
            *s = Settings::new();
        }

        s.load()
    }

    fn load(&mut self) -> apperr::Result<()> {
        self.path = match settings_json_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        let scratch = scratch_arena(None);
        let str = match read_to_string(&scratch, &self.path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
            Ok(str) => str,
        };
        let Ok(json) = json::parse(&scratch, &str) else {
            return Err(apperr::Error::SettingsInvalid("Invalid JSON"));
        };
        let Some(root) = json.as_object() else {
            return Err(apperr::Error::SettingsInvalid("Non-object root"));
        };

        if let Some(f) = root.get_object("files.associations") {
            for &(mut key, ref value) in f.iter() {
                if !key.contains('/') {
                    key = arena_format!(&*scratch, "**/{key}").leak();
                }

                let Some(id) = value.as_str() else {
                    return Err(apperr::Error::SettingsInvalid("files.associations"));
                };
                let Some(language) = LANGUAGES.iter().find(|lang| lang.id == id) else {
                    return Err(apperr::Error::SettingsInvalid("language ID"));
                };

                self.file_associations.push((key.to_string(), language));
            }
        }

        if let Some(leader) = root.get_str("keyboard.leader") {
            self.leader = Some(leader.to_string());
        }

        if let Some(bindings) = root.get_object("keyboard.bindings") {
            for &(key, ref value) in bindings.iter() {
                // `null` and `false` both mean "unbind this", so that a user can
                // drop a default binding without knowing what it was bound to.
                let command = if value.is_null() || value.as_bool() == Some(false) {
                    None
                } else if let Some(name) = value.as_str() {
                    Some(name.to_string())
                } else {
                    return Err(apperr::Error::SettingsInvalid("keyboard.bindings"));
                };

                self.keybindings.push((key.to_string(), command));
            }
        }

        Ok(())
    }
}

fn settings_json_path() -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push("settings.json");

    // edit++ keeps its own config directory, but someone arriving from
    // Microsoft Edit should not silently lose their settings. If this fork has
    // no settings file yet and the upstream one does, read theirs.
    if !path.exists()
        && let Some(mut legacy) = legacy_config_dir()
    {
        legacy.push("settings.json");
        if legacy.exists() {
            return Some(legacy);
        }
    }

    Some(path)
}

/// Where plugins live. Each plugin is a directory or a `.lua` file under here.
#[allow(dead_code, reason = "used by the plugin host")]
pub fn plugin_dir() -> Option<PathBuf> {
    let mut dir = config_dir()?;
    dir.push("plugins");
    Some(dir)
}

fn var_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn push(mut path: PathBuf, suffix: &str) -> PathBuf {
    path.push(suffix);
    path
}

fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        var_path("APPDATA").map(|p| push(p, "edit++"))
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        var_path("HOME").map(|p| push(p, "Library/Application Support/edit++"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios")))]
    {
        var_path("XDG_CONFIG_HOME")
            .or_else(|| var_path("HOME").map(|p| push(p, ".config")))
            .map(|p| push(p, "epp"))
    }
}

/// Microsoft Edit's config directory, read from only as a fallback.
fn legacy_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        var_path("APPDATA").map(|p| push(p, "Microsoft\\Edit"))
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        var_path("HOME").map(|p| push(p, "Library/Application Support/com.microsoft.edit"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios")))]
    {
        var_path("XDG_CONFIG_HOME")
            .or_else(|| var_path("HOME").map(|p| push(p, ".config")))
            .map(|p| push(p, "msedit"))
    }
}
