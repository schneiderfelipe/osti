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

/// The transport's state: whether it's running, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transport {
    /// Whether playback is currently advancing.
    pub intent: PlaybackIntent,
    /// The current (or, while paused, the resume) position.
    pub position: Tick,
}
