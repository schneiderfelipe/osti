//! Terminal UI for osti.
//!
//! This crate owns everything about talking to the terminal: entering and leaving the
//! alternate screen, reading input, and rendering. Rendering is kept decoupled from a real
//! terminal so it can be exercised with [`ratatui::backend::TestBackend`] in tests instead.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
pub use ratatui::DefaultTerminal;
use ratatui::Frame;

/// Initializes the terminal for interactive use: raw mode and the alternate screen buffer, with
/// a panic hook installed so the terminal is restored even if the program panics.
///
/// # Errors
///
/// Returns an error if the terminal could not be initialized, for example because there is no
/// controlling terminal at all.
pub fn init() -> io::Result<DefaultTerminal> {
    ratatui::try_init()
}

/// Restores the terminal to its original state: raw mode disabled, alternate screen left.
///
/// Any failure is reported to stderr rather than returned or panicked on — there is generally
/// nothing more useful to do while already exiting.
pub fn restore() {
    ratatui::restore();
}

/// Draws a single, empty frame.
///
/// This is the whole UI for now, since there's nothing to compose yet. It exists as the seam
/// later rendering will grow from, and so the render loop has something real to call.
// Real rendering is coming; `const fn` would just have to be undone.
#[allow(clippy::missing_const_for_fn)]
pub fn render(_frame: &mut Frame<'_>) {}

/// Blocks until the next key is pressed, ignoring every other terminal event (resizes, mouse
/// events, key releases and repeats, ...).
///
/// # Errors
///
/// Returns an error if reading the next terminal event fails.
pub fn next_key_press() -> io::Result<KeyEvent> {
    loop {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            return Ok(key);
        }
    }
}

/// Returns whether the given key event should quit the application.
///
/// This is a placeholder: there is no command mode yet to bind a real quit command to, so
/// `Esc` and `Ctrl-C` both quit directly.
#[must_use]
pub fn is_quit(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::{KeyCode, KeyEvent, KeyModifiers, is_quit, render};

    #[test]
    fn render_draws_a_blank_frame() {
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).unwrap();

        terminal.draw(render).unwrap();

        let blank_row = " ".repeat(10);
        terminal
            .backend()
            .assert_buffer_lines([blank_row.as_str(); 4]);
    }

    #[test]
    fn esc_quits() {
        assert!(is_quit(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn ctrl_c_quits() {
        assert!(is_quit(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn plain_keys_do_not_quit() {
        assert!(!is_quit(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE
        )));
    }
}
