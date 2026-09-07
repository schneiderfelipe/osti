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
    /// Build one empty track, not playing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: NonEmpty::new(Track::new()),
            transport: Transport::default(),
        }
    }

    /// Return the track at `id`.
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
    /// (one cursor) shouldn't pay for generality it isn't using, right down to not allocating for
    /// it: only the second action onward, if there is one, spills onto the heap.
    #[must_use]
    pub fn batch(actions: impl IntoIterator<Item = Self>) -> Option<Self> {
        let actions = NonEmpty::collect(actions)?;
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
                // Undo the new note *before* restoring whatever it displaced, not after: a
                // `Batch` applies its actions in the order they're stored, and restoring a
                // displaced note first would insert it while the new note is still there —
                // immediately overlapping it, so `Track::insert` would silently clobber the new
                // note as a side effect right then, before the `RemoveNote` below ever runs. If a
                // displaced note happened to start at the exact same position as the new one,
                // that stray clobber is what the later `RemoveNote` would actually delete: the
                // just-restored note, not the new one — silently losing it. Undoing the new
                // note's own insertion first (mirroring how it was the *last* thing the forward
                // action did) avoids that entirely: the track is back to having neither note
                // before any restoration runs, so restoring the displaced ones can't collide with
                // anything.
                let mut inverses = vec![PlaybackAction::RemoveNote {
                    track: *track,
                    at: *at,
                }];
                inverses.extend(removed.into_iter().map(|note| PlaybackAction::InsertNote {
                    track: *track,
                    at: note.position,
                    length: note.length,
                }));
                // Always `Some`: `inverses` always has at least the `RemoveNote` pushed above —
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
                // Same collapsing every other multi-action inverse goes through — a batch that
                // happens to reduce to exactly one undoable sub-action shouldn't come back as a
                // one-element `Batch` any more than `InsertNote`'s own inverse does.
                PlaybackAction::batch(inverses)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::at;

    #[test]
    fn batch_of_nothing_is_none() {
        assert_eq!(PlaybackAction::batch(Vec::new()), None);
    }

    #[test]
    fn batch_of_one_is_the_bare_action_not_a_batch() {
        let action = PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing);
        assert_eq!(PlaybackAction::batch([action.clone()]), Some(action));
    }

    #[test]
    fn batch_of_several_keeps_them_in_order() {
        let first = PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing);
        let second = PlaybackAction::Seek(Tick(3));

        let batch = PlaybackAction::batch([first.clone(), second.clone()]);

        assert_eq!(
            batch,
            Some(PlaybackAction::Batch(Box::new(NonEmpty::from((
                first,
                vec![second]
            )))))
        );
    }

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

        // Just the one surviving inverse, unwrapped — not a one-element `Batch` — same collapsing
        // `PlaybackAction::batch` gives any other single-action result.
        assert_eq!(
            inverse,
            Some(PlaybackAction::RemoveNote {
                track: TrackId(0),
                at: at(0, 60)
            })
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

    #[test]
    fn undoing_an_overlap_clobbering_insert_restores_every_displaced_note() {
        // Regression test: a note displaced by the insert can start at the exact same position
        // the new note does. Restoring it *before* undoing the new note's own insertion used to
        // insert it right on top of the still-present new note, silently clobbering that new note
        // — which the final `RemoveNote` then deleted, having already been restored, instead of
        // the new note it was actually meant to remove.
        let mut playback = Playback::new();
        playback.apply(&PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(0, 60),
            length: Length(2),
        });
        playback.apply(&PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(3, 60),
            length: Length(2),
        });

        let inverse = playback.apply(&PlaybackAction::InsertNote {
            track: TrackId(0),
            at: at(0, 60),
            length: Length(6), // swallows both notes above
        });

        #[allow(clippy::unwrap_used)] // asserting the precondition the test is set up to satisfy
        playback.apply(&inverse.unwrap());

        // Both displaced notes are back — including the one sharing the new note's own start.
        assert_eq!(playback.tracks.first().sounding_at(Tick(0)).count(), 1);
        assert_eq!(playback.tracks.first().sounding_at(Tick(3)).count(), 1);
    }
}
