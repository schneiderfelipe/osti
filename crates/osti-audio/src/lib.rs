//! Audio I/O for `osti`.
//!
//! The audio thread holds its own [`Playback`], not a view onto the UI's — actions are
//! replicated onto it over a lock-free queue, applied through the same [`Playback::apply`] the
//! UI uses, rather than the two sides sharing memory or exchanging snapshots. Two independent
//! copies of one deterministic state machine, fed the same ordered actions, can't drift.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};

use cpal::{
    FromSample, Sample, SampleFormat, SizedSample, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use dasp_signal::{ConstHz, Signal, Sine, rate};
use osti_core::{Pitch, Playback, PlaybackAction, PlaybackIntent, Tick, TrackId};

/// How long one tick lasts, in seconds. Hardcoded for now — a future tempo control would make
/// this a `Transport` field instead of a constant, not a design question to resolve today.
const TICK_SECS: f64 = 0.05;

/// How long an on/off transition takes to fade, instead of switching instantly.
///
/// A note turns on or off at a tick boundary, unrelated to where its tone's waveform happens to
/// be; jumping straight to/from silence there is an amplitude discontinuity, heard as a click. A
/// few milliseconds of fade removes the discontinuity without being long enough to blur the
/// rhythm.
const RAMP_SECS: f64 = 0.005;

/// One currently-sounding (or fading-out) note on one track.
struct Voice {
    track: TrackId,
    pitch: Pitch,
    /// Which note this voice belongs to, identified by its own start — not just its pitch, so
    /// two back-to-back same-pitch notes (no gap between them) are two voices, not one: crossing
    /// from one note's `start` to the next's is a new note event and gets its own ramp-in, the
    /// same re-attack a gap between them would already cause, rather than silently sustaining
    /// through the boundary because the pitch alone still matches.
    start: Tick,
    tone: Sine<ConstHz>,
    /// Current fade level, between `0.0` (silent) and `1.0` (full amplitude).
    level: f64,
    /// Where `level` is heading — `1.0` while the note that spawned this voice is still
    /// sounding, `0.0` once it isn't (the voice is dropped once `level` catches up).
    target: f64,
}

/// The audio thread's own state: a [`Playback`] replica, the voices currently rendering it, and
/// the plumbing to stay in sync with the UI.
struct Player {
    playback: Playback,
    commands: rtrb::Consumer<PlaybackAction>,
    voices: Vec<Voice>,
    tick_bits: Arc<AtomicU16>,
    sample_rate: f64,
    ramp_step: f64,
    /// Fractional progress toward the next tick, in frames.
    frames_into_tick: f64,
}

impl Player {
    fn new(
        playback: Playback,
        commands: rtrb::Consumer<PlaybackAction>,
        tick_bits: Arc<AtomicU16>,
        sample_rate: f64,
    ) -> Self {
        Self {
            playback,
            commands,
            voices: Vec::new(),
            tick_bits,
            sample_rate,
            ramp_step: 1.0 / (RAMP_SECS * sample_rate),
            frames_into_tick: 0.0,
        }
    }

    /// Apply every action the UI has sent since the last buffer.
    fn drain_commands(&mut self) {
        while let Ok(action) = self.commands.pop() {
            self.playback.apply(&action);
        }
    }

    /// Advance the transport by one frame, returning whether it crossed into a new tick.
    fn advance_tick(&mut self) -> bool {
        if self.playback.transport.intent != PlaybackIntent::Playing {
            return false;
        }
        self.frames_into_tick += 1.0;
        let frames_per_tick = self.sample_rate * TICK_SECS;
        if self.frames_into_tick < frames_per_tick {
            return false;
        }
        self.frames_into_tick -= frames_per_tick;
        // Saturates rather than wraps: real looping (going back to an earlier tick on purpose)
        // is real future work with its own design (see `Tick`'s own docs) — silently wrapping
        // the whole timeline back to 0 here would look like that feature already existed. This
        // just stops advancing at the last representable tick instead.
        self.playback.transport.position =
            Tick(self.playback.transport.position.0.saturating_add(1));
        true
    }

    /// Reconcile `voices` with whatever's actually sounding right now, across every track.
    fn sync_voices(&mut self) {
        for voice in &mut self.voices {
            voice.target = 0.0;
        }
        // Nothing is "sounding" while paused — the transport isn't advancing, so a note that
        // happened to be sounding at the moment of pause must fade out (via the loop below
        // leaving every voice's target at the `0.0` just set above) rather than being reaffirmed
        // to `1.0` every buffer and ringing until playback resumes.
        if self.playback.transport.intent == PlaybackIntent::Playing {
            let tick = self.playback.transport.position;
            for (index, track) in self.playback.tracks.iter().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                // realistically far fewer than 256 tracks
                let track_id = TrackId(index as u8);
                for note in track.sounding_at(tick) {
                    let pitch = note.position.pitch;
                    let start = note.position.tick;
                    if let Some(voice) = self.voices.iter_mut().find(|voice| {
                        voice.track == track_id && voice.pitch == pitch && voice.start == start
                    }) {
                        voice.target = 1.0;
                    } else {
                        self.voices.push(Voice {
                            track: track_id,
                            pitch,
                            start,
                            tone: rate(self.sample_rate).const_hz(pitch.frequency_hz()).sine(),
                            level: 0.0,
                            target: 1.0,
                        });
                    }
                }
            }
        }
        self.voices
            .retain(|voice| voice.target > 0.0 || voice.level > 0.0);
    }

    /// Advance every voice by one sample-frame and mix them down.
    fn mix(&mut self) -> f64 {
        let mut sample = 0.0;
        for voice in &mut self.voices {
            let tone_value = voice.tone.next();
            voice.level = if voice.level < voice.target {
                (voice.level + self.ramp_step).min(voice.target)
            } else {
                (voice.level - self.ramp_step).max(voice.target)
            };
            sample = tone_value.mul_add(voice.level, sample);
        }
        sample
    }

    /// Fill a buffer of interleaved frames, mixing every currently-sounding voice across every
    /// track. Every channel of a frame gets the same value.
    fn fill<T: Sample + FromSample<f64>>(&mut self, data: &mut [T], channels: usize) {
        if channels == 0 {
            return;
        }
        self.drain_commands();
        self.sync_voices();
        for frame in data.chunks_mut(channels) {
            if self.advance_tick() {
                self.sync_voices();
            }
            let sample = self.mix();
            frame.fill(T::from_sample(sample));
        }
        self.tick_bits
            .store(self.playback.transport.position.0, Ordering::Relaxed);
    }
}

fn build_and_play<T: SizedSample + FromSample<f64>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    playback: Playback,
    commands: rtrb::Consumer<PlaybackAction>,
    tick_bits: Arc<AtomicU16>,
) -> Result<Stream, cpal::Error> {
    let sample_rate = f64::from(config.sample_rate);
    let channels = usize::from(config.channels);
    let mut player = Player::new(playback, commands, tick_bits, sample_rate);
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| player.fill(data, channels),
        |err| eprintln!("audio stream error: {err}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

/// The audio thread's replicated [`Playback`], and the stream backing it.
///
/// Dropping this stops playback.
#[must_use = "the stream stops as soon as this is dropped"]
pub struct PlaybackHandle {
    // Held only for its `Drop` side effect: stops the stream once nothing needs it anymore.
    _stream: Stream,
    tick_bits: Arc<AtomicU16>,
    commands: rtrb::Producer<PlaybackAction>,
}

impl PlaybackHandle {
    /// Return the transport's current position, as last published by the audio thread.
    #[must_use]
    pub fn current_tick(&self) -> Tick {
        Tick(self.tick_bits.load(Ordering::Relaxed))
    }

    /// Forward an action to the audio thread's own `Playback`, to replicate an edit made on the
    /// UI side.
    ///
    /// Silently dropped if the queue is somehow full — commands are user-driven and infrequent
    /// compared to how often the audio thread drains them, so this should never happen in
    /// practice.
    pub fn send(&mut self, action: PlaybackAction) {
        let _ = self.commands.push(action);
    }
}

impl fmt::Debug for PlaybackHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlaybackHandle").finish_non_exhaustive()
    }
}

/// An error starting playback.
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

/// The queue capacity for actions sent to the audio thread — generous for how infrequent user
/// edits are compared to how often the audio thread drains them.
const COMMAND_QUEUE_CAPACITY: usize = 256;

/// Start playing `playback` on the default output device, following whatever actions are sent
/// to the returned handle from then on.
///
/// # Errors
///
/// Returns an error if there is no default output device, or the stream cannot be configured,
/// built, or started.
pub fn play(playback: Playback) -> Result<PlaybackHandle, Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(Error::NoOutputDevice)?;
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    let (producer, consumer) = rtrb::RingBuffer::new(COMMAND_QUEUE_CAPACITY);
    let tick_bits = Arc::new(AtomicU16::new(playback.transport.position.0));
    let stream = match sample_format {
        SampleFormat::F32 => {
            build_and_play::<f32>(&device, config, playback, consumer, Arc::clone(&tick_bits))
        }
        SampleFormat::I16 => {
            build_and_play::<i16>(&device, config, playback, consumer, Arc::clone(&tick_bits))
        }
        SampleFormat::U16 => {
            build_and_play::<u16>(&device, config, playback, consumer, Arc::clone(&tick_bits))
        }
        other => return Err(Error::UnsupportedSampleFormat(other)),
    }?;

    Ok(PlaybackHandle {
        _stream: stream,
        tick_bits,
        commands: producer,
    })
}

#[cfg(test)]
mod tests {
    use osti_core::Position;

    use super::*;

    fn note(track: u8, tick: u16, pitch: u8, length: u8) -> PlaybackAction {
        PlaybackAction::InsertNote {
            track: TrackId(track),
            at: Position {
                tick: Tick(tick),
                pitch: Pitch(pitch),
            },
            length: osti_core::Length(length),
        }
    }

    fn new_player(playback: Playback) -> (Player, rtrb::Producer<PlaybackAction>) {
        let (producer, consumer) = rtrb::RingBuffer::new(COMMAND_QUEUE_CAPACITY);
        (
            Player::new(playback, consumer, Arc::new(AtomicU16::new(0)), 44_100.0),
            producer,
        )
    }

    /// Send an action through a freshly-created queue, which is never full.
    #[allow(clippy::unwrap_used)]
    fn send(producer: &mut rtrb::Producer<PlaybackAction>, action: PlaybackAction) {
        producer.push(action).unwrap();
    }

    #[test]
    fn fill_does_not_panic_on_zero_channels() {
        let (mut player, _producer) = new_player(Playback::new());
        let mut buffer = [0.0_f32; 4];
        player.fill(&mut buffer, 0); // must not panic
    }

    #[test]
    fn a_note_ramps_in_instead_of_starting_at_full_volume() {
        let mut playback = Playback::new();
        playback.apply(&note(0, 0, 60, 4));
        playback.transport.intent = PlaybackIntent::Playing;
        let (mut player, _producer) = new_player(playback);

        // A few dozen samples in, the tone is well past its first zero-crossing but the ramp
        // (220 samples to reach full level at this sample rate) has barely started.
        let mut buffer = [0.0_f32; 30];
        player.fill(&mut buffer, 1);

        let peak = buffer
            .iter()
            .fold(0.0_f32, |max, &sample| max.max(sample.abs()));
        assert!(peak > 0.0, "it's sounding");
        assert!(peak < 0.5, "but not yet at full level");
    }

    #[test]
    fn silence_when_nothing_is_playing() {
        let (mut player, _producer) = new_player(Playback::new());
        let mut buffer = [1_i16; 4];

        player.fill(&mut buffer, 1);

        assert_eq!(buffer, [0; 4]);
    }

    #[test]
    fn commands_from_the_queue_are_applied() {
        let (mut player, mut producer) = new_player(Playback::new());
        send(&mut producer, note(0, 0, 60, 4));
        send(
            &mut producer,
            PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing),
        );

        player.fill(&mut [1_i16; 1], 1);

        assert_eq!(player.voices.len(), 1);
    }

    #[test]
    fn pausing_silences_a_sounding_note_instead_of_leaving_it_ringing() {
        let mut playback = Playback::new();
        playback.apply(&note(0, 0, 60, 4));
        playback.transport.intent = PlaybackIntent::Playing;
        let (mut player, _producer) = new_player(playback);

        // Ramp fully in first — well past the ~220-sample ramp window at this sample rate.
        player.fill(&mut [0.0_f32; 300], 1);
        assert_eq!(player.voices.len(), 1, "the note is sounding");

        player.playback.transport.intent = PlaybackIntent::Paused;
        let mut buffer = [1.0_f32; 300]; // sentinel value, so silence is unambiguous below
        player.fill(&mut buffer, 1); // well past the ramp window again, still paused

        // The tail of the buffer is silent: the note faded out instead of ringing indefinitely
        // just because the frozen tick it was sounding at is still technically "current".
        assert!(buffer[250..].iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn adjacent_same_pitch_notes_retrigger_instead_of_blending() {
        let mut playback = Playback::new();
        playback.apply(&note(0, 0, 60, 2)); // covers ticks 0..1
        playback.apply(&note(0, 2, 60, 2)); // covers ticks 2..3 — adjacent, same pitch
        playback.transport.intent = PlaybackIntent::Playing;
        let (mut player, _producer) = new_player(playback);

        // Cross exactly two tick boundaries: into the first note, then into the second.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames_per_tick = (44_100.0 * TICK_SECS) as usize;
        let mut buffer = vec![0.0_f32; frames_per_tick * 2];
        player.fill(&mut buffer, 1);

        // Right at the boundary: the first note's voice is still fading out (not yet silent)
        // while the second note's voice has already started ramping in — two voices, not one
        // continuously-sounding tone straight through the boundary between them.
        assert_eq!(player.voices.len(), 2);
    }

    #[test]
    fn two_tracks_mix_together() {
        let mut playback = Playback::new();
        playback.tracks.push(osti_core::Track::new());
        let (mut player, mut producer) = new_player(playback);
        send(&mut producer, note(0, 0, 60, 4));
        send(&mut producer, note(1, 0, 64, 4));
        send(
            &mut producer,
            PlaybackAction::SetPlaybackIntent(PlaybackIntent::Playing),
        );

        player.fill(&mut [1_i16; 1], 1);

        assert_eq!(player.voices.len(), 2);
    }
}
