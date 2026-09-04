//! `osti`: a terminal, keyboard-driven, modal tool for composing music.

use color_eyre::Result;
use osti_tui::DefaultTerminal;

fn main() -> Result<()> {
    color_eyre::install()?;

    let audio_loop = osti_audio::play_looping_note()
        .inspect_err(|err| eprintln!("audio: {err}, continuing without sound"))
        .ok();

    let mut terminal = osti_tui::init()?;
    let result = run(&mut terminal);
    osti_tui::restore();

    drop(audio_loop);
    result
}

/// Run the render/input loop until the user quits.
fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(osti_tui::render)?;

        let key = osti_tui::next_key_press()?;
        if osti_tui::is_quit(key) {
            return Ok(());
        }
    }
}
