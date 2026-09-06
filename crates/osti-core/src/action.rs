//! The one vocabulary every key, and later every typed command and recorded macro, dispatches
//! through.

use crate::mode::Mode;
use crate::playback::PlaybackAction;
use crate::selection::Selection;

/// Everything a keypress can mean.
///
/// Only data mutations live here (plus `Quit`, ending the process, and `Undo`/`Redo`, which
/// replay a stored one) — not everything that mutates data needs to be *undoable* to belong here
/// (see how `Editor::update` treats `SetSelection`, `SetMode`, `SetCommandLine`, and the transport
/// controls), but nothing that merely *reads* state does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// End the session.
    Quit,
    /// Undo the most recent undoable action.
    Undo,
    /// Redo the most recently undone action.
    Redo,
    /// Replace the whole selection.
    SetSelection(Selection),
    /// Switch mode. Leaving `Command` mode this way also clears the command line — there's
    /// nothing left to type once you're not in command mode anymore.
    SetMode(Mode),
    /// Replace the `:` command line's text.
    SetCommandLine(String),
    /// An action affecting the audio-relevant state, forwarded to the audio thread.
    Playback(PlaybackAction),
}
