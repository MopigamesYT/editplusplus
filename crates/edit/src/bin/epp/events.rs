// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! The editor event queue.
//!
//! Plugins need to react to things the editor does, and they need to do it
//! without the editor knowing they exist. The editor therefore emits events
//! into a queue as it works, and the plugin host drains that queue once per
//! frame and fans it out to whoever subscribed.
//!
//! Events are queued rather than dispatched at the emission site on purpose.
//! Emission happens in the middle of the draw pass, often while a document
//! buffer is mutably borrowed; running plugin code there would let a plugin
//! re-enter the editor at a point where its invariants do not hold.

use std::path::PathBuf;

use edit::helpers::Point;

/// Something the editor did, worth telling plugins about.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The editor finished starting up. Emitted exactly once, before the first
    /// frame the user can interact with.
    Ready,
    /// A document was opened or created.
    BufOpen { path: Option<PathBuf>, language: Option<&'static str> },
    /// A document was written to disk.
    BufSave { path: Option<PathBuf> },
    /// A document was closed.
    BufClose { path: Option<PathBuf> },
    /// The active document changed, including when it became `None`.
    BufActivate { path: Option<PathBuf> },
    /// The active document's text changed. Coalesced to at most one per frame.
    BufChange,
    /// The cursor moved. Coalesced to at most one per frame.
    CursorMove { pos: Point },
    /// A document's language was detected or overridden.
    FileType { path: Option<PathBuf>, language: Option<&'static str> },
}

impl Event {
    /// The event's name in config and plugin specs, e.g. `"BufOpen"`.
    pub fn name(&self) -> &'static str {
        match self {
            Event::Ready => "Ready",
            Event::BufOpen { .. } => "BufOpen",
            Event::BufSave { .. } => "BufSave",
            Event::BufClose { .. } => "BufClose",
            Event::BufActivate { .. } => "BufActivate",
            Event::BufChange => "BufChange",
            Event::CursorMove { .. } => "CursorMove",
            Event::FileType { .. } => "FileType",
        }
    }

    /// Whether a second event of this kind in one frame replaces the first.
    /// `CursorMove` and `BufChange` fire constantly while typing; delivering
    /// every one of them would make any subscriber a performance problem.
    fn coalesces(&self) -> bool {
        matches!(self, Event::BufChange | Event::CursorMove { .. })
    }
}

/// Events emitted during the current frame, awaiting delivery.
#[derive(Default)]
pub struct EventQueue {
    pending: Vec<Event>,
}

impl EventQueue {
    pub fn emit(&mut self, event: Event) {
        if event.coalesces()
            && let Some(slot) = self.pending.iter_mut().find(|e| e.name() == event.name())
        {
            *slot = event;
            return;
        }
        self.pending.push(event);
    }

    /// Takes everything queued so far. The caller delivers it; anything emitted
    /// during delivery lands in the next frame's batch rather than extending
    /// this one, so a plugin cannot spin the editor by emitting from a handler.
    pub fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_moves_coalesce_but_opens_do_not() {
        let mut q = EventQueue::default();
        q.emit(Event::CursorMove { pos: Point { x: 1, y: 1 } });
        q.emit(Event::CursorMove { pos: Point { x: 2, y: 2 } });
        q.emit(Event::BufOpen { path: None, language: None });
        q.emit(Event::BufOpen { path: None, language: Some("rust") });

        let drained = q.drain();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0], Event::CursorMove { pos: Point { x: 2, y: 2 } });
        assert!(q.is_empty());
    }

    #[test]
    fn draining_twice_yields_nothing() {
        let mut q = EventQueue::default();
        q.emit(Event::Ready);
        assert_eq!(q.drain().len(), 1);
        assert!(q.drain().is_empty());
    }
}
