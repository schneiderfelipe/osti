//! The editing session: content, selection, mode, and history.

use crate::action::Action;
use crate::history::History;
use crate::mode::Mode;
use crate::pitch::Pitch;
use crate::playback::Playback;
use crate::range::Range;
use crate::selection::Selection;
use crate::time::Tick;
use crate::track::Position;

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
    /// The `:` command line's text, meaningful only while `mode` is `Command`.
    pub command_line: String,
    history: History,
}

impl Editor {
    /// Build a fresh editor: one empty track, a single collapsed selection at the start, normal
    /// mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            playback: Playback::new(),
            selection: Selection::single(Range::at(Position {
                tick: Tick(0),
                pitch: Pitch::A4,
            })),
            mode: Mode::default(),
            command_line: String::new(),
            history: History::default(),
        }
    }

    /// Apply one action, returning the action that was actually applied in the forward
    /// direction — for `Undo`/`Redo` that's the replayed action, not the token itself; for
    /// anything else it's simply `action` handed back. Forward it to the audio thread's own
    /// `Playback` whenever it's `Action::Playback(_)`. `None` only for `Undo`/`Redo` with nothing
    /// to replay (an empty stack) — nothing to forward then either.
    ///
    /// # Panics
    ///
    /// If `action` is `Action::Quit` — ending the session isn't a state mutation this method
    /// could perform; the runtime is expected to intercept `Quit` and stop before ever calling
    /// `update` with it (see `osti`'s own `run` loop).
    pub fn update(&mut self, action: Action) -> Option<Action> {
        let resolved = self.history.resolve(action)?;

        let inverse = match &resolved.action {
            // Unlike `Quit`, `Undo`/`Redo` themselves are never in `resolved.action` — `resolve`
            // above already replaced them with the concrete, data-mutating action they replay.
            Action::Undo | Action::Redo => {
                unreachable!("`History::resolve` replaces these with the action they replay")
            }
            Action::Quit => {
                unreachable!("the runtime must intercept `Quit` before calling `update`")
            }
            Action::SetSelection(new) => {
                self.selection = new.clone().normalized();
                // Moving the selection isn't undoable — matching how editors generally treat
                // cursor movement (Ctrl-Z reaches past it to the last real edit).
                None
            }
            Action::SetMode(mode) => {
                self.mode = *mode;
                if *mode != Mode::Command {
                    self.command_line.clear();
                }
                None // mode switches aren't undoable either.
            }
            Action::SetCommandLine(text) => {
                self.command_line.clone_from(text);
                None
            }
            Action::Playback(playback_action) => {
                self.playback.apply(playback_action).map(Action::Playback)
            }
        };

        self.history.settle(&resolved, inverse);
        Some(resolved.action)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let editor = Editor::new();
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
    fn update_echoes_back_a_fresh_action() {
        let mut editor = Editor::new();
        let action = insert_a4_at(0);
        assert_eq!(editor.update(action.clone()), Some(action));
    }

    #[test]
    fn undo_reverses_the_most_recent_action() {
        let mut editor = Editor::new();
        editor.update(insert_a4_at(0));

        editor.update(Action::Undo);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            0
        );
    }

    #[test]
    fn redo_reapplies_an_undone_action() {
        let mut editor = Editor::new();
        editor.update(insert_a4_at(0));
        editor.update(Action::Undo);

        editor.update(Action::Redo);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            1
        );
    }

    #[test]
    fn leaving_command_mode_clears_the_command_line() {
        let mut editor = Editor::new();
        editor.update(Action::SetMode(Mode::Command));
        editor.update(Action::SetCommandLine("quit".to_string()));

        editor.update(Action::SetMode(Mode::Normal));

        assert_eq!(editor.mode, Mode::Normal);
        assert_eq!(editor.command_line, "");
    }

    #[test]
    #[should_panic(expected = "the runtime must intercept")]
    fn quit_must_never_reach_update() {
        // Documents, and enforces, the contract `update`'s own docs state: a caller — today,
        // only `osti`'s run loop — must handle `Action::Quit` itself before it ever gets here.
        Editor::new().update(Action::Quit);
    }

    #[test]
    fn moving_the_selection_is_not_undoable() {
        let mut editor = Editor::new();
        let moved = Selection::single(Range::at(Position {
            tick: Tick(1),
            pitch: Pitch::A4,
        }));

        editor.update(Action::SetSelection(moved));
        editor.update(Action::Undo); // nothing to undo — the move wasn't recorded

        assert_eq!(editor.selection.primary().anchor, Tick(1));
    }
}
