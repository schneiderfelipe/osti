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
use dasp_signal::{ConstHz, Phase, Signal, Sine, rate};

/// Pitch of the looping note, in Hz (A4).
const FREQUENCY_HZ: f64 = 440.0;

/// How long one on-then-off cycle of the note takes, in seconds.
const PERIOD_SECS: f64 = 0.8;

/// Fraction of each cycle the note is audible for, starting each cycle.
const DUTY: f64 = 0.5;

/// Whether the gate is on at the very start of a cycle, i.e. at phase 0.0.
const GATE_STARTS_ON: bool = 0.0 < DUTY;

/// How long an on/off transition takes to fade, instead of switching instantly.
///
/// The gate flips at a fixed point in time, unrelated to where the tone's waveform happens to be;
/// jumping straight to/from silence there is an amplitude discontinuity, heard as a click. A few
/// milliseconds of fade removes the discontinuity without being long enough to blur the rhythm.
const RAMP_SECS: f64 = 0.005;

/// Build the tone signal for a given sample rate.
fn tone(sample_rate: f64) -> Sine<ConstHz> {
    rate(sample_rate).const_hz(FREQUENCY_HZ).sine()
}

/// Build the gate-phase signal for a given sample rate.
fn gate_phase(sample_rate: f64) -> Phase<ConstHz> {
    rate(sample_rate).const_hz(1.0 / PERIOD_SECS).phase()
}

/// The oscillator, its on/off gate, and the fade level between them, stepped one frame at a time.
struct NoteState {
    tone: Sine<ConstHz>,
    gate_phase: Phase<ConstHz>,
    /// How much `level` moves toward the gate's target each frame.
    ramp_step: f64,
    /// Current fade level, between 0.0 (silent) and 1.0 (the gate's full-on amplitude).
    level: f64,
    note_on: bool,
}

impl NoteState {
    /// Build a note that starts silent, fading in as soon as the gate first turns on.
    fn new(sample_rate: f64) -> Self {
        Self {
            tone: tone(sample_rate),
            gate_phase: gate_phase(sample_rate),
            ramp_step: 1.0 / (RAMP_SECS * sample_rate),
            level: 0.0,
            note_on: GATE_STARTS_ON,
        }
    }

    // Fill a buffer of interleaved frames from the tone, scaled by `level`, which chases the
    // gate's on/off target by at most `ramp_step` each frame rather than jumping straight to it
    // (see `RAMP_SECS`). The tone and gate are stepped once per frame, not once per sample, so
    // multi-channel output isn't sped up; every channel of a frame gets the same value. The tone
    // is always stepped, even while muted, so its pitch stays accurate to real elapsed time.
    // Returns whether the gate was on by the end of the buffer, or its prior state if the buffer
    // had no frames (or `channels` is zero).
    fn fill<T: Sample + FromSample<f64>>(&mut self, data: &mut [T], channels: usize) -> bool {
        // `chunks_mut` panics on a zero chunk size; a device reporting zero channels shouldn't
        // crash the audio thread over it.
        if channels != 0 {
            for frame in data.chunks_mut(channels) {
                let tone_value = self.tone.next();
                self.note_on = self.gate_phase.next() < DUTY;
                let target = if self.note_on { 1.0 } else { 0.0 };
                self.level = if self.level < target {
                    (self.level + self.ramp_step).min(target)
                } else {
                    (self.level - self.ramp_step).max(target)
                };
                frame.fill(T::from_sample(tone_value * self.level));
            }
        }
        self.note_on
    }
}

// Log a stream error and mark the note off, since the stream may never call the data callback
// again afterward to report the truth itself.
fn handle_stream_error(err: &cpal::Error, note_on: &AtomicBool) {
    eprintln!("audio stream error: {err}");
    note_on.store(false, Ordering::Relaxed);
}

// Build and start a stream that loops the note, reporting its on/off state through `note_on`.
fn build_and_play<T: SizedSample + FromSample<f64>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    note_on: Arc<AtomicBool>,
) -> Result<Stream, cpal::Error> {
    let sample_rate = f64::from(config.sample_rate);
    let channels = usize::from(config.channels);
    let mut note = NoteState::new(sample_rate);
    let err_fn = {
        let note_on = Arc::clone(&note_on);
        move |err| handle_stream_error(&err, &note_on)
    };
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let on = note.fill(data, channels);
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
    /// Return whether the note is audible right now.
    ///
    /// Updated once per audio buffer, not per sample (plenty precise for anything watching it
    /// at UI-frame granularity).
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

    let note_on = Arc::new(AtomicBool::new(GATE_STARTS_ON));
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
    use super::*;

    const SAMPLE_RATE: f64 = 44_100.0;

    #[test]
    fn fill_does_not_panic_on_zero_channels() {
        let mut note = NoteState::new(SAMPLE_RATE);
        let mut buffer = [0.0_f32; 4];

        let on = note.fill(&mut buffer, 0);

        assert_eq!(
            on, GATE_STARTS_ON,
            "left untouched, same as an empty buffer"
        );
    }

    #[test]
    fn fill_steps_the_gate_once_per_frame_not_per_sample() {
        // A gate that stepped once per interleaved sample instead of once per frame would be
        // twice as far through its cycle after the same number of stereo frames as mono ones.
        let mut mono = NoteState::new(SAMPLE_RATE);
        let mut mono_buffer = [0.0_f32; 4];
        let mono_on = mono.fill(&mut mono_buffer, 1);

        let mut stereo = NoteState::new(SAMPLE_RATE);
        let mut stereo_buffer = [0.0_f32; 8]; // 4 stereo frames
        let stereo_on = stereo.fill(&mut stereo_buffer, 2);

        assert_eq!(mono_on, stereo_on);
    }

    #[test]
    // Computed from exact binary fractions (1.0 and a power-of-two-friendly step), so equality
    // holds precisely.
    #[allow(clippy::float_cmp)]
    fn fill_ramps_the_level_instead_of_cutting_it_when_the_gate_turns_off() {
        let mut note = NoteState {
            tone: tone(SAMPLE_RATE),
            gate_phase: gate_phase(SAMPLE_RATE),
            ramp_step: 0.25,
            level: 1.0,
            note_on: true,
        };
        // Push the gate right up to the duty cycle's end, so the very next frame turns it off.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps_to_duty_cycle = (SAMPLE_RATE * PERIOD_SECS * DUTY) as u64;
        for _ in 0..steps_to_duty_cycle {
            note.gate_phase.next();
        }

        note.fill(&mut [0.0_f32; 1], 1);

        // A hard cut would drop `level` straight to 0.0; ramping moves it by one step instead.
        assert!(!note.note_on);
        assert_eq!(note.level, 1.0 - note.ramp_step);
    }

    #[test]
    fn fill_settles_to_silence_once_ramped_off() {
        let mut note = NoteState::new(SAMPLE_RATE);
        let mut buffer = [1_i16; 1];

        // Run through the on phase (long enough to fully ramp in) and past the end of the
        // following off phase's ramp, with a small margin for rounding at each boundary.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let on_frames = (SAMPLE_RATE * PERIOD_SECS * DUTY) as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ramp_frames = (SAMPLE_RATE * RAMP_SECS) as u64;
        for _ in 0..(on_frames + ramp_frames + 10) {
            note.fill(&mut buffer, 1);
        }

        assert!(!note.note_on);
        assert_eq!(buffer, [0]);
    }

    #[test]
    fn handle_stream_error_marks_the_note_off() {
        let note_on = AtomicBool::new(true);

        handle_stream_error(
            &cpal::Error::new(cpal::ErrorKind::DeviceNotAvailable),
            &note_on,
        );

        assert!(!note_on.load(Ordering::Relaxed));
    }
}
