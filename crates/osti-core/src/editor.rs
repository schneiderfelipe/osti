//! The editing session: content, selection, mode, and history.

use crate::action::Action;
use crate::history::History;
use crate::mode::Mode;
use crate::pitch::Pitch;
use crate::playback::Playback;
use crate::range::Range;
use crate::selection::Selection;
use crate::time::Tick;

/// The full state of one editing session — the top-level Elm-style model, `Playback` (see there)
/// nested inside it as the audio-relevant part.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The audio-relevant state, also replicated on the audio thread.
    pub playback: Playback,
    /// Where the cursor(s) are.
    pub selection: Selection,
    /// The current editing mode.
    pub mode: Mode,
    history: History,
}

impl Editor {
    /// A fresh editor over one empty track, `length` ticks long, with a single collapsed
    /// selection at the start.
    #[must_use]
    pub fn new(length: Tick) -> Self {
        Self {
            playback: Playback::new(length),
            selection: Selection::single(Range {
                pitch: Pitch::A4,
                anchor: Tick(0),
                head: Tick(0),
            }),
            mode: Mode::default(),
            history: History::default(),
        }
    }

    /// Apply one action, returning the action that was actually applied to reach the new state,
    /// when the caller couldn't already know that from `action` alone.
    ///
    /// For `Undo`/`Redo` this is the replayed action — forward it to the audio thread's own
    /// `Playback` whenever it's `Action::Playback(_)`. For anything else this is always `None`:
    /// the caller still owns `action` itself (never consumed here) and already knows what to
    /// forward from that. `None` for `Undo`/`Redo` specifically means there was nothing to
    /// replay (an empty stack) — also nothing to forward.
    pub fn update(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::Quit => {
                unreachable!("Quit is intercepted by the runtime before reaching Editor::update")
            }
            Action::Undo => {
                let inverse = self.history.pop_undo()?;
                if let Some(redo) = self.mutate(&inverse) {
                    self.history.push_redo(redo);
                }
                Some(inverse)
            }
            Action::Redo => {
                let action = self.history.pop_redo()?;
                if let Some(undo) = self.mutate(&action) {
                    self.history.push_undo(undo);
                }
                Some(action)
            }
            other => {
                if let Some(inverse) = self.mutate(other) {
                    self.history.record(inverse);
                }
                None
            }
        }
    }

    /// The pure primitive step — no history bookkeeping, never sees `Quit`/`Undo`/`Redo`.
    fn mutate(&mut self, action: &Action) -> Option<Action> {
        match action {
            Action::SetSelection(new) => {
                self.selection = new.clone().normalized();
                // Moving the selection isn't undoable — matching how editors generally treat
                // cursor movement (Ctrl-Z reaches past it to the last real edit).
                None
            }
            Action::Playback(playback_action) => {
                self.playback.apply(playback_action).map(Action::Playback)
            }
            Action::Quit | Action::Undo | Action::Redo => {
                unreachable!("intercepted by `update` before reaching `mutate`")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::Position;
    use crate::playback::{PlaybackAction, TrackId};

    fn insert_a4_at(tick: u16) -> Action {
        Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: Position {
                tick: Tick(tick),
                pitch: Pitch::A4,
            },
            length: crate::time::Length(1),
        })
    }

    #[test]
    fn a_fresh_editor_starts_paused_with_no_notes() {
        let editor = Editor::new(Tick(16));
        assert_eq!(
            editor.playback.transport.intent,
            crate::transport::PlaybackIntent::Paused
        );
        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            0
        );
    }

    #[test]
    fn update_returns_none_for_a_fresh_action() {
        let mut editor = Editor::new(Tick(16));
        assert_eq!(editor.update(&insert_a4_at(0)), None);
    }

    #[test]
    fn undo_reverses_the_most_recent_action() {
        let mut editor = Editor::new(Tick(16));
        editor.update(&insert_a4_at(0));

        editor.update(&Action::Undo);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            0
        );
    }

    #[test]
    fn redo_reapplies_an_undone_action() {
        let mut editor = Editor::new(Tick(16));
        editor.update(&insert_a4_at(0));
        editor.update(&Action::Undo);

        editor.update(&Action::Redo);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            1
        );
    }

    #[test]
    fn moving_the_selection_is_not_undoable() {
        let mut editor = Editor::new(Tick(16));
        let moved = Selection::single(Range {
            pitch: Pitch::A4,
            anchor: Tick(1),
            head: Tick(1),
        });

        editor.update(&Action::SetSelection(moved));
        editor.update(&Action::Undo); // nothing to undo — the move wasn't recorded

        assert_eq!(editor.selection.primary().anchor, Tick(1));
    }
}
