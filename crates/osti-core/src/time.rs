//! Time, measured in ticks.

/// A position in time on a track, measured in ticks.
///
/// Plain and absolute for now — no wrapping, no loop of its own. Looping (even partial: looping
/// just a region of a track some number of times) is real future work, but it needs its own
/// concept (a loop region, a repeat count) rather than folding into what a tick means; not
/// building that until there's an actual design for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u16);

/// A span of time, measured in ticks — a note's duration.
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
