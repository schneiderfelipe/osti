//! Audio I/O for `osti`.
//!
//! Owns talking to the audio device.

use std::{
    f32::consts::TAU,
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

/// Pitch of the looping note, in Hz (A4).
const FREQUENCY_HZ: f32 = 440.0;

/// How long one on-then-off cycle of the note takes, in seconds.
const PERIOD_SECS: f32 = 0.8;

/// Fraction of `PERIOD_SECS` the note is audible for, starting each cycle.
const DUTY: f32 = 0.5;

// Whether the note is audible at the given number of seconds since playback started.
fn is_note_on(elapsed_secs: f32) -> bool {
    elapsed_secs.rem_euclid(PERIOD_SECS) < PERIOD_SECS * DUTY
}

// The note's waveform value at the given number of seconds since playback started.
fn note_sample(elapsed_secs: f32) -> f32 {
    (elapsed_secs * FREQUENCY_HZ * TAU).sin()
}

// Fill a buffer with the looping note, advancing `elapsed_secs` by one sample each step and
// reporting whether the note was on by the end of the buffer.
fn fill_note<T: Sample + FromSample<f32>>(
    data: &mut [T],
    elapsed_secs: &mut f32,
    sample_rate: f32,
) -> bool {
    let mut note_on = false;
    for sample in data.iter_mut() {
        note_on = is_note_on(*elapsed_secs);
        *sample = if note_on {
            T::from_sample(note_sample(*elapsed_secs))
        } else {
            T::EQUILIBRIUM
        };
        *elapsed_secs += 1.0 / sample_rate;
    }
    note_on
}

// Build and start a stream that loops the note, reporting its on/off state through `note_on`.
fn build_and_play<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    note_on: Arc<AtomicBool>,
) -> Result<Stream, cpal::Error> {
    #[allow(clippy::cast_precision_loss)]
    let sample_rate = config.sample_rate as f32;
    let mut elapsed_secs = 0.0;
    let err_fn = |err| eprintln!("audio stream error: {err}");
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let on = fill_note(data, &mut elapsed_secs, sample_rate);
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
    use super::{DUTY, PERIOD_SECS, fill_note, is_note_on};

    #[test]
    fn note_is_on_at_the_start_of_a_cycle() {
        assert!(is_note_on(0.0));
        assert!(is_note_on(PERIOD_SECS * DUTY * 0.5));
    }

    #[test]
    fn note_is_off_in_the_second_half_of_a_cycle() {
        assert!(!is_note_on(PERIOD_SECS * DUTY));
        assert!(!is_note_on(PERIOD_SECS * 0.99));
    }

    #[test]
    fn note_cycles_repeat() {
        assert_eq!(is_note_on(0.1), is_note_on(0.1 + PERIOD_SECS));
    }

    #[test]
    fn fill_note_reports_on_when_the_buffer_ends_on() {
        let mut buffer = [0.0_f32; 4];
        let mut elapsed_secs = 0.0;
        assert!(fill_note(&mut buffer, &mut elapsed_secs, 44_100.0));
    }

    #[test]
    // Comparing against a hardcoded EQUILIBRIUM, not a computed value, so exactness is correct.
    #[allow(clippy::float_cmp)]
    fn fill_note_writes_silence_when_off() {
        let mut buffer = [1.0_f32; 4];
        let mut elapsed_secs = PERIOD_SECS * DUTY;
        fill_note(&mut buffer, &mut elapsed_secs, 44_100.0);
        assert!(buffer.iter().all(|&s| s == 0.0));
    }
}
