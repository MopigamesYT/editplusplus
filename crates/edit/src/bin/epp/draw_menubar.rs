// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use edit::helpers::*;
use edit::input::vk;
use edit::tui::*;
use stdext::arena_format;

use crate::commands::{self, Builtin};
use crate::localization::*;
use crate::settings::Settings;
use crate::state::*;

/// Draws a menu entry for a command and runs it if the user picks it.
///
/// Title and shortcut both come from the registry and the keymap rather than
/// being written out here, so a rebound key shows the new binding and a
/// renamed command shows the new name without touching this file.
fn menu_command(ctx: &mut Context, state: &mut State, builtin: Builtin, accelerator: char) {
    let id = builtin.id();
    let shortcut = state.keymap.shortcut_for(id);

    let activated = match state.commands.get(id) {
        Some(cmd) => ctx.menubar_menu_button(&cmd.title, accelerator, shortcut),
        None => false,
    };

    if activated {
        commands::exec(ctx, state, id);
    }
}

/// As [`menu_command`], for a command that toggles something.
fn menu_command_checkbox(
    ctx: &mut Context,
    state: &mut State,
    builtin: Builtin,
    accelerator: char,
    checked: bool,
) {
    let id = builtin.id();
    let shortcut = state.keymap.shortcut_for(id);

    let activated = match state.commands.get(id) {
        Some(cmd) => ctx.menubar_menu_checkbox(&cmd.title, accelerator, shortcut, checked),
        None => false,
    };

    if activated {
        commands::exec(ctx, state, id);
    }
}

pub fn draw_menubar(ctx: &mut Context, state: &mut State) {
    ctx.menubar_begin();
    ctx.attr_background_rgba(state.menubar_color_bg);
    ctx.attr_foreground_rgba(state.menubar_color_fg);
    {
        let contains_focus = ctx.contains_focus();

        if ctx.menubar_menu_begin(loc(LocId::File), 'F') {
            draw_menu_file(ctx, state);
        }
        if !contains_focus && ctx.consume_shortcut(vk::F10) {
            ctx.steal_focus();
        }
        if state.documents.active().is_some() {
            if ctx.menubar_menu_begin(loc(LocId::Edit), 'E') {
                draw_menu_edit(ctx, state);
            }
            if ctx.menubar_menu_begin(loc(LocId::View), 'V') {
                draw_menu_view(ctx, state);
            }
        }
        if ctx.menubar_menu_begin(loc(LocId::Help), 'H') {
            draw_menu_help(ctx, state);
        }
    }
    ctx.menubar_end();
}

fn draw_menu_file(ctx: &mut Context, state: &mut State) {
    menu_command(ctx, state, Builtin::FileNew, 'N');
    menu_command(ctx, state, Builtin::FileOpen, 'O');

    if state.documents.active().is_some() {
        menu_command(ctx, state, Builtin::FileSave, 'S');
        menu_command(ctx, state, Builtin::FileSaveAs, 'A');
    }

    // Without a config directory there is no settings file to open.
    if !Settings::borrow().path.as_os_str().is_empty() {
        menu_command(ctx, state, Builtin::FilePreferences, 'P');
    }

    if state.documents.active().is_some() {
        menu_command(ctx, state, Builtin::FileClose, 'C');
    }
    menu_command(ctx, state, Builtin::FileExit, 'X');

    ctx.menubar_menu_end();
}

fn draw_menu_edit(ctx: &mut Context, state: &mut State) {
    menu_command(ctx, state, Builtin::EditUndo, 'U');
    menu_command(ctx, state, Builtin::EditRedo, 'R');
    menu_command(ctx, state, Builtin::EditCut, 'T');
    menu_command(ctx, state, Builtin::EditCopy, 'C');
    menu_command(ctx, state, Builtin::EditPaste, 'P');

    if state.wants_search.kind != StateSearchKind::Disabled {
        menu_command(ctx, state, Builtin::EditFind, 'F');
        menu_command(ctx, state, Builtin::EditReplace, 'L');
    }

    menu_command(ctx, state, Builtin::EditSelectAll, 'A');

    ctx.menubar_menu_end();
}

fn draw_menu_view(ctx: &mut Context, state: &mut State) {
    if let Some(doc) = state.documents.active() {
        let word_wrap = doc.buffer.borrow().is_word_wrap_enabled();

        menu_command(ctx, state, Builtin::ViewFocusStatusbar, 'S');
        menu_command(ctx, state, Builtin::ViewGoToFile, 'F');
        menu_command(ctx, state, Builtin::ViewGoToLine, 'G');
        menu_command(ctx, state, Builtin::ViewCommandPalette, 'C');
        menu_command_checkbox(ctx, state, Builtin::ViewToggleWordWrap, 'W', word_wrap);
    }

    ctx.menubar_menu_end();
}

fn draw_menu_help(ctx: &mut Context, state: &mut State) {
    menu_command(ctx, state, Builtin::HelpAbout, 'A');
    ctx.menubar_menu_end();
}

pub fn draw_dialog_about(ctx: &mut Context, state: &mut State) {
    ctx.modal_begin("about", loc(LocId::AboutDialogTitle));
    {
        ctx.block_begin("content");
        ctx.inherit_focus();
        ctx.attr_padding(Rect::three(1, 2, 1));
        {
            ctx.label("description", "edit++");
            ctx.attr_overflow(Overflow::TruncateTail);
            ctx.attr_position(Position::Center);

            ctx.label(
                "version",
                &arena_format!(
                    ctx.arena(),
                    "{}{}",
                    loc(LocId::AboutDialogVersion),
                    env!("CARGO_PKG_VERSION")
                ),
            );
            ctx.attr_overflow(Overflow::TruncateHead);
            ctx.attr_position(Position::Center);

            ctx.label("copyright", "A fork of Microsoft Edit");
            ctx.attr_overflow(Overflow::TruncateTail);
            ctx.attr_position(Position::Center);

            ctx.label("upstream", "Copyright (c) Microsoft Corporation");
            ctx.attr_overflow(Overflow::TruncateTail);
            ctx.attr_position(Position::Center);

            ctx.block_begin("choices");
            ctx.inherit_focus();
            ctx.attr_padding(Rect::three(1, 2, 0));
            ctx.attr_position(Position::Center);
            {
                if ctx.button("ok", loc(LocId::Ok), ButtonStyle::default()) {
                    state.wants_about = false;
                }
                ctx.inherit_focus();
            }
            ctx.block_end();
        }
        ctx.block_end();
    }
    if ctx.modal_end() {
        state.wants_about = false;
    }
}
