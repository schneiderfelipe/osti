//! A terminal, keyboard-driven, modal tool for composing music.

use std::time::Duration;

use clap::Parser;
use color_eyre::Result;

use osti_audio::PlaybackHandle;
use osti_core::{Action, Editor, PlaybackIntent, Tick};
use osti_tui::{DefaultTerminal, Keymap};

/// How often the UI redraws on its own, to reflect the transport advancing in the audio thread.
const REDRAW_INTERVAL: Duration = Duration::from_millis(33);

/// Command-line arguments.
// `about` pulls its text from Cargo.toml's description, already the same sentence as the crate
// doc comment above; no need for a third copy here.
#[derive(Parser)]
#[command(about, author, version)]
struct Cli;

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse();

    let mut editor = Editor::new();
    let mut audio = osti_audio::play(editor.playback.clone())
        .inspect_err(|err| eprintln!("audio: {err}, continuing without sound"))
        .ok();

    let mut terminal = osti_tui::init()?;
    let result = run(&mut terminal, &mut editor, &mut audio);
    osti_tui::restore();

    result
}

/// Run the render/input loop until the user quits.
fn run(
    terminal: &mut DefaultTerminal,
    editor: &mut Editor,
    audio: &mut Option<PlaybackHandle>,
) -> Result<()> {
    let mut keymap = Keymap::default();
    loop {
        terminal.draw(|frame| osti_tui::render(frame, editor, playhead(editor, audio.as_ref())))?;

        let Some(key) = osti_tui::next_event(REDRAW_INTERVAL)? else {
            continue;
        };
        let Some(action) = keymap.feed(key, editor) else {
            continue;
        };
        if action == Action::Quit {
            return Ok(());
        }
        perform(editor, audio, action);
    }
}

/// Where to draw the playhead: the audio thread's own live position while it's actually
/// advancing, or the editor's last-known position otherwise (paused, stopped, or no audio at
/// all) — the audio thread has no reason to keep publishing a position it isn't moving.
fn playhead(editor: &Editor, audio: Option<&PlaybackHandle>) -> Tick {
    if editor.playback.transport.intent == PlaybackIntent::Playing
        && let Some(handle) = audio
    {
        return handle.current_tick();
    }
    editor.playback.transport.position
}

/// Apply an action to the editor, then forward whatever it actually changed to the audio thread's
/// own replica whenever that's audio-relevant.
fn perform(editor: &mut Editor, audio: &mut Option<PlaybackHandle>, action: Action) {
    let Some(Action::Playback(playback_action)) = editor.update(action) else {
        return;
    };
    if let Some(handle) = audio {
        handle.send(playback_action);
    }
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
