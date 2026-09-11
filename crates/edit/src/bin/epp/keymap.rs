// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Keybindings, including multi-key chords.
//!
//! The editor used to compare keys against a hardcoded `if` chain. Routing them
//! through a table instead buys three things at once: users can rebind anything
//! from `settings.json`, plugins can claim keys without patching the editor, and
//! a binding can span more than one keypress, which is what makes a leader key
//! and a which-key style hint possible.
//!
//! ## Binding syntax
//!
//! Both common spellings parse, and they can be mixed:
//!
//! ```text
//! "ctrl+shift+p"      VS Code style
//! "<C-S-p>"           vim style, same binding
//! "ctrl+k ctrl+s"     a two-key chord, space separated
//! "<leader>ff"        leader, then f, then f
//! "f3"                a named key
//! ```
//!
//! ## Resolution
//!
//! An exact match wins immediately, even when a longer binding shares it as a
//! prefix. There is no timeout, so a chord never leaves the editor waiting on a
//! key that may never come; the cost is that binding both `<leader>f` and
//! `<leader>ff` makes the latter unreachable.

use std::collections::HashMap;
use std::fmt::Write as _;

use edit::input::{InputKey, InputKeyMod, kbmod, vk};

use crate::commands::{Builtin, CommandId, Commands};
use crate::localization::*;
use crate::settings::Settings;

/// The default leader. `Ctrl+K` rather than `Space`, because this is a modeless
/// editor where `Space` has to keep inserting a space.
pub const DEFAULT_LEADER: InputKey = InputKey::new(kbmod::CTRL.value() | 'K' as u32);

/// What a keypress meant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    /// No binding involves this key. The caller should handle it normally.
    Unhandled,
    /// The key started or extended a chord. It was consumed; more is expected.
    Pending,
    /// The key did not extend the chord in progress, so the chord was dropped.
    /// The key is still consumed, to keep a mistyped chord from being typed
    /// into the document.
    Cancelled,
    /// A binding completed.
    Run(CommandId),
}

struct Bind {
    keys: Vec<InputKey>,
    command: CommandId,
}

pub struct Keymap {
    binds: Vec<Bind>,
    /// Keys of a chord seen so far. Empty when no chord is in progress.
    pending: Vec<InputKey>,
    leader: InputKey,
}

impl Keymap {
    pub fn new() -> Self {
        let mut this = Self { binds: Vec::new(), pending: Vec::new(), leader: DEFAULT_LEADER };
        this.load_defaults();
        this
    }

    /// Rebuilds the keymap from the builtin defaults plus `settings.json`.
    /// Returns one message per binding that could not be applied.
    pub fn reload(&mut self, commands: &Commands) -> Vec<String> {
        self.binds.clear();
        self.pending.clear();
        self.leader = DEFAULT_LEADER;

        let settings = Settings::borrow();

        if let Some(spec) = &settings.leader {
            match parse_chord(spec, DEFAULT_LEADER) {
                Ok(keys) if keys.len() == 1 => self.leader = keys[0],
                Ok(_) => return vec![format!("keyboard.leader: \"{spec}\" must be a single key")],
                Err(err) => return vec![format!("keyboard.leader: {err}")],
            }
        }

        self.load_defaults();

        let mut errors = Vec::new();

        for (spec, command) in &settings.keybindings {
            let keys = match parse_chord(spec, self.leader) {
                Ok(keys) => keys,
                Err(err) => {
                    errors.push(format!("keyboard.bindings[\"{spec}\"]: {err}"));
                    continue;
                }
            };

            let Some(name) = command else {
                self.unbind(&keys);
                continue;
            };

            match commands.id_of(name) {
                Some(id) => self.bind(keys, id),
                None => {
                    errors.push(format!("keyboard.bindings[\"{spec}\"]: unknown command {name}"))
                }
            }
        }

        errors
    }

    fn load_defaults(&mut self) {
        let mut bind = |key: InputKey, builtin: Builtin| {
            self.binds.push(Bind { keys: vec![key], command: builtin.id() });
        };

        bind(kbmod::CTRL | vk::N, Builtin::FileNew);
        bind(kbmod::CTRL | vk::O, Builtin::FileOpen);
        bind(kbmod::CTRL | vk::S, Builtin::FileSave);
        bind(kbmod::CTRL_SHIFT | vk::S, Builtin::FileSaveAs);
        bind(kbmod::CTRL | vk::W, Builtin::FileClose);
        bind(kbmod::CTRL | vk::Q, Builtin::FileExit);

        // The focused text area handles these itself as it renders, so these
        // entries mostly exist so the palette and the menus can state the
        // truth about what is bound. Rebinding them moves the command but does
        // not take the key away from the text area.
        bind(kbmod::CTRL | vk::Z, Builtin::EditUndo);
        bind(kbmod::CTRL | vk::Y, Builtin::EditRedo);
        bind(kbmod::CTRL | vk::X, Builtin::EditCut);
        bind(kbmod::CTRL | vk::C, Builtin::EditCopy);
        bind(kbmod::CTRL | vk::V, Builtin::EditPaste);
        bind(kbmod::CTRL | vk::A, Builtin::EditSelectAll);
        bind(kbmod::ALT | vk::Z, Builtin::ViewToggleWordWrap);

        bind(kbmod::CTRL | vk::F, Builtin::EditFind);
        bind(kbmod::CTRL | vk::R, Builtin::EditReplace);
        bind(vk::F3, Builtin::EditFindNext);

        bind(kbmod::CTRL | vk::P, Builtin::ViewGoToFile);
        bind(kbmod::CTRL | vk::G, Builtin::ViewGoToLine);
        bind(kbmod::CTRL_SHIFT | vk::P, Builtin::ViewCommandPalette);

        // Chords under the leader. These are new; the single-key bindings above
        // are what the editor already shipped and are left exactly as they were.
        let leader = self.leader;
        let mut chord = |rest: &[InputKey], builtin: Builtin| {
            let mut keys = Vec::with_capacity(rest.len() + 1);
            keys.push(leader);
            keys.extend_from_slice(rest);
            self.binds.push(Bind { keys, command: builtin.id() });
        };

        chord(&[leader], Builtin::ViewCommandPalette);
        chord(&[vk::F, vk::F], Builtin::ViewGoToFile);
        chord(&[vk::F, vk::N], Builtin::FileNew);
        chord(&[vk::F, vk::S], Builtin::FileSave);
        chord(&[vk::F, vk::P], Builtin::FilePreferences);
        chord(&[vk::U, vk::W], Builtin::ViewToggleWordWrap);
        chord(&[vk::C, vk::R], Builtin::ConfigReload);
        chord(&[vk::H, vk::A], Builtin::HelpAbout);
    }

    /// Adds a binding, replacing any existing binding on the same key sequence.
    pub fn bind(&mut self, keys: Vec<InputKey>, command: CommandId) {
        self.unbind(&keys);
        self.binds.push(Bind { keys, command });
    }

    pub fn unbind(&mut self, keys: &[InputKey]) {
        self.binds.retain(|b| b.keys != keys);
    }

    /// Whether a chord is in progress, meaning the next key belongs to the
    /// keymap and must not reach the document.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn pending(&self) -> &[InputKey] {
        &self.pending
    }

    pub fn leader(&self) -> InputKey {
        self.leader
    }

    pub fn resolve(&mut self, key: InputKey) -> Resolution {
        if self.has_pending() && key.key() == vk::ESCAPE {
            self.pending.clear();
            return Resolution::Cancelled;
        }

        self.pending.push(key);

        let mut exact = None;
        let mut has_longer = false;

        for b in &self.binds {
            if b.keys.len() == self.pending.len() {
                if b.keys == self.pending {
                    // Last registration wins, so user and plugin bindings
                    // override the defaults loaded before them.
                    exact = Some(b.command);
                }
            } else if b.keys.len() > self.pending.len() && b.keys.starts_with(&self.pending) {
                has_longer = true;
            }
        }

        if let Some(command) = exact {
            self.pending.clear();
            return Resolution::Run(command);
        }
        if has_longer {
            return Resolution::Pending;
        }

        let was_chord = self.pending.len() > 1;
        self.pending.clear();
        if was_chord { Resolution::Cancelled } else { Resolution::Unhandled }
    }

    /// The binding to advertise for a command in the menubar, which has room
    /// for a single key only. Chord-only commands get [`vk::NULL`].
    pub fn shortcut_for(&self, command: CommandId) -> InputKey {
        self.binds
            .iter()
            .find(|b| b.command == command && b.keys.len() == 1)
            .map_or(vk::NULL, |b| b.keys[0])
    }

    /// The bindings that would continue the chord in progress, as
    /// (remaining keys, command), for a which-key style hint.
    pub fn continuations(&self) -> Vec<(Vec<InputKey>, CommandId)> {
        let mut out: Vec<_> = self
            .binds
            .iter()
            .filter(|b| b.keys.len() > self.pending.len() && b.keys.starts_with(&self.pending))
            .map(|b| (b.keys[self.pending.len()..].to_vec(), b.command))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.dedup_by(|a, b| a.0 == b.0);
        out
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders a key sequence the way the binding syntax spells it.
pub fn format_keys(keys: &[InputKey]) -> String {
    let mut out = String::new();
    for (i, &key) in keys.iter().enumerate() {
        if i != 0 {
            out.push(' ');
        }
        format_key_into(&mut out, key);
    }
    out
}

fn format_key_into(out: &mut String, key: InputKey) {
    if key.modifiers_contains(kbmod::CTRL) {
        let _ = write!(out, "{}+", loc(LocId::Ctrl));
    }
    if key.modifiers_contains(kbmod::ALT) {
        let _ = write!(out, "{}+", loc(LocId::Alt));
    }
    if key.modifiers_contains(kbmod::SHIFT) {
        let _ = write!(out, "{}+", loc(LocId::Shift));
    }

    let bare = key.key();
    if let Some(name) = NAMED_KEYS.iter().find(|(_, k)| k.value() == bare.value()) {
        out.push_str(name.0);
    } else {
        let ch = bare.value();
        match char::from_u32(ch) {
            // Letters are written lowercase, because the parser reads an
            // uppercase letter as implying Shift. Spelling Shift out above and
            // keeping the letter lowercase is the only form that survives a
            // round trip through `parse_chord` unchanged.
            Some(c) if c.is_ascii_graphic() => out.push(c.to_ascii_lowercase()),
            _ => {
                let _ = write!(out, "0x{ch:02X}");
            }
        }
    }
}

/// Key names accepted in bindings and used when rendering them back out. The
/// first spelling of each key is the canonical one; later ones are aliases.
static NAMED_KEYS: &[(&str, InputKey)] = &[
    ("Space", vk::SPACE),
    ("Tab", vk::TAB),
    ("Enter", vk::RETURN),
    ("Esc", vk::ESCAPE),
    ("Backspace", vk::BACK),
    ("Delete", vk::DELETE),
    ("Insert", vk::INSERT),
    ("Home", vk::HOME),
    ("End", vk::END),
    ("PageUp", vk::PRIOR),
    ("PageDown", vk::NEXT),
    ("Left", vk::LEFT),
    ("Right", vk::RIGHT),
    ("Up", vk::UP),
    ("Down", vk::DOWN),
    ("F1", vk::F1),
    ("F2", vk::F2),
    ("F3", vk::F3),
    ("F4", vk::F4),
    ("F5", vk::F5),
    ("F6", vk::F6),
    ("F7", vk::F7),
    ("F8", vk::F8),
    ("F9", vk::F9),
    ("F10", vk::F10),
    ("F11", vk::F11),
    ("F12", vk::F12),
    // Aliases.
    ("Return", vk::RETURN),
    ("CR", vk::RETURN),
    ("Escape", vk::ESCAPE),
    ("BS", vk::BACK),
    ("Del", vk::DELETE),
    ("Ins", vk::INSERT),
    ("PgUp", vk::PRIOR),
    ("PgDn", vk::NEXT),
];

fn named_key(name: &str) -> Option<InputKey> {
    NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, k)| k)
}

fn modifier(name: &str) -> Option<InputKeyMod> {
    Some(match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "c" => kbmod::CTRL,
        "alt" | "meta" | "a" | "m" => kbmod::ALT,
        "shift" | "s" => kbmod::SHIFT,
        _ => return None,
    })
}

/// Parses a binding into the key sequence it stands for.
///
/// `<leader>` expands to `leader`, which is why this needs to be told what the
/// leader currently is rather than reading it back out of the keymap.
pub fn parse_chord(spec: &str, leader: InputKey) -> Result<Vec<InputKey>, String> {
    let mut keys = Vec::new();
    let mut rest = spec;

    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix(' ') {
            rest = tail;
            continue;
        }

        if let Some(tail) = rest.strip_prefix('<') {
            let Some(end) = tail.find('>') else {
                return Err("unterminated '<'".to_string());
            };
            let token = &tail[..end];
            rest = &tail[end + 1..];

            if token.eq_ignore_ascii_case("leader") {
                keys.push(leader);
            } else {
                keys.push(parse_key(token)?);
            }
            continue;
        }

        // A run of plain characters. If it reads as one key spec, it is one key;
        // otherwise each character is its own key, so "<leader>ff" works.
        let run_end = rest.find([' ', '<']).unwrap_or(rest.len());
        let run = &rest[..run_end];
        rest = &rest[run_end..];

        if run.len() > 1 && (run.contains('+') || named_key(run).is_some()) {
            keys.push(parse_key(run)?);
        } else {
            for ch in run.chars() {
                keys.push(parse_key(&ch.to_string())?);
            }
        }
    }

    if keys.is_empty() {
        return Err("empty binding".to_string());
    }
    Ok(keys)
}

/// Parses a single `mod+mod+key` (or `mod-mod-key`) spec.
fn parse_key(spec: &str) -> Result<InputKey, String> {
    let sep = if spec.contains('+') { '+' } else { '-' };
    let mut parts: Vec<&str> = spec.split(sep).collect();

    // A bare "+" or "-" is the key itself, not a separator, and splitting it
    // leaves empty parts behind.
    if parts.iter().all(|p| p.is_empty()) {
        parts = vec![spec];
    }

    let Some(key_name) = parts.pop().filter(|p| !p.is_empty()) else {
        return Err(format!("\"{spec}\" has no key"));
    };

    let mut mods = kbmod::NONE;
    for part in parts {
        match modifier(part) {
            Some(m) => mods |= m,
            None => return Err(format!("\"{part}\" is not a modifier")),
        }
    }

    let key = if let Some(key) = named_key(key_name) {
        key
    } else if key_name.chars().count() == 1 {
        let ch = key_name.chars().next().unwrap();
        // Uppercase letters imply Shift, which is how the input parser reports
        // them. Fold that into the modifiers so "<C-S-p>" and "ctrl+P" agree.
        let ch_upper = ch.to_ascii_uppercase();
        match InputKey::from_ascii(ch_upper) {
            Some(key) => {
                if ch.is_ascii_uppercase() && key.modifiers_contains(kbmod::SHIFT) {
                    mods |= kbmod::SHIFT;
                }
                key.key()
            }
            None => {
                return Err(format!(
                    "\"{ch}\" cannot be bound; only letters, digits, space and named keys can be"
                ));
            }
        }
    } else {
        return Err(format!("\"{key_name}\" is not a known key"));
    };

    Ok(key.with_modifiers(mods))
}

/// Maps every command that has a binding to its shortest one, for the palette.
#[allow(dead_code, reason = "used by the command palette")]
pub fn shortcuts_by_command(keymap: &Keymap) -> HashMap<CommandId, Vec<InputKey>> {
    let mut out: HashMap<CommandId, Vec<InputKey>> = HashMap::new();
    for b in &keymap.binds {
        out.entry(b.command)
            .and_modify(|best| {
                // `<=` so that on a tie the later registration wins, which is
                // the user's binding rather than the default it replaced.
                if b.keys.len() <= best.len() {
                    *best = b.keys.clone();
                }
            })
            .or_insert_with(|| b.keys.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(spec: &str) -> Vec<InputKey> {
        parse_chord(spec, DEFAULT_LEADER).unwrap()
    }

    #[test]
    fn vscode_and_vim_spellings_agree() {
        assert_eq!(parse("ctrl+shift+p"), parse("<C-S-p>"));
        assert_eq!(parse("ctrl+p"), vec![kbmod::CTRL | vk::P]);
        assert_eq!(parse("alt+z"), vec![kbmod::ALT | vk::Z]);
    }

    #[test]
    fn uppercase_letter_implies_shift() {
        assert_eq!(parse("ctrl+P"), parse("ctrl+shift+p"));
    }

    #[test]
    fn named_keys_beat_character_runs() {
        assert_eq!(parse("f3"), vec![vk::F3]);
        assert_eq!(parse("ff"), vec![vk::F, vk::F]);
        assert_eq!(parse("escape"), vec![vk::ESCAPE]);
    }

    #[test]
    fn chords_split_on_spaces_and_leader() {
        assert_eq!(parse("ctrl+k ctrl+s"), vec![kbmod::CTRL | vk::K, kbmod::CTRL | vk::S]);
        assert_eq!(parse("<leader>ff"), vec![DEFAULT_LEADER, vk::F, vk::F]);
    }

    #[test]
    fn unbindable_keys_are_rejected_not_silently_dropped() {
        assert!(parse_chord("ctrl+/", DEFAULT_LEADER).is_err());
        assert!(parse_chord("<C-", DEFAULT_LEADER).is_err());
        assert!(parse_chord("hyper+x", DEFAULT_LEADER).is_err());
        assert!(parse_chord("", DEFAULT_LEADER).is_err());
    }

    #[test]
    fn round_trips_through_the_formatter() {
        for spec in ["ctrl+p", "ctrl+k ctrl+s", "f3", "alt+shift+z"] {
            let keys = parse(spec);
            assert_eq!(parse(&format_keys(&keys)), keys, "{spec}");
        }
    }

    #[test]
    fn chord_resolution_waits_then_runs() {
        let mut km = Keymap::new();
        km.binds.clear();
        km.bind(parse("ctrl+k ctrl+s"), Builtin::FileSave.id());

        assert_eq!(km.resolve(kbmod::CTRL | vk::K), Resolution::Pending);
        assert!(km.has_pending());
        assert_eq!(km.resolve(kbmod::CTRL | vk::S), Resolution::Run(Builtin::FileSave.id()));
        assert!(!km.has_pending());
    }

    #[test]
    fn a_broken_chord_is_swallowed_not_typed() {
        let mut km = Keymap::new();
        km.binds.clear();
        km.bind(parse("ctrl+k ctrl+s"), Builtin::FileSave.id());

        assert_eq!(km.resolve(kbmod::CTRL | vk::K), Resolution::Pending);
        assert_eq!(km.resolve(vk::X), Resolution::Cancelled);
        assert!(!km.has_pending());
        // An unrelated key with no chord in progress must stay unhandled, or
        // typing would stop working.
        assert_eq!(km.resolve(vk::X), Resolution::Unhandled);
    }

    #[test]
    fn escape_cancels_a_chord() {
        let mut km = Keymap::new();
        assert_eq!(km.resolve(DEFAULT_LEADER), Resolution::Pending);
        assert_eq!(km.resolve(vk::ESCAPE), Resolution::Cancelled);
        assert!(!km.has_pending());
    }

    #[test]
    fn later_bindings_win() {
        let mut km = Keymap::new();
        km.binds.clear();
        km.bind(parse("ctrl+s"), Builtin::FileSave.id());
        km.bind(parse("ctrl+s"), Builtin::FileNew.id());
        assert_eq!(km.resolve(kbmod::CTRL | vk::S), Resolution::Run(Builtin::FileNew.id()));
    }

    #[test]
    fn defaults_cover_the_previously_hardcoded_shortcuts() {
        let km = Keymap::new();
        for (spec, builtin) in [
            ("ctrl+n", Builtin::FileNew),
            ("ctrl+o", Builtin::FileOpen),
            ("ctrl+s", Builtin::FileSave),
            ("ctrl+shift+s", Builtin::FileSaveAs),
            ("ctrl+w", Builtin::FileClose),
            ("ctrl+q", Builtin::FileExit),
            ("ctrl+p", Builtin::ViewGoToFile),
            ("ctrl+g", Builtin::ViewGoToLine),
            ("ctrl+f", Builtin::EditFind),
            ("ctrl+r", Builtin::EditReplace),
            ("f3", Builtin::EditFindNext),
        ] {
            let keys = parse(spec);
            assert_eq!(km.shortcut_for(builtin.id()), keys[0], "{spec}");
        }
    }
}
