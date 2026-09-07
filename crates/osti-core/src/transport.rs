//! Playback intent and position.

use crate::time::Tick;

/// Whether the transport is advancing.
///
/// There's no separate "stopped" state: stopping is pausing and seeking to the start (`Tick(0)`)
/// at once, not a third thing to represent — see how `Action::Playback(PlaybackAction::Seek(_))`
/// and `SetPlaybackIntent(Paused)` compose for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackIntent {
    /// Not advancing.
    #[default]
    Paused,
    /// Advancing.
    Playing,
}

impl PlaybackIntent {
    /// The other state — play/pause always means "switch to whichever I'm not."
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Playing => Self::Paused,
            Self::Paused => Self::Playing,
        }
    }
}

/// The transport's state: whether it's running, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transport {
    /// Whether playback is currently advancing.
    pub intent: PlaybackIntent,
    /// The current (or, while paused, the resume) position.
    pub position: Tick,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_flips_between_playing_and_paused() {
        assert_eq!(PlaybackIntent::Paused.toggled(), PlaybackIntent::Playing);
        assert_eq!(PlaybackIntent::Playing.toggled(), PlaybackIntent::Paused);
    }
}
