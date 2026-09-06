//! Terminal UI for `osti`.
//!
//! This crate is the one place that knows about `crossterm`/`ratatui` — everything it hands back
//! (`Action`, `Mode`) is plain data from `osti-core`, decoupled from which physical keys produced
//! it.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use osti_core::{
    Action, Editor, Mode, Pitch, PlaybackAction, PlaybackIntent, Range, Tick, TrackId,
};
pub use ratatui::DefaultTerminal;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// The lowest pitch shown — one octave, high pitch at the top (piano-roll convention), containing
/// A4. No scrolling yet: this, together with a pattern's own length, stands in for a viewport
/// until one's actually needed.
const LOWEST_VISIBLE_PITCH: u8 = 60;

/// The highest pitch shown.
const HIGHEST_VISIBLE_PITCH: u8 = 71;

/// Initialize the terminal for interactive use, installing a panic hook that restores it.
///
/// # Errors
///
/// Returns an error if the terminal could not be initialized.
pub fn init() -> io::Result<DefaultTerminal> {
    ratatui::try_init()
}

/// Restore the terminal to its original state.
pub fn restore() {
    ratatui::restore();
}

/// Draw one frame: a grid of every visible pitch by every tick in the current track, the
/// playhead, and the selection.
pub fn render(frame: &mut Frame<'_>, editor: &Editor, playhead: Tick) {
    let pattern = editor.playback.tracks.first();
    let lines: Vec<Line> = (LOWEST_VISIBLE_PITCH..=HIGHEST_VISIBLE_PITCH)
        .rev()
        .map(|raw_pitch| {
            let pitch = Pitch(raw_pitch);
            let spans = (0..pattern.length.0)
                .map(|raw_tick| {
                    let tick = Tick(raw_tick);
                    let sounding = pattern
                        .sounding_at(tick)
                        .any(|(position, _)| position.pitch == pitch);
                    let symbol = if sounding { "●" } else { "·" };

                    let mut modifier = Modifier::empty();
                    if tick == playhead {
                        modifier |= Modifier::REVERSED;
                    }
                    if editor
                        .selection
                        .ranges()
                        .any(|range| covers(range, pitch, tick))
                    {
                        modifier |= Modifier::UNDERLINED;
                    }
                    Span::styled(symbol, Style::default().add_modifier(modifier))
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), frame.area());
}

fn covers(range: Range, pitch: Pitch, tick: Tick) -> bool {
    range.pitch == pitch && (range.start()..=range.end()).contains(&tick)
}

/// Wait up to `timeout` for the next key press.
///
/// Returns `None` if the timeout elapses without one, or immediately if the terminal reports any
/// other kind of event, letting the caller redraw right away (e.g. on a resize) instead of
/// waiting out the rest of the timeout.
///
/// # Errors
///
/// Returns an error if polling or reading the next terminal event fails.
pub fn next_event(timeout: std::time::Duration) -> io::Result<Option<KeyEvent>> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()?
        && key.kind == KeyEventKind::Press
    {
        return Ok(Some(key));
    }
    Ok(None)
}

/// Return whether the given key event should quit the application.
///
/// Currently `Esc` and `Ctrl-C` both quit. Checked by the runtime before anything else — ending
/// the process isn't a data mutation `Editor::update` could perform, so it never becomes an
/// `Action`.
#[must_use]
pub fn is_quit(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

/// What feeding a key into a [`Keymap`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The key could be the start of a longer bound sequence — wait for the next one.
    Pending,
    /// The key didn't continue any known sequence; any pending sequence is abandoned.
    Cancelled,
    /// Switch to a different mode (not an `Action` — mode, like cursor movement, isn't undoable).
    SwitchMode(Mode),
    /// A complete sequence resolved to this action.
    Resolved(Action),
    /// Apply this action, then switch mode — insert mode's "place a note, then go back to
    /// normal" is the only binding that needs both at once.
    ResolvedAndSwitchMode(Action, Mode),
}

/// A pending, possibly multi-key sequence, resolved against [`Mode`]-dependent bindings.
///
/// The same Vim/Kakoune/Helix-style key-chord model DESIGN.md's lineage implies (e.g. `g` `g` to
/// go to the start), not just single keys.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    pending: Vec<KeyEvent>,
}

impl Keymap {
    /// Feed one key event, given the editor's current mode and content (needed to resolve a
    /// relative key like "left" into an absolute `Action` — see `Editor::update`'s own docs on
    /// why actions themselves carry no such context).
    pub fn feed(&mut self, key: KeyEvent, editor: &Editor) -> KeyOutcome {
        self.pending.push(key);
        let outcome = resolve(&self.pending, editor);
        if !matches!(outcome, KeyOutcome::Pending) {
            self.pending.clear();
        }
        outcome
    }
}

fn resolve(pending: &[KeyEvent], editor: &Editor) -> KeyOutcome {
    match editor.mode {
        Mode::Normal => resolve_normal(pending, editor),
        Mode::Insert => resolve_insert(pending, editor),
    }
}

fn resolve_normal(pending: &[KeyEvent], editor: &Editor) -> KeyOutcome {
    let length = editor.playback.tracks.first().length;
    match pending {
        [key] => match key.code {
            KeyCode::Char('g') => KeyOutcome::Pending, // could be the start of `g g`
            KeyCode::Left | KeyCode::Char('h') => moved(editor, |range| {
                let tick = Tick(range.head.0.saturating_sub(1));
                Range {
                    anchor: tick,
                    head: tick,
                    ..range
                }
            }),
            KeyCode::Right | KeyCode::Char('l') => moved(editor, |range| {
                let tick = Tick((range.head.0 + 1).min(length.0.saturating_sub(1)));
                Range {
                    anchor: tick,
                    head: tick,
                    ..range
                }
            }),
            KeyCode::Up | KeyCode::Char('k') => moved(editor, |range| Range {
                pitch: Pitch((range.pitch.0 + 1).min(HIGHEST_VISIBLE_PITCH)),
                ..range
            }),
            KeyCode::Down | KeyCode::Char('j') => moved(editor, |range| Range {
                pitch: Pitch(range.pitch.0.saturating_sub(1).max(LOWEST_VISIBLE_PITCH)),
                ..range
            }),
            KeyCode::Char('x') => remove_at_every_range(editor),
            KeyCode::Char('i') => KeyOutcome::SwitchMode(Mode::Insert),
            KeyCode::Char('u') => KeyOutcome::Resolved(Action::Undo),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                KeyOutcome::Resolved(Action::Redo)
            }
            KeyCode::Char(' ') => {
                let intent = toggled(editor.playback.transport.intent);
                KeyOutcome::Resolved(Action::Playback(PlaybackAction::SetPlaybackIntent(intent)))
            }
            _ => KeyOutcome::Cancelled,
        },
        [first, second]
            if first.code == KeyCode::Char('g') && second.code == KeyCode::Char('g') =>
        {
            KeyOutcome::Resolved(Action::Playback(PlaybackAction::Seek(Tick(0))))
        }
        _ => KeyOutcome::Cancelled,
    }
}

/// Insert mode has exactly one job: place a note at every cursor, sized to that cursor's own
/// span (see `Range::length`), then return to normal mode. `Esc` cancels without inserting.
fn resolve_insert(pending: &[KeyEvent], editor: &Editor) -> KeyOutcome {
    let [key] = pending else {
        return KeyOutcome::Cancelled;
    };
    if key.code == KeyCode::Esc {
        return KeyOutcome::SwitchMode(Mode::Normal);
    }
    let insertions = editor
        .selection
        .ranges()
        .map(|range| PlaybackAction::InsertNote {
            track: TrackId(0),
            at: range.position(),
            length: range.length(),
        });
    #[allow(clippy::unwrap_used)] // a selection always has at least one range
    let action = Action::Playback(PlaybackAction::batch(insertions).unwrap());
    KeyOutcome::ResolvedAndSwitchMode(action, Mode::Normal)
}

fn moved(editor: &Editor, f: impl FnMut(Range) -> Range) -> KeyOutcome {
    KeyOutcome::Resolved(Action::SetSelection(editor.selection.clone().map(f)))
}

fn remove_at_every_range(editor: &Editor) -> KeyOutcome {
    let removals = editor
        .selection
        .ranges()
        .map(|range| PlaybackAction::RemoveNote {
            track: TrackId(0),
            at: range.position(),
        });
    #[allow(clippy::unwrap_used)] // a selection always has at least one range
    KeyOutcome::Resolved(Action::Playback(PlaybackAction::batch(removals).unwrap()))
}

const fn toggled(intent: PlaybackIntent) -> PlaybackIntent {
    match intent {
        PlaybackIntent::Playing => PlaybackIntent::Paused,
        PlaybackIntent::Paused => PlaybackIntent::Playing,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    #[test]
    fn render_places_a_note_marker_where_one_sounds() {
        let mut editor = Editor::new(Tick(4));
        editor.update(&Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(2),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(1),
        }));
        let mut terminal = Terminal::new(TestBackend::new(4, 12)).unwrap();

        // Away from the playhead/cursor's own column, so this checks the note marker alone.
        terminal
            .draw(|frame| render(frame, &editor, Tick(0)))
            .unwrap();

        // Rows go high-to-low from 71 down to 60; A4 (69) is the third row.
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 2)].symbol(), "●");
        assert_eq!(buffer[(1, 2)].symbol(), "·");
    }

    #[test]
    fn esc_quits() {
        assert!(is_quit(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn plain_keys_do_not_quit() {
        assert!(!is_quit(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn a_single_g_is_pending_gg_resolves_to_seek_start() {
        let editor = Editor::new(Tick(4));
        let mut keymap = Keymap::default();

        let first = keymap.feed(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            &editor,
        );
        assert_eq!(first, KeyOutcome::Pending);

        let second = keymap.feed(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            &editor,
        );
        assert_eq!(
            second,
            KeyOutcome::Resolved(Action::Playback(PlaybackAction::Seek(Tick(0))))
        );
    }

    #[test]
    fn i_switches_to_insert_mode() {
        let editor = Editor::new(Tick(4));
        let mut keymap = Keymap::default();

        let outcome = keymap.feed(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
            &editor,
        );

        assert_eq!(outcome, KeyOutcome::SwitchMode(Mode::Insert));
    }
}
