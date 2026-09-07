//! Audio-relevant state, and its own action type — replicated onto the audio thread by applying
//! the same actions there, rather than by sharing memory or sending snapshots.

use nonempty::NonEmpty;

use crate::time::{Length, Tick};
use crate::track::{Position, Track};
use crate::transport::{PlaybackIntent, Transport};

/// Which track, among a [`Playback`]'s tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(pub u8);

/// Everything the audio thread needs to play.
///
/// One or more tracks (always at least one — a `Playback` playing nothing isn't worth
/// representing separately from "one empty track"), all sharing one transport: starting playback
/// starts every track at once, since transport applies to the whole `Playback`, not per track.
#[derive(Debug, Clone)]
pub struct Playback {
    /// The tracks being played, together.
    pub tracks: NonEmpty<Track>,
    /// The shared transport.
    pub transport: Transport,
}

impl Playback {
    /// One empty track, not playing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: NonEmpty::new(Track::new()),
            transport: Transport::default(),
        }
    }

    /// The track at `id`.
    #[must_use]
    pub fn track(&self, id: TrackId) -> &Track {
        &self.tracks[usize::from(id.0)]
    }

    fn track_mut(&mut self, track: TrackId) -> &mut Track {
        &mut self.tracks[usize::from(track.0)]
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self::new()
    }
}

/// An action that changes [`Playback`]'s state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackAction {
    /// Place a note, replacing whatever same-pitch notes it overlaps.
    InsertNote {
        /// Which track.
        track: TrackId,
        /// Where and at what pitch.
        at: Position,
        /// How long it lasts.
        length: Length,
    },
    /// Remove the note at a position, if any.
    RemoveNote {
        /// Which track.
        track: TrackId,
        /// Where to remove from.
        at: Position,
    },
    /// Start or pause the transport.
    SetPlaybackIntent(PlaybackIntent),
    /// Move the transport's position.
    Seek(Tick),
    /// Apply several actions in order, as one.
    Batch(Box<NonEmpty<Self>>),
}

impl PlaybackAction {
    /// Combine several actions into one atomic batch — how a multi-cursor edit is built, one
    /// action per range in the selection. `None` for an empty input: there's nothing to batch,
    /// and an empty batch isn't a representable state to begin with. A single action is handed
    /// back unwrapped rather than boxed in a one-element batch — the overwhelmingly common case
    /// (one cursor) shouldn't pay for generality it isn't using.
    #[must_use]
    pub fn batch(actions: impl IntoIterator<Item = Self>) -> Option<Self> {
        let actions = NonEmpty::from_vec(actions.into_iter().collect())?;
        if actions.tail.is_empty() {
            Some(actions.head)
        } else {
            Some(Self::Batch(Box::new(actions)))
        }
    }
}

impl Playback {
    /// Apply one action, returning its inverse if it's undoable.
    ///
    /// `None` covers two different but related cases: the action isn't undoable at all (the
    /// transport controls — matching how real transport controls behave elsewhere, undoing
    /// "pressed play" doesn't rewind anything), or it was undoable in principle but had nothing
    /// to undo (removing a note that wasn't there).
    ///
    pub fn apply(&mut self, action: &PlaybackAction) -> Option<PlaybackAction> {
        match action {
            PlaybackAction::InsertNote { track, at, length } => {
                let removed = self.track_mut(*track).insert(*at, *length);
                let mut inverses: Vec<PlaybackAction> = removed
                    .into_iter()
                    .map(|note| PlaybackAction::InsertNote {
                        track: *track,
                        at: note.position,
                        length: note.length,
                    })
                    .collect();
                inverses.push(PlaybackAction::RemoveNote {
                    track: *track,
                    at: *at,
                });
                // Always `Some`: `inverses` always has at least the `RemoveNote` just pushed —
                // same collapsing `PlaybackAction::batch` gives every other multi-action caller,
                // rather than reimplementing it here by hand.
                PlaybackAction::batch(inverses)
            }
            PlaybackAction::RemoveNote { track, at } => {
                self.track_mut(*track)
                    .remove(*at)
                    .map(|length| PlaybackAction::InsertNote {
                        track: *track,
                        at: *at,
                        length,
                    })
            }
            PlaybackAction::SetPlaybackIntent(intent) => {
                self.transport.intent = *intent;
                None
            }
            PlaybackAction::Seek(tick) => {
                self.transport.position = *tick;
                None
            }
            PlaybackAction::Batch(batch) => {
                // Revert whatever was undoable and skip what wasn't, rather than making the whole
                // batch non-undoable because one piece of it isn't — a batch mixing a transport
                // change with real edits shouldn't silently swallow undo for the edits too.
                let mut inverses: Vec<PlaybackAction> =
                    batch.iter().filter_map(|a| self.apply(a)).collect();
                inverses.reverse();
                NonEmpty::from_vec(inverses).map(|ne| PlaybackAction::Batch(Box::new(ne)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::at;

    #[test]
    fn inserting_a_note_undoes_to_removing_it() {
        let mut playback = Playback::new();
        let insert = PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(0, 60),
            length: Length(4),
        };

        let inverse = playback.apply(&insert);

        assert_eq!(
            inverse,
            Some(PlaybackAction::RemoveNote {
                track: TrackId(0),
                at: at(0, 60)
            })
        );
    }

    #[test]
    fn removing_nothing_is_not_undoable() {
        let mut playback = Playback::new();
        let inverse = playback.apply(&PlaybackAction::RemoveNote {
            track: TrackId(0),
            at: at(0, 60),
        });
        assert_eq!(inverse, None);
    }

    #[test]
    fn transport_actions_are_not_undoable() {
        let mut playback = Playback::new();
        let inverse = playback.apply(&PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing));
        assert_eq!(inverse, None);
        assert_eq!(playback.transport.intent, PlaybackIntent::Playing);
    }

    #[test]
    fn a_batch_mixing_undoable_and_not_reverts_only_the_undoable_part() {
        let mut playback = Playback::new();
        let batch = PlaybackAction::Batch(Box::new(NonEmpty::from((
            PlaybackAction::InsertNote {
                track: TrackId(0),
                at: at(0, 60),
                length: Length(4),
            },
            vec![PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing)],
        ))));

        let inverse = playback.apply(&batch);

        assert_eq!(
            inverse,
            Some(PlaybackAction::Batch(Box::new(NonEmpty::new(
                PlaybackAction::RemoveNote {
                    track: TrackId(0),
                    at: at(0, 60)
                }
            ))))
        );
    }

    #[test]
    fn inserting_over_an_existing_note_undoes_to_restoring_it() {
        let mut playback = Playback::new();
        playback.apply(&PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(0, 60),
            length: Length(8),
        });

        let inverse = playback.apply(&PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(2, 60),
            length: Length(2),
        });

        #[allow(clippy::unwrap_used)] // asserting the precondition the test is set up to satisfy
        let undo = inverse.unwrap();
        playback.apply(&undo);
        // The old, longer note is back, covering tick 2 again (it always did — [0, 8)); what's
        // gone is the new note that used to occupy this position, and nothing past the old one.
        assert_eq!(playback.tracks.first().sounding_at(Tick(2)).count(), 1);
        assert_eq!(playback.tracks.first().sounding_at(Tick(10)).count(), 0);
    }
}
