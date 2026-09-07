//! A non-empty set of ranges, one primary — Helix's multi-cursor model.

use nonempty::NonEmpty;

use crate::range::Range;

/// One or more [`Range`]s, always at least one — there's no such thing as an empty selection,
/// there's always a cursor somewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection(NonEmpty<Range>);

impl Selection {
    /// Build a selection containing just this one range.
    #[must_use]
    pub const fn single(range: Range) -> Self {
        Self(NonEmpty::new(range))
    }

    /// Return the primary range: the one shown distinctly, and the one a future viewport would
    /// keep scrolled into view.
    #[must_use]
    pub const fn primary(&self) -> Range {
        *self.0.first()
    }

    /// Iterate every range in the selection.
    pub fn ranges(&self) -> impl Iterator<Item = Range> + '_ {
        self.0.iter().copied()
    }

    /// Move every range the same way — how a movement key affects every cursor in a multi-cursor
    /// selection at once, not just the primary one. Takes `&self`, not `self`: every `Range` is
    /// `Copy`, so there's nothing building the result needs to consume — callers that only have a
    /// borrowed `Selection` (the overwhelmingly common case, one per keypress) don't need to
    /// clone it first just to call this.
    #[must_use]
    pub fn map(&self, mut f: impl FnMut(Range) -> Range) -> Self {
        Self(NonEmpty {
            head: f(self.0.head),
            tail: self.0.tail.iter().map(|&range| f(range)).collect(),
        })
    }

    /// Merge any ranges that overlap or touch on the same pitch.
    ///
    /// Applied whenever the selection changes (see `Editor`'s handling of `SetSelection`), so "no
    /// two ranges in a selection overlap" holds by construction, not by caller discipline. Takes
    /// `&self` for the same reason [`Selection::map`] does.
    ///
    /// # Panics
    ///
    /// Never, in practice — the one internal `unwrap` only fails if `self` were empty, which
    /// `Selection`'s own invariant rules out.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut ranges: Vec<Range> = self.0.iter().copied().collect();
        ranges.sort_by_key(|range| (range.pitch, range.start()));

        let mut iter = ranges.into_iter();
        #[allow(clippy::unwrap_used)] // `ranges` came from a non-empty selection
        let mut merged = NonEmpty::new(iter.next().unwrap());
        for range in iter {
            if merged.last().touches(range) {
                *merged.last_mut() = merged.last().merged(range);
            } else {
                merged.push(range);
            }
        }
        Self(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::Pitch;
    use crate::time::Tick;

    fn range(pitch: u8, anchor: u16, head: u16) -> Range {
        Range {
            pitch: Pitch(pitch),
            anchor: Tick(anchor),
            head: Tick(head),
        }
    }

    #[test]
    fn a_single_range_is_its_own_primary() {
        let selection = Selection::single(range(60, 0, 0));
        assert_eq!(selection.primary(), range(60, 0, 0));
    }

    #[test]
    fn normalizing_a_single_range_changes_nothing() {
        let selection = Selection::single(range(60, 0, 3));
        assert_eq!(
            selection.normalized().ranges().collect::<Vec<_>>(),
            selection.ranges().collect::<Vec<_>>()
        );
    }

    #[test]
    fn overlapping_ranges_merge_on_normalize() {
        // Built by hand — there's no public "add a range" yet, this is exactly the shape
        // `SetSelection` would construct once multi-cursor keybindings exist.
        let selection = Selection(NonEmpty::from((range(60, 0, 2), vec![range(60, 1, 4)])));

        let normalized: Vec<_> = selection.normalized().ranges().collect();
        assert_eq!(normalized, vec![range(60, 0, 4)]);
    }

    #[test]
    fn ranges_on_different_pitches_stay_separate() {
        let selection = Selection(NonEmpty::from((range(60, 0, 2), vec![range(61, 0, 2)])));
        assert_eq!(selection.normalized().ranges().count(), 2);
    }

    #[test]
    fn map_moves_every_range() {
        let selection = Selection(NonEmpty::from((range(60, 0, 0), vec![range(61, 0, 0)])));

        let moved = selection.map(|r| Range {
            head: Tick(r.head.0 + 1),
            anchor: Tick(r.anchor.0 + 1),
            ..r
        });

        assert_eq!(
            moved.ranges().collect::<Vec<_>>(),
            vec![range(60, 1, 1), range(61, 1, 1)]
        );
    }
}
