//! osti: a terminal, keyboard-driven, modal tool for composing music.

use std::io;

use osti_tui::DefaultTerminal;

fn main() -> io::Result<()> {
    let audio_loop = osti_audio::play_silence()
        .inspect_err(|err| eprintln!("audio: {err}, continuing without sound"))
        .ok();

    let mut terminal = osti_tui::init()?;
    let result = run(&mut terminal);
    osti_tui::restore();

    drop(audio_loop);
    result
}

/// Runs the render/input loop until the user quits.
fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    loop {
        terminal.draw(osti_tui::render)?;

        let key = osti_tui::next_key_press()?;
        if osti_tui::is_quit(key) {
            return Ok(());
        }
    }
}
