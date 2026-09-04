//! Terminal UI for osti.
//!
//! Owns talking to the terminal: entering and leaving the alternate screen, reading input, and
//! rendering.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
pub use ratatui::DefaultTerminal;
use ratatui::Frame;

/// Initialize the terminal for interactive use, installing a panic hook that restores it.
///
/// # Errors
///
/// Returns an error if the terminal could not be initialized.
pub fn init() -> io::Result<DefaultTerminal> {
    ratatui::try_init()
}

/// Restore the terminal to its original state.
///
/// Any failure is reported to stderr rather than returned or panicked on.
pub fn restore() {
    ratatui::restore();
}

/// Draw a single, empty frame: there is nothing to compose yet.
// Real rendering is coming; `const fn` would just have to be undone.
#[allow(clippy::missing_const_for_fn)]
pub fn render(_frame: &mut Frame<'_>) {}

/// Block until the next key is pressed, ignoring every other terminal event.
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

/// Return whether the given key event should quit the application.
///
/// Currently `Esc` and `Ctrl-C` both quit.
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
