//! A track: notes placed in pitch and time.

use std::collections::BTreeMap;

use crate::pitch::Pitch;
use crate::time::{Length, Tick};

/// A coordinate in a track's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// When the note starts.
    pub tick: Tick,
    /// Which pitch it's at.
    pub pitch: Pitch,
}

/// A sorted map from where each note starts to how long it lasts.
///
/// The value only carries data about the note *other* than its pitch (currently just how long it
/// lasts) — the pitch is already the other half of the key, so repeating it in the value would be
/// the same fact stored twice. No length or looping of its own yet — a track is just an open,
/// unbounded timeline of notes; how much of it plays, loops, or is shown is a separate concern
/// for later (transport, viewport), not this type's job.
#[derive(Debug, Clone, Default)]
pub struct Track {
    notes: BTreeMap<Position, Length>,
}

impl Track {
    /// An empty track.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            notes: BTreeMap::new(),
        }
    }

    /// Every note whose span covers `tick`, at any pitch — zero, one, or several (a chord).
    pub fn sounding_at(&self, tick: Tick) -> impl Iterator<Item = (Position, Length)> + '_ {
        let window = window(tick, tick);
        self.notes
            .range(window)
            .filter(move |&(&position, &length)| covers(position, length, tick))
            .map(|(&position, &length)| (position, length))
    }

    /// Every note at `pitch` overlapping `[at, at + length)`.
    fn overlapping(&self, pitch: Pitch, at: Tick, length: Length) -> Vec<(Position, Length)> {
        let end = at.0.saturating_add(u16::from(length.0));
        // A candidate can start anywhere from `Length::MAX` ticks before `at` (any earlier and
        // even the longest possible note couldn't reach `at`) up to `end - 1` (any later and it
        // starts after `[at, end)` is already over) — not just at or before `at` itself, which
        // would miss an existing note starting partway through the new one.
        let window = window(at, Tick(end.saturating_sub(1)));
        self.notes
            .range(window)
            .filter(|&(&position, &len)| {
                position.pitch == pitch
                    && position.tick.0 < end
                    && at.0 < position.tick.0.saturating_add(u16::from(len.0))
            })
            .map(|(&position, &length)| (position, length))
            .collect()
    }

    /// Place a note at `at`, removing (and returning) whatever same-pitch notes it overlaps —
    /// two notes of the same pitch sounding at once in one track isn't a sound, it's an
    /// undefined one; a chord is several *different* pitches, not overlapping copies of one.
    pub(crate) fn insert(&mut self, at: Position, length: Length) -> Vec<(Position, Length)> {
        let removed = self.overlapping(at.pitch, at.tick, length);
        for (position, _) in &removed {
            self.notes.remove(position);
        }
        self.notes.insert(at, length);
        removed
    }

    /// Remove the note at `at`, if any, returning its length.
    pub(crate) fn remove(&mut self, at: Position) -> Option<Length> {
        self.notes.remove(&at)
    }
}

/// The narrowest range of the map that could contain a note starting anywhere from
/// `Length::MAX` ticks before `earliest_start` (any earlier and even the longest possible note
/// couldn't start late enough to still matter) through `latest_start`, at any pitch.
fn window(earliest_start: Tick, latest_start: Tick) -> std::ops::RangeInclusive<Position> {
    let lower = Position {
        tick: Tick(
            earliest_start
                .0
                .saturating_sub(u16::from(Length::MAX.0.saturating_sub(1))),
        ),
        pitch: Pitch(u8::MIN),
    };
    let upper = Position {
        tick: latest_start,
        pitch: Pitch(u8::MAX),
    };
    lower..=upper
}

fn covers(position: Position, length: Length, tick: Tick) -> bool {
    position.tick <= tick && tick.0 < position.tick.0.saturating_add(u16::from(length.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(tick: u16, pitch: u8) -> Position {
        Position {
            tick: Tick(tick),
            pitch: Pitch(pitch),
        }
    }

    #[test]
    fn empty_track_has_no_notes_sounding() {
        let track = Track::new();
        assert_eq!(track.sounding_at(Tick(0)).count(), 0);
    }

    #[test]
    fn sounding_at_finds_a_chord() {
        let mut track = Track::new();
        track.insert(at(0, 60), Length(4));
        let removed = track.insert(at(0, 64), Length(4));

        assert!(removed.is_empty()); // different pitches never conflict
        assert_eq!(track.sounding_at(Tick(2)).count(), 2);
    }

    #[test]
    fn sounding_at_excludes_notes_outside_their_span() {
        let mut track = Track::new();
        track.insert(at(0, 60), Length(4));

        assert_eq!(track.sounding_at(Tick(4)).count(), 0); // [0, 4) — 4 is just past the end
    }

    #[test]
    fn inserting_over_a_same_pitch_note_replaces_it() {
        let mut track = Track::new();
        track.insert(at(0, 60), Length(8));

        let removed = track.insert(at(2, 60), Length(2));

        assert_eq!(removed, vec![(at(0, 60), Length(8))]);
        assert_eq!(track.sounding_at(Tick(0)).count(), 0); // the old note is gone
        assert_eq!(track.sounding_at(Tick(2)).count(), 1); // the new one took its place
    }

    #[test]
    fn inserting_a_long_note_also_replaces_a_shorter_one_starting_partway_through() {
        let mut track = Track::new();
        track.insert(at(5, 60), Length(2)); // starts *after* the note below, not before it

        let removed = track.insert(at(0, 60), Length(10));

        assert_eq!(removed, vec![(at(5, 60), Length(2))]);
        assert_eq!(track.sounding_at(Tick(5)).count(), 1); // only the new, longer note remains
    }
}
