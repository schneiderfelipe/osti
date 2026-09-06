//! A pattern: notes placed in pitch and time.

use std::collections::BTreeMap;

use crate::pitch::Pitch;
use crate::time::{Length, Tick};

/// A coordinate in a pattern's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// When the note starts.
    pub tick: Tick,
    /// Which pitch it's at.
    pub pitch: Pitch,
}

/// One loop's worth of notes: a sorted map from where each note starts to how long it lasts, plus
/// the pattern's own loop length.
///
/// The value only carries data about the note *other* than its pitch (currently just how long it
/// lasts) — the pitch is already the other half of the key, so repeating it in the value would be
/// the same fact stored twice.
#[derive(Debug, Clone, Default)]
pub struct Pattern {
    notes: BTreeMap<Position, Length>,
    /// How long one loop of this pattern is, in ticks.
    ///
    /// Independent of `notes`, not derived from it — an empty pattern still needs a length to
    /// have a grid to place notes into, and deriving it from the last note's end would mean
    /// removing that note silently shrinks the loop for everything else in it.
    pub length: Tick,
}

impl Pattern {
    /// An empty pattern, `length` ticks long.
    #[must_use]
    pub const fn new(length: Tick) -> Self {
        Self {
            notes: BTreeMap::new(),
            length,
        }
    }

    /// Every note whose span covers `tick`, at any pitch — zero, one, or several (a chord).
    pub fn sounding_at(&self, tick: Tick) -> impl Iterator<Item = (Position, Length)> + '_ {
        let window = window(Position {
            tick,
            pitch: Pitch(u8::MIN),
        });
        self.notes
            .range(window)
            .filter(move |&(&position, &length)| covers(position, length, tick))
            .map(|(&position, &length)| (position, length))
    }

    /// Every note at `pitch` overlapping `[at, at + length)`.
    fn overlapping(&self, pitch: Pitch, at: Tick, length: Length) -> Vec<(Position, Length)> {
        let end = at.0.saturating_add(u16::from(length.0));
        let window = window(Position { tick: at, pitch });
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
    /// two notes of the same pitch sounding at once in one pattern isn't a sound, it's an
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

/// The narrowest range of the map that could contain anything covering `from` — bounded by
/// `Length::MAX`, since nothing sounding for longer than that could still start further back.
fn window(from: Position) -> std::ops::RangeInclusive<Position> {
    let earliest = Tick(
        from.tick
            .0
            .saturating_sub(u16::from(Length::MAX.0.saturating_sub(1))),
    );
    let lower = Position {
        tick: earliest,
        pitch: Pitch(u8::MIN),
    };
    let upper = Position {
        tick: from.tick,
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
    fn empty_pattern_has_no_notes_sounding() {
        let pattern = Pattern::new(Tick(16));
        assert_eq!(pattern.sounding_at(Tick(0)).count(), 0);
    }

    #[test]
    fn sounding_at_finds_a_chord() {
        let mut pattern = Pattern::new(Tick(16));
        pattern.insert(at(0, 60), Length(4));
        pattern.insert(at(0, 64), Length(4));

        assert_eq!(pattern.sounding_at(Tick(2)).count(), 2);
    }

    #[test]
    fn sounding_at_excludes_notes_outside_their_span() {
        let mut pattern = Pattern::new(Tick(16));
        pattern.insert(at(0, 60), Length(4));

        assert_eq!(pattern.sounding_at(Tick(4)).count(), 0); // [0, 4) — 4 is just past the end
    }

    #[test]
    fn inserting_over_a_same_pitch_note_replaces_it() {
        let mut pattern = Pattern::new(Tick(16));
        pattern.insert(at(0, 60), Length(8));

        let removed = pattern.insert(at(2, 60), Length(2));

        assert_eq!(removed, vec![(at(0, 60), Length(8))]);
        assert_eq!(pattern.sounding_at(Tick(0)).count(), 0); // the old note is gone
        assert_eq!(pattern.sounding_at(Tick(2)).count(), 1); // the new one took its place
    }

    #[test]
    fn different_pitches_never_conflict() {
        let mut pattern = Pattern::new(Tick(16));
        pattern.insert(at(0, 60), Length(8));

        let removed = pattern.insert(at(0, 61), Length(8));
        assert!(removed.is_empty());
    }
}
