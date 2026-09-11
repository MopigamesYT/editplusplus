// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! The command palette and the which-key hint.
//!
//! Both exist because of the command registry: the palette lists whatever is
//! registered, and the hint lists whatever the keymap could still match. Neither
//! knows anything about the commands it shows, so a plugin that registers a
//! command gets a palette entry and a key hint for free.

use edit::framebuffer::IndexedColor;
use edit::fuzzy::score_fuzzy;
use edit::helpers::*;
use edit::tui::*;
use stdext::arena_format;

use crate::commands::{self, CommandId};
use crate::keymap;
use crate::localization::*;
use crate::state::*;

/// How many chord continuations the hint will list before giving up on fitting.
const MAX_HINTS: usize = 12;

pub fn draw_command_palette(ctx: &mut Context, state: &mut State) {
    let width = (ctx.size().width - 20).max(20);
    let height = (ctx.size().height - 10).max(6);
    let mut chosen = None;
    let mut done = false;

    ctx.modal_begin("palette", loc(LocId::CommandPalette));
    {
        ctx.table_begin("palette-search");
        ctx.table_set_columns(&[0, COORD_TYPE_SAFE_MAX]);
        ctx.table_set_cell_gap(Size { width: 1, height: 0 });
        ctx.inherit_focus();
        {
            ctx.table_next_row();
            ctx.inherit_focus();

            ctx.label("needle-label", loc(LocId::SearchNeedleLabel));
            ctx.editline("needle", &mut state.command_palette_needle);
            ctx.inherit_focus();
        }
        ctx.table_end();

        let matches = palette_matches(ctx, state);
        let shortcuts = keymap::shortcuts_by_command(&state.keymap);
        let has_document = state.documents.active().is_some();

        ctx.scrollarea_begin("scrollarea", Size { width, height });
        ctx.attr_background_rgba(ctx.indexed_alpha(IndexedColor::Black, 1, 4));
        {
            ctx.list_begin("commands");
            ctx.inherit_focus();

            for &id in &matches {
                let Some(cmd) = state.commands.get(id) else {
                    continue;
                };

                ctx.styled_list_item_begin();
                ctx.attr_overflow(Overflow::TruncateTail);

                // A command that needs a document is shown but dimmed when
                // there isn't one, so the palette stays a stable map of what
                // exists rather than a list that shifts under the user.
                let enabled = has_document || !cmd.needs_document;
                if !enabled {
                    ctx.styled_label_set_foreground(ctx.indexed_alpha(
                        IndexedColor::Foreground,
                        1,
                        2,
                    ));
                }
                ctx.styled_label_add_text(&cmd.title);

                ctx.styled_label_set_foreground(ctx.indexed_alpha(IndexedColor::Foreground, 1, 2));
                ctx.styled_label_add_text(&arena_format!(ctx.arena(), "   {}", cmd.name));

                if let Some(keys) = shortcuts.get(&id) {
                    ctx.styled_label_set_foreground(ctx.indexed(IndexedColor::BrightBlue));
                    ctx.styled_label_add_text(&arena_format!(
                        ctx.arena(),
                        "   {}",
                        keymap::format_keys(keys)
                    ));
                }

                if ctx.styled_list_item_end(false) == ListSelection::Activated && enabled {
                    chosen = Some(id);
                    break;
                }
            }

            ctx.list_end();
        }
        ctx.scrollarea_end();
    }
    done |= ctx.modal_end();

    if let Some(id) = chosen {
        done = true;
        // Close before running, so a command that opens another modal isn't
        // fighting the palette for focus on the next frame.
        state.wants_command_palette = false;
        commands::exec(ctx, state, id);
    }

    if done {
        state.wants_command_palette = false;
        state.command_palette_needle.clear();
        ctx.needs_rerender();
    }
}

/// Commands matching the current needle, best match first. An empty needle
/// lists everything in registration order, which groups them by area.
fn palette_matches(ctx: &Context, state: &State) -> Vec<CommandId> {
    let needle = state.command_palette_needle.trim_ascii();

    if needle.is_empty() {
        return state.commands.iter().map(|(id, _)| id).collect();
    }

    let mut scored: Vec<(i32, usize, CommandId)> = Vec::new();

    for (i, (id, cmd)) in state.commands.iter().enumerate() {
        // Score the title and the dotted name separately and keep the better
        // of the two, so both "go to file" and "view.goto" find the command.
        let (title_score, _) = score_fuzzy(ctx.arena(), &cmd.title, needle, true);
        let (name_score, _) = score_fuzzy(ctx.arena(), &cmd.name, needle, true);
        let score = title_score.max(name_score);

        if score > 0 {
            scored.push((score, i, id));
        }
    }

    // Registration order breaks ties, so equally-scored commands don't shuffle
    // between frames.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, id)| id).collect()
}

/// Shows the chord in progress and what could complete it.
pub fn draw_which_key(ctx: &mut Context, state: &mut State) {
    let continuations = state.keymap.continuations();
    if continuations.is_empty() {
        return;
    }

    let pending = pending_chord_text(state);

    ctx.block_begin("which-key");
    ctx.attr_float(FloatSpec {
        anchor: Anchor::Root,
        gravity_x: 0.0,
        gravity_y: 1.0,
        // Sit just above the statusbar rather than on top of it.
        offset_y: (ctx.size().height - 1) as f32,
        ..Default::default()
    });
    ctx.attr_border();
    ctx.attr_padding(Rect::two(0, 1));
    ctx.attr_background_rgba(ctx.indexed_alpha(IndexedColor::Background, 15, 16));
    {
        ctx.label("prefix", &arena_format!(ctx.arena(), "{pending} …"));
        ctx.attr_foreground_rgba(ctx.indexed(IndexedColor::BrightBlue));

        ctx.table_begin("hints");
        // Both columns size to their content, so the hint is as wide as it
        // needs to be rather than spanning the terminal.
        ctx.table_set_columns(&[0, 0]);
        ctx.table_set_cell_gap(Size { width: 2, height: 0 });
        {
            for (i, (keys, command)) in continuations.iter().take(MAX_HINTS).enumerate() {
                let Some(cmd) = state.commands.get(*command) else {
                    continue;
                };

                ctx.table_next_row();
                ctx.next_block_id_mixin(i as u64);

                ctx.label("key", &keymap::format_keys(keys));
                ctx.attr_foreground_rgba(ctx.indexed(IndexedColor::BrightGreen));

                ctx.label("title", &cmd.title);
                ctx.attr_overflow(Overflow::TruncateTail);
            }

            if continuations.len() > MAX_HINTS {
                ctx.table_next_row();
                ctx.label("more-key", "…");
                ctx.label(
                    "more",
                    &arena_format!(ctx.arena(), "+{}", continuations.len() - MAX_HINTS),
                );
            }
        }
        ctx.table_end();
    }
    ctx.block_end();
}
