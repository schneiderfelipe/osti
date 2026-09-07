//! Musical pitch.

/// A pitch, as a MIDI note number (0..=127; A4 is 69, 440 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

impl Pitch {
    /// A4, 440 Hz — the reference pitch tuning is built from.
    pub const A4: Self = Self(69);

    /// Return the pitch's frequency, in Hz.
    #[must_use]
    pub fn frequency_hz(self) -> f64 {
        440.0 * ((f64::from(self.0) - 69.0) / 12.0).exp2()
    }

    /// Return the pitch's name, in scientific pitch notation (`"A4"`, `"C#5"`, ...) — MIDI's own
    /// convention, where middle C (60) is `C4`.
    #[must_use]
    pub fn name(self) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let octave = i16::from(self.0) / 12 - 1;
        let class = usize::from(self.0 % 12);
        format!("{}{octave}", NAMES[class])
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

    #[test]
    fn middle_c_is_named_c4() {
        assert_eq!(Pitch(60).name(), "C4");
    }

    #[test]
    fn a4_is_named_a4() {
        assert_eq!(Pitch::A4.name(), "A4");
    }

    #[test]
    fn sharps_and_the_lowest_octave_are_named_correctly() {
        assert_eq!(Pitch(61).name(), "C#4");
        assert_eq!(Pitch(0).name(), "C-1");
    }
}
