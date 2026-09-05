//! Audio I/O for `osti`.
//!
//! Owns talking to the audio device.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cpal::{
    FromSample, Sample, SampleFormat, SizedSample, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use dasp_signal::{Signal, rate};

/// Pitch of the looping note, in Hz (A4).
const FREQUENCY_HZ: f64 = 440.0;

/// How long one on-then-off cycle of the note takes, in seconds.
const PERIOD_SECS: f64 = 0.8;

/// Fraction of each cycle the note is audible for, starting each cycle.
const DUTY: f64 = 0.5;

// Fill a buffer from `tone`, muted whenever `gate_phase`'s fractional cycle position (which
// wraps every step) falls outside the duty cycle. `tone` is always stepped, even while muted, so
// its pitch stays accurate to real elapsed time. Reports whether the note was on by the end of
// the buffer.
fn fill_note<T, O, G>(data: &mut [T], tone: &mut O, gate_phase: &mut G) -> bool
where
    T: Sample + FromSample<f64>,
    O: Signal<Frame = f64>,
    G: Signal<Frame = f64>,
{
    let mut note_on = false;
    for sample in data.iter_mut() {
        let tone_value = tone.next();
        note_on = gate_phase.next() < DUTY;
        *sample = if note_on {
            T::from_sample(tone_value)
        } else {
            T::EQUILIBRIUM
        };
    }
    note_on
}

// Build and start a stream that loops the note, reporting its on/off state through `note_on`.
fn build_and_play<T: SizedSample + FromSample<f64>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    note_on: Arc<AtomicBool>,
) -> Result<Stream, cpal::Error> {
    let sample_rate = f64::from(config.sample_rate);
    let mut tone = rate(sample_rate).const_hz(FREQUENCY_HZ).sine();
    let mut gate_phase = rate(sample_rate).const_hz(1.0 / PERIOD_SECS).phase();
    let err_fn = |err| eprintln!("audio stream error: {err}");
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let on = fill_note(data, &mut tone, &mut gate_phase);
            note_on.store(on, Ordering::Relaxed);
        },
        err_fn,
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

/// A single note, looping continuously on and off, and the audio thread's stream backing it.
///
/// Dropping this stops playback.
#[must_use = "the loop stops playing as soon as this is dropped"]
pub struct NoteLoop {
    // Held only for its `Drop` side effect: stops the stream once nothing needs it anymore.
    _stream: Stream,
    note_on: Arc<AtomicBool>,
}

impl NoteLoop {
    /// Whether the note is audible right now.
    ///
    /// Updated once per audio buffer, not per sample — plenty precise for anything watching it
    /// at UI-frame granularity.
    #[must_use]
    pub fn is_note_on(&self) -> bool {
        self.note_on.load(Ordering::Relaxed)
    }
}

impl fmt::Debug for NoteLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoteLoop").finish_non_exhaustive()
    }
}

/// An error starting the note loop.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No default output device is available.
    #[error("no default output device is available")]
    NoOutputDevice,

    /// The sample format the device reported isn't one this crate knows how to fill.
    #[error("unsupported sample format: {0}")]
    UnsupportedSampleFormat(SampleFormat),

    /// cpal reported an error querying the device, building the stream, or starting it.
    #[error(transparent)]
    Cpal(#[from] cpal::Error),
}

/// Start a single note, looping audibly on and off, on the default output device.
///
/// # Errors
///
/// Returns an error if there is no default output device, or the stream cannot be configured,
/// built, or started.
pub fn play_looping_note() -> Result<NoteLoop, Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Error::NoOutputDevice)?;
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    let note_on = Arc::new(AtomicBool::new(false));
    let stream = match sample_format {
        SampleFormat::F32 => build_and_play::<f32>(&device, config, Arc::clone(&note_on)),
        SampleFormat::I16 => build_and_play::<i16>(&device, config, Arc::clone(&note_on)),
        SampleFormat::U16 => build_and_play::<u16>(&device, config, Arc::clone(&note_on)),
        other => return Err(Error::UnsupportedSampleFormat(other)),
    }?;

    Ok(NoteLoop {
        _stream: stream,
        note_on,
    })
}

#[cfg(test)]
mod tests {
    use dasp_signal::{Signal, rate};

    use super::{DUTY, FREQUENCY_HZ, PERIOD_SECS, fill_note};

    const SAMPLE_RATE: f64 = 44_100.0;

    #[test]
    fn fill_note_reports_on_at_the_start_of_a_cycle() {
        let mut buffer = [0.0_f32; 4];
        let mut tone = rate(SAMPLE_RATE).const_hz(FREQUENCY_HZ).sine();
        let mut gate_phase = rate(SAMPLE_RATE).const_hz(1.0 / PERIOD_SECS).phase();

        assert!(fill_note(&mut buffer, &mut tone, &mut gate_phase));
    }

    #[test]
    // Comparing against a hardcoded EQUILIBRIUM, not a computed value, so exactness is correct.
    #[allow(clippy::float_cmp)]
    fn fill_note_writes_silence_when_off() {
        let mut buffer = [1.0_f32; 4];
        let mut tone = rate(SAMPLE_RATE).const_hz(FREQUENCY_HZ).sine();
        let mut gate_phase = rate(SAMPLE_RATE).const_hz(1.0 / PERIOD_SECS).phase();
        // Step past the duty cycle's end, with a small margin against rounding at the boundary.
        // Small, known-non-negative values, so the truncation is exact and the sign is moot.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps_past_duty_cycle = (SAMPLE_RATE * PERIOD_SECS * DUTY) as u64 + 10;
        for _ in 0..steps_past_duty_cycle {
            gate_phase.next();
        }

        fill_note(&mut buffer, &mut tone, &mut gate_phase);

        assert!(buffer.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn fill_note_keeps_toggling_across_many_cycles() {
        let mut buffer = [0.0_f32; 512];
        let mut tone = rate(SAMPLE_RATE).const_hz(FREQUENCY_HZ).sine();
        let mut gate_phase = rate(SAMPLE_RATE).const_hz(1.0 / PERIOD_SECS).phase();

        // ~460 simulated seconds across many buffers: an accumulator that drifted or stalled
        // (the class of bug a hand-rolled one once had) would eventually stop toggling.
        let mut saw_on = false;
        let mut saw_off = false;
        for _ in 0..40_000 {
            if fill_note(&mut buffer, &mut tone, &mut gate_phase) {
                saw_on = true;
            } else {
                saw_off = true;
            }
        }

        assert!(saw_on);
        assert!(saw_off);
    }
}
