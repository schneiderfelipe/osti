//! Audio I/O for osti.
//!
//! Owns talking to the audio device.

use std::fmt;

use cpal::{
    Sample, SampleFormat, SizedSample, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

// Fill a buffer with silence: set every sample to its format's equilibrium value.
fn fill_silence<T: Sample>(data: &mut [T]) {
    for sample in data.iter_mut() {
        *sample = T::EQUILIBRIUM;
    }
}

// Build and start a silent output stream using sample type `T`.
fn build_and_play<T: SizedSample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
) -> Result<Stream, cpal::Error> {
    let err_fn = |err| eprintln!("audio stream error: {err}");
    let stream =
        device.build_output_stream(config, |data: &mut [T], _| fill_silence(data), err_fn, None)?;
    stream.play()?;
    Ok(stream)
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

/// Start a looping silent output stream on the default device.
///
/// # Errors
///
/// Returns an error if there is no default output device, or the stream cannot be configured,
/// built, or started.
pub fn play_silence() -> Result<SilentLoop, Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Error::NoOutputDevice)?;
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    let stream = match sample_format {
        SampleFormat::F32 => build_and_play::<f32>(&device, config),
        SampleFormat::I16 => build_and_play::<i16>(&device, config),
        SampleFormat::U16 => build_and_play::<u16>(&device, config),
        other => return Err(Error::UnsupportedSampleFormat(other)),
    }?;

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
