//! A selectable span of one track's grid.

use crate::pitch::Pitch;
use crate::time::{Length, Tick};
use crate::track::Position;

/// One pitch row, spanning ticks from `anchor` to `head` (either order — extending "backward" is
/// just `head < anchor`).
///
/// This is the spreadsheet sense of "range" (as in `A1:C5`), not a text editor's linear span:
/// there's no document order across pitches for "everything between two points" to mean, so a
/// range never crosses rows. A chord is several independent ranges (one per pitch, i.e.
/// multi-cursor), not one range spanning them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// The row this range lives on.
    pub pitch: Pitch,
    /// Where the range was started from.
    pub anchor: Tick,
    /// The moving end.
    pub head: Tick,
}

impl Range {
    /// A single-tick range at `position`, collapsed (`anchor == head`).
    #[must_use]
    pub const fn at(position: Position) -> Self {
        Self {
            pitch: position.pitch,
            anchor: position.tick,
            head: position.tick,
        }
    }

    /// The earliest tick the range covers.
    #[must_use]
    pub fn start(self) -> Tick {
        self.anchor.min(self.head)
    }

    /// The latest tick the range covers.
    #[must_use]
    pub fn end(self) -> Tick {
        self.anchor.max(self.head)
    }

    /// How many ticks the range spans, inclusive of both ends — this is what sets a new note's
    /// length when inserting at this range.
    #[must_use]
    pub fn length(self) -> Length {
        let span = self.end().0 - self.start().0 + 1;
        #[allow(clippy::cast_possible_truncation)] // clamped to u8's range first
        let ticks = span.min(u16::from(u8::MAX)) as u8;
        Length(ticks)
    }

    /// The grid coordinate this range starts from.
    #[must_use]
    pub fn position(self) -> Position {
        Position {
            tick: self.start(),
            pitch: self.pitch,
        }
    }

    /// Whether `self` and `other` overlap or touch on the same pitch (mergeable into one range).
    #[must_use]
    pub fn touches(self, other: Self) -> bool {
        self.pitch == other.pitch
            && self.start().0 <= other.end().0.saturating_add(1)
            && other.start().0 <= self.end().0.saturating_add(1)
    }

    /// The smallest range covering both `self` and `other`.
    ///
    /// Only meaningful when [`Range::touches`] holds — merging two unrelated ranges would silently
    /// pull in whatever lies between them.
    #[must_use]
    pub fn merged(self, other: Self) -> Self {
        Self {
            pitch: self.pitch,
            anchor: self.start().min(other.start()),
            head: self.end().max(other.end()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collapsed_range_has_length_one() {
        let range = Range::at(Position {
            tick: Tick(3),
            pitch: Pitch(60),
        });
        assert_eq!(range.length(), Length(1));
    }

    #[test]
    fn length_is_inclusive_of_both_ends_regardless_of_direction() {
        let forward = Range {
            pitch: Pitch(60),
            anchor: Tick(2),
            head: Tick(5),
        };
        let backward = Range {
            pitch: Pitch(60),
            anchor: Tick(5),
            head: Tick(2),
        };
        assert_eq!(forward.length(), Length(4));
        assert_eq!(backward.length(), Length(4));
    }

    #[test]
    fn adjacent_ranges_touch_but_distant_ones_do_not() {
        let first = Range {
            pitch: Pitch(60),
            anchor: Tick(0),
            head: Tick(2),
        };
        let adjacent = Range {
            pitch: Pitch(60),
            anchor: Tick(3),
            head: Tick(4),
        };
        let distant = Range {
            pitch: Pitch(60),
            anchor: Tick(10),
            head: Tick(11),
        };
        assert!(first.touches(adjacent));
        assert!(!first.touches(distant));
    }

    #[test]
    fn ranges_on_different_pitches_never_touch() {
        let low = Range {
            pitch: Pitch(60),
            anchor: Tick(0),
            head: Tick(5),
        };
        let high = Range {
            pitch: Pitch(61),
            anchor: Tick(0),
            head: Tick(5),
        };
        assert!(!low.touches(high));
    }

    #[test]
    fn merging_spans_both_ranges_fully() {
        let first = Range {
            pitch: Pitch(60),
            anchor: Tick(0),
            head: Tick(2),
        };
        let second = Range {
            pitch: Pitch(60),
            anchor: Tick(1),
            head: Tick(5),
        };
        let merged = first.merged(second);
        assert_eq!((merged.start(), merged.end()), (Tick(0), Tick(5)));
    }
}
