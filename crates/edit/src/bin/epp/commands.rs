// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! The command registry.
//!
//! Every user-triggerable action in the editor is a named command. Menus,
//! keybindings, and plugins all reach the editor through this one table.
//! That indirection is what makes a discoverable keymap and a command palette
//! possible at all: without it, "what can I bind?" has no answer.
//!
//! Builtin commands are a closed enum dispatched by [`exec`]. Plugins register
//! additional commands at runtime via [`Commands::register`], which hands back
//! a [`CommandId`] that is indistinguishable from a builtin one at the call site.

use std::collections::HashMap;

use edit::tui::Context;

use crate::draw_editor::{SearchAction, search_execute};
use crate::localization::*;
use crate::settings::Settings;
use crate::state::*;

/// A handle to a registered command, stable for the lifetime of the registry.
///
/// For builtins this is `Builtin as u32`, which lets [`Builtin::id`] be free.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CommandId(u32);

/// The set of commands the editor itself implements.
///
/// Discriminants are load-bearing: they double as [`CommandId`] values, so
/// entries may be appended but not reordered. `ALL` must stay in sync, which
/// `Commands::new` asserts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Builtin {
    FileNew,
    FileOpen,
    FileSave,
    FileSaveAs,
    FilePreferences,
    FileClose,
    FileExit,

    EditUndo,
    EditRedo,
    EditCut,
    EditCopy,
    EditPaste,
    EditFind,
    EditReplace,
    EditFindNext,
    EditSelectAll,

    ViewFocusStatusbar,
    ViewGoToFile,
    ViewGoToLine,
    ViewToggleWordWrap,
    ViewCommandPalette,

    HelpAbout,

    ConfigReload,
}

/// Static metadata for a [`Builtin`].
pub struct Spec {
    /// The stable, dotted identifier used in config files and by plugins.
    pub name: &'static str,
    /// The human-readable, localized title shown in menus and the palette.
    pub title: LocId,
    /// Whether the command is meaningless without an open document.
    pub needs_document: bool,
}

impl Builtin {
    pub const ALL: &'static [Builtin] = &[
        Builtin::FileNew,
        Builtin::FileOpen,
        Builtin::FileSave,
        Builtin::FileSaveAs,
        Builtin::FilePreferences,
        Builtin::FileClose,
        Builtin::FileExit,
        Builtin::EditUndo,
        Builtin::EditRedo,
        Builtin::EditCut,
        Builtin::EditCopy,
        Builtin::EditPaste,
        Builtin::EditFind,
        Builtin::EditReplace,
        Builtin::EditFindNext,
        Builtin::EditSelectAll,
        Builtin::ViewFocusStatusbar,
        Builtin::ViewGoToFile,
        Builtin::ViewGoToLine,
        Builtin::ViewToggleWordWrap,
        Builtin::ViewCommandPalette,
        Builtin::HelpAbout,
        Builtin::ConfigReload,
    ];

    /// The registry handle for this builtin. See the note on the enum.
    pub const fn id(self) -> CommandId {
        CommandId(self as u32)
    }

    pub const fn spec(self) -> Spec {
        macro_rules! spec {
            ($name:literal, $title:expr) => {
                Spec { name: $name, title: $title, needs_document: false }
            };
            ($name:literal, $title:expr, doc) => {
                Spec { name: $name, title: $title, needs_document: true }
            };
        }

        match self {
            Builtin::FileNew => spec!("file.new", LocId::FileNew),
            Builtin::FileOpen => spec!("file.open", LocId::FileOpen),
            Builtin::FileSave => spec!("file.save", LocId::FileSave, doc),
            Builtin::FileSaveAs => spec!("file.saveAs", LocId::FileSaveAs, doc),
            Builtin::FilePreferences => spec!("file.preferences", LocId::FilePreferences),
            Builtin::FileClose => spec!("file.close", LocId::FileClose, doc),
            Builtin::FileExit => spec!("file.exit", LocId::FileExit),

            Builtin::EditUndo => spec!("edit.undo", LocId::EditUndo, doc),
            Builtin::EditRedo => spec!("edit.redo", LocId::EditRedo, doc),
            Builtin::EditCut => spec!("edit.cut", LocId::EditCut, doc),
            Builtin::EditCopy => spec!("edit.copy", LocId::EditCopy, doc),
            Builtin::EditPaste => spec!("edit.paste", LocId::EditPaste, doc),
            Builtin::EditFind => spec!("edit.find", LocId::EditFind, doc),
            Builtin::EditReplace => spec!("edit.replace", LocId::EditReplace, doc),
            Builtin::EditFindNext => spec!("edit.findNext", LocId::EditFindNext, doc),
            Builtin::EditSelectAll => spec!("edit.selectAll", LocId::EditSelectAll, doc),

            Builtin::ViewFocusStatusbar => {
                spec!("view.focusStatusbar", LocId::ViewFocusStatusbar, doc)
            }
            Builtin::ViewGoToFile => spec!("view.goToFile", LocId::ViewGoToFile, doc),
            Builtin::ViewGoToLine => spec!("view.goToLine", LocId::FileGoto, doc),
            Builtin::ViewToggleWordWrap => {
                spec!("view.toggleWordWrap", LocId::ViewWordWrap, doc)
            }
            Builtin::ViewCommandPalette => {
                spec!("view.commandPalette", LocId::CommandPalette)
            }

            Builtin::HelpAbout => spec!("help.about", LocId::HelpAbout),

            Builtin::ConfigReload => spec!("config.reload", LocId::ConfigReload),
        }
    }
}

/// What running a command actually does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Builtin(Builtin),
    /// A command owned by the plugin host, dispatched back to it by handle.
    Plugin(PluginRef),
}

/// An opaque handle the plugin host uses to find its side of a command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PluginRef(pub u32);

pub struct Command {
    pub name: Box<str>,
    pub title: Box<str>,
    pub action: Action,
    pub needs_document: bool,
}

pub struct Commands {
    entries: Vec<Command>,
    by_name: HashMap<Box<str>, CommandId>,
}

impl Commands {
    pub fn new() -> Self {
        let mut this = Self {
            entries: Vec::with_capacity(Builtin::ALL.len()),
            by_name: HashMap::with_capacity(Builtin::ALL.len()),
        };

        for (i, &builtin) in Builtin::ALL.iter().enumerate() {
            // `Builtin::id` hands out `self as u32`, so insertion order is the
            // contract that keeps those handles pointing at the right entry.
            debug_assert_eq!(builtin as u32 as usize, i);

            let spec = builtin.spec();
            let id = this.insert(Command {
                name: spec.name.into(),
                title: loc(spec.title).into(),
                action: Action::Builtin(builtin),
                needs_document: spec.needs_document,
            });
            debug_assert_eq!(id, builtin.id());
        }

        this
    }

    fn insert(&mut self, cmd: Command) -> CommandId {
        // Re-registering a name replaces the entry in place, so that a plugin
        // reload doesn't leave stale duplicates in the palette, and so that any
        // `CommandId` already handed out keeps resolving to the live command.
        if let Some(&id) = self.by_name.get(&cmd.name) {
            self.entries[id.0 as usize] = cmd;
            return id;
        }

        let id = CommandId(self.entries.len() as u32);
        self.by_name.insert(cmd.name.clone(), id);
        self.entries.push(cmd);
        id
    }

    /// Registers a command owned by the plugin host.
    #[allow(dead_code, reason = "used once the plugin host lands")]
    pub fn register(&mut self, name: &str, title: &str, plugin: PluginRef) -> CommandId {
        self.insert(Command {
            name: name.into(),
            title: title.into(),
            action: Action::Plugin(plugin),
            needs_document: false,
        })
    }

    pub fn get(&self, id: CommandId) -> Option<&Command> {
        self.entries.get(id.0 as usize)
    }

    pub fn id_of(&self, name: &str) -> Option<CommandId> {
        self.by_name.get(name).copied()
    }

    /// Every registered command, in registration order.
    #[allow(dead_code, reason = "used by the command palette")]
    pub fn iter(&self) -> impl Iterator<Item = (CommandId, &Command)> {
        self.entries.iter().enumerate().map(|(i, c)| (CommandId(i as u32), c))
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs a command. Returns whether it did anything, which the caller uses to
/// decide if the triggering input should count as consumed.
pub fn exec(ctx: &mut Context, state: &mut State, id: CommandId) -> bool {
    let Some(cmd) = state.commands.get(id) else {
        return false;
    };
    let (action, needs_document) = (cmd.action, cmd.needs_document);

    if needs_document && state.documents.active().is_none() {
        return false;
    }

    match action {
        Action::Builtin(builtin) => exec_builtin(ctx, state, builtin),
        // The plugin host is not wired up yet; a registered plugin command
        // cannot exist, so reaching this is a bug rather than a user error.
        Action::Plugin(_) => false,
    }
}

/// Runs a command by name. Returns false if no such command is registered.
#[allow(dead_code, reason = "used by the plugin host and the command palette")]
pub fn exec_by_name(ctx: &mut Context, state: &mut State, name: &str) -> bool {
    match state.commands.id_of(name) {
        Some(id) => exec(ctx, state, id),
        None => false,
    }
}

fn exec_builtin(ctx: &mut Context, state: &mut State, builtin: Builtin) -> bool {
    match builtin {
        Builtin::FileNew => draw_add_untitled_document(ctx, state),
        Builtin::FileOpen => state.wants_file_picker = StateFilePicker::Open,
        Builtin::FileSave => state.wants_save = true,
        Builtin::FileSaveAs => state.wants_file_picker = StateFilePicker::SaveAs,
        Builtin::FilePreferences => return open_preferences(ctx, state),
        Builtin::FileClose => state.wants_close = true,
        Builtin::FileExit => state.wants_exit = true,

        Builtin::EditUndo => with_active_buffer(state, |tb| tb.undo()),
        Builtin::EditRedo => with_active_buffer(state, |tb| tb.redo()),
        Builtin::EditCut => {
            let doc = state.documents.active().unwrap();
            doc.buffer.borrow_mut().cut(ctx.clipboard_mut());
        }
        Builtin::EditCopy => {
            let doc = state.documents.active().unwrap();
            doc.buffer.borrow_mut().copy(ctx.clipboard_mut());
        }
        Builtin::EditPaste => {
            let doc = state.documents.active().unwrap();
            doc.buffer.borrow_mut().paste(ctx.clipboard_ref(), false);
        }
        Builtin::EditSelectAll => with_active_buffer(state, |tb| tb.select_all()),

        Builtin::EditFind | Builtin::EditReplace => {
            if state.wants_search.kind == StateSearchKind::Disabled {
                return false;
            }
            state.wants_search.kind = if builtin == Builtin::EditFind {
                StateSearchKind::Search
            } else {
                StateSearchKind::Replace
            };
            state.wants_search.focus = true;
        }
        Builtin::EditFindNext => search_execute(ctx, state, SearchAction::Search),

        Builtin::ViewFocusStatusbar => state.wants_statusbar_focus = true,
        Builtin::ViewGoToFile => state.wants_go_to_file = true,
        Builtin::ViewGoToLine => state.wants_goto = true,
        Builtin::ViewToggleWordWrap => with_active_buffer(state, |tb| {
            let enabled = tb.is_word_wrap_enabled();
            tb.set_word_wrap(!enabled);
        }),
        Builtin::ViewCommandPalette => {
            state.command_palette_needle.clear();
            state.wants_command_palette = true;
        }

        Builtin::HelpAbout => state.wants_about = true,

        Builtin::ConfigReload => {
            if let Err(err) = Settings::reload() {
                error_log_add(ctx, state, err);
            }
            reload_keymap(state);
        }
    }

    ctx.needs_rerender();
    true
}

/// Runs `f` against the active document's buffer. Callers must have already
/// established that a document is open, which `exec` does via `needs_document`.
fn with_active_buffer(state: &mut State, f: impl FnOnce(&mut edit::buffer::TextBuffer)) {
    let doc = state.documents.active().unwrap();
    f(&mut doc.buffer.borrow_mut());
}

fn open_preferences(ctx: &mut Context, state: &mut State) -> bool {
    let path = Settings::borrow().path.clone();
    if path.as_os_str().is_empty() {
        return false;
    }

    match state.documents.add_file_path(&path) {
        Ok(doc) => {
            if let mut tb = doc.buffer.borrow_mut()
                && tb.text_length() == 0
            {
                Settings::bootstrap(&mut tb);
            }
        }
        Err(err) => error_log_add(ctx, state, err),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_discriminants_match_all_order() {
        for (i, &b) in Builtin::ALL.iter().enumerate() {
            assert_eq!(b as u32 as usize, i, "Builtin::ALL is out of order at {i}");
            assert_eq!(b.id(), CommandId(i as u32));
        }
    }

    #[test]
    fn builtin_names_are_unique_and_dotted() {
        let mut seen = std::collections::HashSet::new();
        for &b in Builtin::ALL {
            let name = b.spec().name;
            assert!(name.contains('.'), "{name} is not a dotted identifier");
            assert!(seen.insert(name), "duplicate command name {name}");
        }
    }
}
