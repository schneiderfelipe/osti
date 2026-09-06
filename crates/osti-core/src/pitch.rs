//! Musical pitch.

/// A pitch, as a MIDI note number (0..=127; A4 is 69, 440 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

impl Pitch {
    /// A4, 440 Hz — the reference pitch tuning is built from.
    pub const A4: Self = Self(69);

    /// The pitch's frequency, in Hz.
    #[must_use]
    pub fn frequency_hz(self) -> f64 {
        440.0 * ((f64::from(self.0) - 69.0) / 12.0).exp2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440_hz() {
        assert!((Pitch::A4.frequency_hz() - 440.0).abs() < 1e-9);
    }

    #[test]
    fn one_octave_up_doubles_the_frequency() {
        let octave_up = Pitch(Pitch::A4.0 + 12);
        assert!((octave_up.frequency_hz() - 880.0).abs() < 1e-9);
    }
}
