//! Audio I/O for osti.
//!
//! This crate owns talking to the audio device. The buffer-filling logic that decides what to
//! play is a small, pure function kept separate from the cpal glue that wires it into a real
//! output stream, so it can be unit tested with no audio device present.

use std::fmt;

use cpal::{
    Sample, SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

/// Fills a buffer with silence: every sample is set to its format's equilibrium value.
pub fn fill_silence<T: Sample>(data: &mut [T]) {
    for sample in data.iter_mut() {
        *sample = T::EQUILIBRIUM;
    }
}

/// A continuously looping silent audio stream.
///
/// Dropping this stops playback.
#[must_use = "the loop stops playing as soon as this is dropped"]
pub struct SilentLoop {
    // Held only for its `Drop` side effect: stops the stream once nothing needs it anymore.
    _stream: Stream,
}

impl fmt::Debug for SilentLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SilentLoop").finish_non_exhaustive()
    }
}

/// An error starting the silent audio loop.
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

/// Starts playing silence on the system's default output device, in a loop, for as long as the
/// returned [`SilentLoop`] is kept alive.
///
/// # Errors
///
/// Returns an error if there is no default output device, its configuration cannot be read, or
/// the stream cannot be built or started. A missing output device is common in headless or
/// sandboxed environments and should be reported rather than treated as fatal.
pub fn play_silence() -> Result<SilentLoop, Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Error::NoOutputDevice)?;
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    let err_fn = |err| eprintln!("audio stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            config,
            |data: &mut [f32], _| fill_silence(data),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            config,
            |data: &mut [i16], _| fill_silence(data),
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            config,
            |data: &mut [u16], _| fill_silence(data),
            err_fn,
            None,
        ),
        other => return Err(Error::UnsupportedSampleFormat(other)),
    }?;

    stream.play()?;

    Ok(SilentLoop { _stream: stream })
}

#[cfg(test)]
mod tests {
    use super::fill_silence;

    #[test]
    // Comparing against a hardcoded EQUILIBRIUM, not a computed value, so exactness is correct.
    #[allow(clippy::float_cmp)]
    fn fills_f32_buffer_with_silence() {
        let mut buffer = [1.0_f32, -1.0, 0.5];
        fill_silence(&mut buffer);
        assert_eq!(buffer, [0.0; 3]);
    }

    #[test]
    fn fills_i16_buffer_with_silence() {
        let mut buffer = [i16::MIN, i16::MAX, 42];
        fill_silence(&mut buffer);
        assert_eq!(buffer, [0; 3]);
    }
}
