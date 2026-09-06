//! Terminal UI for `osti`.
//!
//! This crate is the one place that knows about `crossterm`/`ratatui` — everything it hands back
//! (`Action`) is plain data from `osti-core`, decoupled from which physical keys produced it.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use osti_core::{
    Action, Editor, Mode, Pitch, PlaybackAction, PlaybackIntent, Range, Tick, TrackId,
};
pub use ratatui::DefaultTerminal;
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// The lowest pitch shown — one octave, high pitch at the top (piano-roll convention), containing
/// A4. No scrolling yet: this, together with `VISIBLE_TICKS`, stands in for a viewport until one
/// is actually needed.
const LOWEST_VISIBLE_PITCH: u8 = 60;

/// The highest pitch shown.
const HIGHEST_VISIBLE_PITCH: u8 = 71;

/// How many ticks of a track are shown at once — see `LOWEST_VISIBLE_PITCH`.
const VISIBLE_TICKS: u16 = 16;

/// A static keybinding reference, toggled by `?` — not the dynamic, partially-typed-sequence
/// guidance ROADMAP.md lists separately under "Beyond MVP"; just a fixed cheat sheet for now.
const HELP_TEXT: &str = "\
osti — keybindings

  h j k l / \u{2190} \u{2193} \u{2191} \u{2192}   move the cursor
  i                     insert mode (Enter places a note, Esc leaves)
  d                     delete the note at the cursor
  Space                 play / pause
  g g                   seek to the start
  u / U                 undo / redo
  :                     command line — type q or quit, then Enter, to exit
  ?                     toggle this help
  Ctrl-C                force quit, from anywhere

Esc leaves insert/command mode or this help screen — it never quits.";

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

/// Draw one frame.
///
/// The keybinding help panel in `Mode::Help`, otherwise a grid of every visible pitch by tick
/// with the playhead and selection — plus a status line always showing the mode (and, in
/// `Mode::Command`, the command being typed).
pub fn render(frame: &mut Frame<'_>, editor: &Editor, playhead: Tick) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    if editor.mode == Mode::Help {
        frame.render_widget(Paragraph::new(HELP_TEXT), main_area);
    } else {
        frame.render_widget(Paragraph::new(grid(editor, playhead)), main_area);
    }
    frame.render_widget(Paragraph::new(status_line(editor)), status_area);
}

fn grid(editor: &Editor, playhead: Tick) -> Vec<Line<'static>> {
    let track = editor.playback.tracks.first();
    (LOWEST_VISIBLE_PITCH..=HIGHEST_VISIBLE_PITCH)
        .rev()
        .map(|raw_pitch| {
            let pitch = Pitch(raw_pitch);
            let spans = (0..VISIBLE_TICKS)
                .map(|raw_tick| {
                    let tick = Tick(raw_tick);
                    let sounding = track
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
        .collect()
}

fn status_line(editor: &Editor) -> String {
    match editor.mode {
        Mode::Normal => "NORMAL".to_string(),
        Mode::Insert => "INSERT".to_string(),
        Mode::Help => "HELP — press Esc or ? to close".to_string(),
        Mode::Command => format!(":{}", editor.command_line),
    }
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

/// A pending, possibly multi-key sequence, resolved against [`Mode`]-dependent bindings.
///
/// The same Vim/Kakoune/Helix-style key-chord model DESIGN.md's lineage implies (e.g. `g` `g` to
/// go to the start), not just single keys.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    pending: Vec<KeyEvent>,
}

/// What a pending sequence resolved to.
enum Resolution {
    /// Could still extend to a longer bound sequence — wait for the next key.
    Pending,
    /// Doesn't continue any known sequence.
    Cancelled,
    /// A complete sequence, resolved to this action.
    Action(Action),
}

impl Keymap {
    /// Feed one key event, given the editor's current mode and content — needed to resolve a
    /// relative key like "left" into an absolute `Action` (see `Editor::update`'s own docs on why
    /// actions themselves carry no such context) and to type into the command line.
    ///
    /// `Ctrl-C` always quits immediately, in any mode, regardless of any pending sequence — a
    /// safety net independent of the `:q`/`:quit` command line.
    pub fn feed(&mut self, key: KeyEvent, editor: &Editor) -> Option<Action> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.pending.clear();
            return Some(Action::Quit);
        }
        self.pending.push(key);
        match resolve(&self.pending, editor) {
            Resolution::Pending => None,
            Resolution::Cancelled => {
                self.pending.clear();
                None
            }
            Resolution::Action(action) => {
                self.pending.clear();
                Some(action)
            }
        }
    }
}

fn resolve(pending: &[KeyEvent], editor: &Editor) -> Resolution {
    match editor.mode {
        Mode::Normal => resolve_normal(pending, editor),
        Mode::Insert => resolve_insert(pending, editor),
        Mode::Command => resolve_command(pending, editor),
        Mode::Help => resolve_help(pending),
    }
}

/// Movement, shared by normal and insert mode — Helix's own insert mode still lets you move
/// around, it's not a modal dead-end.
fn movement(code: KeyCode, editor: &Editor) -> Option<Resolution> {
    match code {
        KeyCode::Left | KeyCode::Char('h') => Some(moved(editor, |range| {
            let tick = Tick(range.head.0.saturating_sub(1));
            Range {
                anchor: tick,
                head: tick,
                ..range
            }
        })),
        KeyCode::Right | KeyCode::Char('l') => Some(moved(editor, |range| {
            let tick = Tick((range.head.0 + 1).min(VISIBLE_TICKS - 1));
            Range {
                anchor: tick,
                head: tick,
                ..range
            }
        })),
        KeyCode::Up | KeyCode::Char('k') => Some(moved(editor, |range| Range {
            pitch: Pitch((range.pitch.0 + 1).min(HIGHEST_VISIBLE_PITCH)),
            ..range
        })),
        KeyCode::Down | KeyCode::Char('j') => Some(moved(editor, |range| Range {
            pitch: Pitch(range.pitch.0.saturating_sub(1).max(LOWEST_VISIBLE_PITCH)),
            ..range
        })),
        _ => None,
    }
}

fn moved(editor: &Editor, f: impl FnMut(Range) -> Range) -> Resolution {
    Resolution::Action(Action::SetSelection(editor.selection.clone().map(f)))
}

fn resolve_normal(pending: &[KeyEvent], editor: &Editor) -> Resolution {
    match pending {
        [key] => {
            if let Some(resolution) = movement(key.code, editor) {
                return resolution;
            }
            match key.code {
                KeyCode::Char('g') => Resolution::Pending, // could be the start of `g g`
                KeyCode::Char('d') => remove_at_every_range(editor),
                KeyCode::Char('i') => Resolution::Action(Action::SetMode(Mode::Insert)),
                KeyCode::Char(':') => Resolution::Action(Action::SetMode(Mode::Command)),
                KeyCode::Char('?') => Resolution::Action(Action::SetMode(Mode::Help)),
                KeyCode::Char('u') => Resolution::Action(Action::Undo),
                KeyCode::Char('U') => Resolution::Action(Action::Redo),
                KeyCode::Char(' ') => {
                    let intent = toggled(editor.playback.transport.intent);
                    Resolution::Action(Action::Playback(PlaybackAction::SetPlaybackIntent(intent)))
                }
                _ => Resolution::Cancelled,
            }
        }
        [first, second]
            if first.code == KeyCode::Char('g') && second.code == KeyCode::Char('g') =>
        {
            Resolution::Action(Action::Playback(PlaybackAction::Seek(Tick(0))))
        }
        _ => Resolution::Cancelled,
    }
}

/// Insert mode: movement still works; `Enter` places a note at every cursor, sized to that
/// cursor's own span (see `Range::length`); `Esc` returns to normal.
fn resolve_insert(pending: &[KeyEvent], editor: &Editor) -> Resolution {
    let [key] = pending else {
        return Resolution::Cancelled;
    };
    if let Some(resolution) = movement(key.code, editor) {
        return resolution;
    }
    match key.code {
        KeyCode::Esc => Resolution::Action(Action::SetMode(Mode::Normal)),
        KeyCode::Enter => {
            let insertions = editor
                .selection
                .ranges()
                .map(|range| PlaybackAction::InsertNote {
                    track: TrackId(0),
                    at: range.position(),
                    length: range.length(),
                });
            #[allow(clippy::unwrap_used)] // a selection always has at least one range
            Resolution::Action(Action::Playback(PlaybackAction::batch(insertions).unwrap()))
        }
        _ => Resolution::Cancelled,
    }
}

/// The `:` command line: characters build up `editor.command_line`, `Enter` executes it.
fn resolve_command(pending: &[KeyEvent], editor: &Editor) -> Resolution {
    let [key] = pending else {
        return Resolution::Cancelled;
    };
    match key.code {
        KeyCode::Esc => Resolution::Action(Action::SetMode(Mode::Normal)),
        KeyCode::Enter => Resolution::Action(command(&editor.command_line)),
        KeyCode::Backspace => {
            let mut text = editor.command_line.clone();
            text.pop();
            Resolution::Action(Action::SetCommandLine(text))
        }
        KeyCode::Char(c) => {
            let mut text = editor.command_line.clone();
            text.push(c);
            Resolution::Action(Action::SetCommandLine(text))
        }
        _ => Resolution::Cancelled,
    }
}

/// The only commands so far are the ones needed to exit — an unrecognized command just closes
/// the command line, same as Esc.
fn command(text: &str) -> Action {
    match text {
        "q" | "quit" => Action::Quit,
        _ => Action::SetMode(Mode::Normal),
    }
}

fn resolve_help(pending: &[KeyEvent]) -> Resolution {
    let [key] = pending else {
        return Resolution::Cancelled;
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('?') => Resolution::Action(Action::SetMode(Mode::Normal)),
        _ => Resolution::Cancelled,
    }
}

fn remove_at_every_range(editor: &Editor) -> Resolution {
    let removals = editor
        .selection
        .ranges()
        .map(|range| PlaybackAction::RemoveNote {
            track: TrackId(0),
            at: range.position(),
        });
    #[allow(clippy::unwrap_used)] // a selection always has at least one range
    Resolution::Action(Action::Playback(PlaybackAction::batch(removals).unwrap()))
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
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(2),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(1),
        }));
        let mut terminal = Terminal::new(TestBackend::new(4, 13)).unwrap();

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
    fn ctrl_c_quits_from_any_mode() {
        let mut editor = Editor::new();
        editor.mode = Mode::Insert;
        let mut keymap = Keymap::default();

        let outcome = keymap.feed(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &editor,
        );

        assert_eq!(outcome, Some(Action::Quit));
    }

    #[test]
    fn a_single_g_is_pending_gg_resolves_to_seek_start() {
        let editor = Editor::new();
        let mut keymap = Keymap::default();

        let first = keymap.feed(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            &editor,
        );
        assert_eq!(first, None);

        let second = keymap.feed(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
            &editor,
        );
        assert_eq!(
            second,
            Some(Action::Playback(PlaybackAction::Seek(Tick(0))))
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)] // each key here is set up to resolve to an action
    fn colon_then_quit_then_enter_quits() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();

        let enter_command_mode = keymap
            .feed(
                KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
                &editor,
            )
            .unwrap();
        editor.update(enter_command_mode);

        for c in "quit".chars() {
            let action = keymap
                .feed(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), &editor)
                .unwrap();
            editor.update(action);
        }

        let outcome = keymap.feed(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &editor);

        assert_eq!(outcome, Some(Action::Quit));
    }

    #[test]
    fn esc_leaves_help_without_quitting() {
        let mut editor = Editor::new();
        editor.mode = Mode::Help;
        let mut keymap = Keymap::default();

        let outcome = keymap.feed(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &editor);

        assert_eq!(outcome, Some(Action::SetMode(Mode::Normal)));
    }
}
