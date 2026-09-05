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

/// Build the tone and gate-phase signals for a given sample rate.
fn signals(sample_rate: f64) -> (Sine<ConstHz>, Phase<ConstHz>) {
    let tone = rate(sample_rate).const_hz(FREQUENCY_HZ).sine();
    let gate_phase = rate(sample_rate).const_hz(1.0 / PERIOD_SECS).phase();
    (tone, gate_phase)
}

// Fill a buffer of interleaved frames from `tone`, muted whenever `gate_phase`'s fractional
// cycle position (which wraps every step) falls outside the duty cycle. `tone` and `gate_phase`
// are stepped once per frame, not once per sample, so multi-channel output isn't sped up; every
// channel of a frame gets the same value. `tone` is always stepped, even while muted, so its
// pitch stays accurate to real elapsed time. Reports whether the note was on by the end of the
// buffer, or the loop's prior state if the buffer had no frames (or `channels` is zero).
fn fill_note<T: Sample + FromSample<f64>>(
    data: &mut [T],
    channels: usize,
    tone: &mut Sine<ConstHz>,
    gate_phase: &mut Phase<ConstHz>,
    note_on: &mut bool,
) {
    // `chunks_mut` panics on a zero chunk size; a device reporting zero channels shouldn't crash
    // the audio thread over it.
    if channels == 0 {
        return;
    }
    for frame in data.chunks_mut(channels) {
        let tone_value = tone.next();
        *note_on = gate_phase.next() < DUTY;
        let value = if *note_on {
            T::from_sample(tone_value)
        } else {
            T::EQUILIBRIUM
        };
        frame.fill(value);
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
    let (mut tone, mut gate_phase) = signals(sample_rate);
    let mut on = GATE_STARTS_ON;
    let err_fn = {
        let note_on = Arc::clone(&note_on);
        move |err| handle_stream_error(&err, &note_on)
    };
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            // Safe to call unconditionally: fill_note's loop is a no-op on an empty slice, which
            // leaves `on` at its last value, exactly the desired behavior.
            fill_note(data, channels, &mut tone, &mut gate_phase, &mut on);
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
    use std::sync::atomic::{AtomicBool, Ordering};

    use dasp_signal::{ConstHz, Phase, Signal, Sine};

    use super::{DUTY, PERIOD_SECS, fill_note, handle_stream_error, signals};

    const SAMPLE_RATE: f64 = 44_100.0;

    // fill_note takes concrete signal types, so these stay concrete too (an `impl Signal` return
    // wouldn't satisfy that parameter type).
    fn tone() -> Sine<ConstHz> {
        signals(SAMPLE_RATE).0
    }

    fn gate_phase() -> Phase<ConstHz> {
        signals(SAMPLE_RATE).1
    }

    #[test]
    // Comparing against a hardcoded EQUILIBRIUM, not a computed value, so exactness is correct.
    #[allow(clippy::float_cmp)]
    fn fill_note_writes_silence_when_off() {
        let mut buffer = [1.0_f32; 4];
        let mut gate_phase = gate_phase();
        // Step past the duty cycle's end, with a small margin against rounding at the boundary.
        // Small, known-non-negative values, so the truncation is exact and the sign is moot.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps_past_duty_cycle = (SAMPLE_RATE * PERIOD_SECS * DUTY) as u64 + 10;
        for _ in 0..steps_past_duty_cycle {
            gate_phase.next();
        }
        let mut on = true;

        fill_note(&mut buffer, 1, &mut tone(), &mut gate_phase, &mut on);

        assert!(!on);
        assert!(buffer.iter().all(|&s| s == 0.0));
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

    #[test]
    fn fill_note_does_not_panic_on_zero_channels() {
        let mut buffer = [0.0_f32; 4];
        let mut on = true;

        fill_note(&mut buffer, 0, &mut tone(), &mut gate_phase(), &mut on);

        assert!(on, "left untouched, same as an empty buffer");
    }

    #[test]
    fn fill_note_converts_the_tone_to_i16_samples_when_on() {
        let mut buffer = [0_i16; 4];
        let mut on = false;

        fill_note(&mut buffer, 1, &mut tone(), &mut gate_phase(), &mut on);

        assert!(on);
        assert!(buffer.iter().any(|&s| s != 0));
    }

    #[test]
    // Both channels are written from the very same computed value, so exactness is correct.
    #[allow(clippy::float_cmp)]
    fn fill_note_writes_every_channel_of_a_frame_alike() {
        let mut buffer = [0.0_f32; 8]; // 4 stereo frames
        let mut on = false;

        fill_note(&mut buffer, 2, &mut tone(), &mut gate_phase(), &mut on);

        for frame in buffer.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn fill_note_steps_the_gate_once_per_frame_not_per_sample() {
        // A gate that stepped once per interleaved sample instead of once per frame would be
        // twice as far through its cycle after the same number of stereo frames.
        let mut mono = [0.0_f32; 4];
        let mut mono_on = false;
        fill_note(&mut mono, 1, &mut tone(), &mut gate_phase(), &mut mono_on);

        let mut stereo = [0.0_f32; 8];
        let mut stereo_on = false;
        fill_note(
            &mut stereo,
            2,
            &mut tone(),
            &mut gate_phase(),
            &mut stereo_on,
        );

        assert_eq!(mono_on, stereo_on);
    }

    #[test]
    fn fill_note_keeps_toggling_across_many_cycles() {
        let mut buffer = [0.0_f32; 512];
        let mut tone = tone();
        let mut gate_phase = gate_phase();
        let mut on = false;

        // A handful of cycles is enough to see both states; dasp_signal's Phase wraps every step
        // by construction; this just confirms our own glue code passes that through correctly.
        let mut saw_on = false;
        let mut saw_off = false;
        for _ in 0..300 {
            fill_note(&mut buffer, 1, &mut tone, &mut gate_phase, &mut on);
            if on {
                saw_on = true;
            } else {
                saw_off = true;
            }
        }

        assert!(saw_on);
        assert!(saw_off);
    }
}
