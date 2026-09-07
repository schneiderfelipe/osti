//! Domain model for `osti`: tracks, notes, selection, and the action/undo core.

mod action;
mod editor;
mod history;
mod mode;
mod pitch;
mod playback;
mod range;
mod selection;
mod time;
mod track;
mod transport;

pub use action::Action;
pub use editor::Editor;
pub use mode::Mode;
pub use pitch::Pitch;
pub use playback::{Playback, PlaybackAction, TrackId};
pub use range::Range;
pub use selection::Selection;
pub use time::{Length, Tick};
pub use track::{Note, Position, Track};
pub use transport::{PlaybackIntent, Transport};

/// Test-only helpers shared across this crate's own test modules, so a `Position` is built the
/// same way everywhere instead of each module redefining an identical helper.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::pitch::Pitch;
    use crate::time::Tick;
    use crate::track::Position;

    /// A position at `tick`, `pitch`.
    pub fn at(tick: u16, pitch: u8) -> Position {
        Position {
            tick: Tick(tick),
            pitch: Pitch(pitch),
        }
    }
}
