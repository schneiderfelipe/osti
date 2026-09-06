//! The one vocabulary every key, and later every typed command and recorded macro, dispatches
//! through.

use crate::playback::PlaybackAction;
use crate::selection::Selection;

/// Everything a keypress can mean.
///
/// Only data mutations live here (plus `Quit`, ending the process, and `Undo`/`Redo`, which
/// replay a stored one) — not everything that mutates data needs to be *undoable* to belong here
/// (see how `Editor::update` treats `SetSelection` and the transport controls), but nothing that
/// merely *reads* state does.
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
    /// An action affecting the audio-relevant state, forwarded to the audio thread.
    Playback(PlaybackAction),
}
