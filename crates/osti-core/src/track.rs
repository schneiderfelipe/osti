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

/// A placed note: where it starts, and how long it lasts.
///
/// What every query for a note actually in a [`Track`] hands back, rather than a bare
/// `(Position, Length)` tuple — bundling the two together is what lets `end`/`covers` live in one
/// place instead of every caller re-deriving "where does this note end" or "is it sounding at
/// tick X" by hand from the pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Where it starts.
    pub position: Position,
    /// How long it lasts.
    pub length: Length,
}

impl Note {
    /// The last tick this note covers.
    #[must_use]
    pub fn end(self) -> Tick {
        Tick(
            self.position
                .tick
                .0
                .saturating_add(u16::from(self.length.0))
                .saturating_sub(1),
        )
    }

    /// Whether this note's span covers `tick`.
    #[must_use]
    pub fn covers(self, tick: Tick) -> bool {
        self.position.tick <= tick && tick <= self.end()
    }
}

impl From<(&Position, &Length)> for Note {
    /// Pairs a [`Track`]'s `BTreeMap` entries directly (`(&Position, &Length)`, exactly what its
    /// iterators yield), so every query below can build a `Note` with `.map(Note::from)` instead
    /// of repeating the same field-by-field construction.
    fn from((&position, &length): (&Position, &Length)) -> Self {
        Self { position, length }
    }
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
    pub fn sounding_at(&self, tick: Tick) -> impl Iterator<Item = Note> + '_ {
        let window = window(tick, tick);
        self.notes
            .range(window)
            .map(Note::from)
            .filter(move |note| note.covers(tick))
    }

    /// Every note at `pitch`, in tick order — the one row a selection ever moves along, so this
    /// is what note-boundary movement (jumping to the previous/next note, like Helix's word
    /// motions) and span-based edits (deleting everything a multi-tick selection covers) both
    /// build on.
    pub fn notes_at_pitch(&self, pitch: Pitch) -> impl Iterator<Item = Note> + '_ {
        // The map is sorted tick-first, so filtering to one pitch still yields ascending ticks.
        self.notes
            .iter()
            .filter(move |(position, _)| position.pitch == pitch)
            .map(Note::from)
    }

    /// The start of the nearest note at `pitch` starting before `tick`, if any.
    ///
    /// Helix's `b` (jump to the previous word start), for notes: already sitting on or inside a
    /// note jumps to *that* note's start first (its start is still `< tick` unless `tick` is
    /// exactly it), pressing it again from there reaches the one before.
    #[must_use]
    pub fn previous_note_start(&self, pitch: Pitch, tick: Tick) -> Option<Tick> {
        self.notes_at_pitch(pitch)
            .map(|note| note.position.tick)
            .filter(|&start| start < tick)
            .max()
    }

    /// The end (last covered tick) of the nearest note at `pitch` ending after `tick`, if any —
    /// Helix's `e`, symmetric to [`Track::previous_note_start`].
    #[must_use]
    pub fn next_note_end(&self, pitch: Pitch, tick: Tick) -> Option<Tick> {
        self.notes_at_pitch(pitch)
            .map(Note::end)
            .filter(|&end| end > tick)
            .min()
    }

    /// Every note at `pitch` starting within `[start, end]` (inclusive) — everything a (possibly
    /// multi-tick) selection covers, for deleting more than one note at once. Notes merely
    /// overlapping into the span from before `start` are left alone; only where a note *starts*
    /// counts as being in the selection.
    pub fn positions_in_span(
        &self,
        pitch: Pitch,
        start: Tick,
        end: Tick,
    ) -> impl Iterator<Item = Position> + '_ {
        self.notes_at_pitch(pitch)
            .map(|note| note.position)
            .filter(move |position| start <= position.tick && position.tick <= end)
    }

    /// Every note at `pitch` overlapping `[at, at + length)`.
    fn overlapping(&self, pitch: Pitch, at: Tick, length: Length) -> Vec<Note> {
        let end = at.0.saturating_add(u16::from(length.0));
        // A candidate can start anywhere from `Length::MAX` ticks before `at` (any earlier and
        // even the longest possible note couldn't reach `at`) up to `end - 1` (any later and it
        // starts after `[at, end)` is already over) — not just at or before `at` itself, which
        // would miss an existing note starting partway through the new one.
        let window = window(at, Tick(end.saturating_sub(1)));
        self.notes
            .range(window)
            .map(Note::from)
            .filter(|note| {
                note.position.pitch == pitch && note.position.tick.0 < end && at.0 <= note.end().0
            })
            .collect()
    }

    /// Place a note at `at`, removing (and returning) whatever same-pitch notes it overlaps —
    /// two notes of the same pitch sounding at once in one track isn't a sound, it's an
    /// undefined one; a chord is several *different* pitches, not overlapping copies of one.
    pub(crate) fn insert(&mut self, at: Position, length: Length) -> Vec<Note> {
        let removed = self.overlapping(at.pitch, at.tick, length);
        for note in &removed {
            self.notes.remove(&note.position);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::at;

    fn note(tick: u16, pitch: u8, length: u8) -> Note {
        Note {
            position: at(tick, pitch),
            length: Length(length),
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

        assert_eq!(removed, vec![note(0, 60, 8)]);
        assert_eq!(track.sounding_at(Tick(0)).count(), 0); // the old note is gone
        assert_eq!(track.sounding_at(Tick(2)).count(), 1); // the new one took its place
    }

    #[test]
    fn inserting_a_long_note_also_replaces_a_shorter_one_starting_partway_through() {
        let mut track = Track::new();
        track.insert(at(5, 60), Length(2)); // starts *after* the note below, not before it

        let removed = track.insert(at(0, 60), Length(10));

        assert_eq!(removed, vec![note(5, 60, 2)]);
        assert_eq!(track.sounding_at(Tick(5)).count(), 1); // only the new, longer note remains
    }

    #[test]
    fn previous_note_start_finds_the_current_note_before_an_earlier_one() {
        let mut track = Track::new();
        track.insert(at(0, 60), Length(2));
        track.insert(at(5, 60), Length(2));

        // Standing inside the note at 5: its own start comes first, not the one at 0.
        assert_eq!(track.previous_note_start(Pitch(60), Tick(6)), Some(Tick(5)));
        // Standing exactly on a note's start: the previous *other* note, not itself.
        assert_eq!(track.previous_note_start(Pitch(60), Tick(5)), Some(Tick(0)));
        // Nothing before the first note.
        assert_eq!(track.previous_note_start(Pitch(60), Tick(0)), None);
    }

    #[test]
    fn next_note_end_finds_the_current_note_before_a_later_one() {
        let mut track = Track::new();
        track.insert(at(0, 60), Length(2)); // covers 0..2, ends at 1
        track.insert(at(5, 60), Length(2)); // covers 5..7, ends at 6

        // Standing inside the first note: its own end comes first.
        assert_eq!(track.next_note_end(Pitch(60), Tick(0)), Some(Tick(1)));
        // Standing exactly on that end: the next note's end, not the same one again.
        assert_eq!(track.next_note_end(Pitch(60), Tick(1)), Some(Tick(6)));
        // Nothing after the last note.
        assert_eq!(track.next_note_end(Pitch(60), Tick(6)), None);
    }

    #[test]
    fn note_boundary_motions_ignore_other_pitches() {
        let mut track = Track::new();
        track.insert(at(3, 61), Length(1));
        assert_eq!(track.previous_note_start(Pitch(60), Tick(10)), None);
        assert_eq!(track.next_note_end(Pitch(60), Tick(0)), None);
    }

    #[test]
    fn positions_in_span_finds_notes_starting_within_it_only() {
        let mut track = Track::new();
        track.insert(at(2, 60), Length(1));
        track.insert(at(4, 60), Length(1));
        track.insert(at(8, 60), Length(1));

        let found: Vec<_> = track
            .positions_in_span(Pitch(60), Tick(2), Tick(4))
            .collect();

        assert_eq!(found, vec![at(2, 60), at(4, 60)]);
    }
}
