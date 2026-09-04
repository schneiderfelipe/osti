//! Terminal UI for `osti`.
//!
//! Owns talking to the terminal: entering and leaving the alternate screen, reading input, and
//! rendering.

use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
pub use ratatui::DefaultTerminal;
use ratatui::{Frame, widgets::Paragraph};

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

/// Draw a single frame, showing whether the note is currently on.
pub fn render(frame: &mut Frame<'_>, note_on: bool) {
    let indicator = if note_on { "●" } else { "○" };
    frame.render_widget(Paragraph::new(indicator), frame.area());
}

/// A key press, or a tick after the timeout with no input.
///
/// The tick lets callers redraw on a timer, so the UI can reflect state that changes on its
/// own — not just in response to a key.
#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    /// A key was pressed.
    Key(KeyEvent),
    /// No input arrived within the timeout.
    Tick,
}

/// Wait up to `timeout` for the next key press, ignoring every other terminal event.
///
/// # Errors
///
/// Returns an error if polling or reading the next terminal event fails.
pub fn next_event(timeout: Duration) -> io::Result<InputEvent> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()?
        && key.kind == KeyEventKind::Press
    {
        return Ok(InputEvent::Key(key));
    }
    Ok(InputEvent::Tick)
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
    fn render_shows_the_note_off() {
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();

        terminal.draw(|frame| render(frame, false)).unwrap();

        terminal.backend().assert_buffer_lines(["○"]);
    }

    #[test]
    fn render_shows_the_note_on() {
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();

        terminal.draw(|frame| render(frame, true)).unwrap();

        terminal.backend().assert_buffer_lines(["●"]);
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
