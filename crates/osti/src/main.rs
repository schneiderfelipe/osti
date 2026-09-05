//! `osti`: a terminal, keyboard-driven, modal tool for composing music.

use std::time::Duration;

use clap::Parser;
use color_eyre::Result;
use osti_audio::NoteLoop;
use osti_tui::DefaultTerminal;

/// How often the UI redraws on its own, to reflect the note's state changing in the audio thread.
const REDRAW_INTERVAL: Duration = Duration::from_millis(33);

/// Command-line arguments, currently just `--help`/`--version`.
#[derive(Parser)]
#[command(version, about)]
struct Cli;

fn main() -> Result<()> {
    color_eyre::install()?;
    Cli::parse();

    let audio_loop = osti_audio::play_looping_note()
        .inspect_err(|err| eprintln!("audio: {err}, continuing without sound"))
        .ok();

    let mut terminal = osti_tui::init()?;
    let result = run(&mut terminal, audio_loop.as_ref());
    osti_tui::restore();

    result
}

/// Run the render/input loop until the user quits.
fn run(terminal: &mut DefaultTerminal, audio_loop: Option<&NoteLoop>) -> Result<()> {
    loop {
        let note_on = audio_loop.is_some_and(NoteLoop::is_note_on);
        terminal.draw(|frame| osti_tui::render(frame, note_on))?;

        if let Some(key) = osti_tui::next_event(REDRAW_INTERVAL)?
            && osti_tui::is_quit(key)
        {
            return Ok(());
        }
    }
}
