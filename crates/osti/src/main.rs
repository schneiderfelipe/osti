//! A terminal, keyboard-driven, modal tool for composing music.

use std::time::Duration;

use clap::Parser;
use color_eyre::Result;

use osti_audio::PlaybackHandle;
use osti_core::{Action, Editor, Playback, PlaybackAction, PlaybackIntent, Tick};
use osti_tui::{DefaultTerminal, Keymap, Screen, Viewport};

/// How often the UI redraws on its own, to reflect the transport advancing in the audio thread.
const REDRAW_INTERVAL: Duration = Duration::from_millis(33);

/// Command-line arguments.
// `about` pulls its text from Cargo.toml's description, already the same sentence as the crate
// doc comment above; no need for a third copy here.
#[derive(Parser)]
#[command(about, author, version)]
struct Cli;

/// The audio thread's handle, if the device could actually be opened.
///
/// `None` doesn't mean broken, it means silent: [`Audio::start`] reports any failure to open a
/// device once, up front, and the rest of the app carries on without sound rather than refusing
/// to run at all — the whole point of a terminal tool is that it still works headless.
#[must_use = "the stream stops as soon as this is dropped"]
struct Audio(Option<PlaybackHandle>);

impl Audio {
    /// Try to start playing `playback`, falling back to silence (and a one-line stderr message)
    /// if the device couldn't be opened.
    fn start(playback: Playback) -> Self {
        Self(
            osti_audio::play(playback)
                .inspect_err(|err| eprintln!("audio: {err}, continuing without sound"))
                .ok(),
        )
    }

    /// Return the transport's live position, if it's actually advancing there — `None` with no
    /// device, or with one that isn't playing.
    fn current_tick(&self) -> Option<Tick> {
        self.0.as_ref().map(PlaybackHandle::current_tick)
    }

    /// Forward an action to the audio thread's own replica, if there's a device to forward it to.
    fn send(&mut self, action: PlaybackAction) {
        if let Some(handle) = &mut self.0 {
            handle.send(action);
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse();

    let mut editor = Editor::new();
    let mut audio = Audio::start(editor.playback.clone());

    let mut terminal = osti_tui::init()?;
    let result = run(&mut terminal, &mut editor, &mut audio);
    osti_tui::restore();

    result
}

/// Run the render/input loop until the user quits.
fn run(terminal: &mut DefaultTerminal, editor: &mut Editor, audio: &mut Audio) -> Result<()> {
    let mut keymap = Keymap::default();
    loop {
        let size = terminal.size()?;
        let viewport = Viewport::fit(size.width, size.height);

        terminal.draw(|frame| {
            frame.render_widget(
                Screen {
                    editor,
                    playhead: playhead(editor, audio),
                    viewport: &viewport,
                },
                frame.area(),
            );
        })?;

        let Some(key) = osti_tui::next_event(REDRAW_INTERVAL)? else {
            continue;
        };
        let Some(action) = keymap.feed(key, editor, &viewport) else {
            continue;
        };
        if action == Action::Quit {
            return Ok(());
        }
        perform(editor, audio, action);
    }
}

/// Return where to draw the playhead: the audio thread's own live position while it's actually
/// advancing, or the editor's last-known position otherwise (paused, stopped, or no audio at
/// all) — the audio thread has no reason to keep publishing a position it isn't moving.
fn playhead(editor: &Editor, audio: &Audio) -> Tick {
    if editor.playback.transport.intent == PlaybackIntent::Playing
        && let Some(tick) = audio.current_tick()
    {
        return tick;
    }
    editor.playback.transport.position
}

/// Apply an action to the editor, then forward whatever it actually changed to the audio thread's
/// own replica whenever that's audio-relevant.
fn perform(editor: &mut Editor, audio: &mut Audio, action: Action) {
    let Some(Action::Playback(playback_action)) = editor.update(action) else {
        return;
    };
    audio.send(playback_action);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
