//! Time, measured in ticks.

/// A position in time within a pattern, measured in ticks.
///
/// Wraps at the pattern's own length — like a clock face, a tick past the end is the same
/// position on the next lap, not a new one. A future song-wide absolute tick (once arrangement
/// exists) would be a different, non-wrapping thing, not this type reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u16);

/// A span of time, measured in ticks — a note's duration, or a whole pattern's loop length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Length(pub u8);

impl Length {
    /// The longest a single span can be.
    ///
    /// Also the widest window a "what's sounding right now" search ever needs to look backward —
    /// nothing can still be sounding from further back than this and still cover the tick being
    /// asked about.
    pub const MAX: Self = Self(u8::MAX);
}
