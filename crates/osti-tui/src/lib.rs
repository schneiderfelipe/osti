//! Terminal UI for `osti`.
//!
//! This crate is the one place that knows about `crossterm`/`ratatui` — everything it hands back
//! (`Action`) is plain data from `osti-core`, decoupled from which physical keys produced it.

use std::io;
use std::ops::Range as TickRange;
use std::ops::RangeInclusive;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use osti_core::{Action, Editor, Mode, Pitch, PlaybackAction, Position, Range, Tick, TrackId};
pub use ratatui::DefaultTerminal;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// The visible window of the grid: as many pitches and ticks as fit on screen, centered on A4.
///
/// No scrolling yet — this is simply sized to the terminal, not bigger than it, recomputed every
/// frame so a resize is reflected immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// The pitch rows shown, high to low.
    pub pitches: RangeInclusive<Pitch>,
    /// The tick columns shown.
    pub ticks: TickRange<Tick>,
}

impl Viewport {
    /// Fit as many rows and columns as `(width, height)` allows.
    #[must_use]
    pub fn fit(width: u16, height: u16) -> Self {
        let rows = height.saturating_sub(1).max(1); // one row reserved for the status line
        #[allow(clippy::cast_possible_truncation)] // terminals aren't 256+ rows tall
        let rows = rows.min(u16::from(u8::MAX)) as u8;
        let half = rows / 2;
        let low = Pitch::A4.0.saturating_sub(half);
        let high = low.saturating_add(rows.saturating_sub(1));
        Self {
            pitches: Pitch(low)..=Pitch(high),
            ticks: Tick(0)..Tick(width.max(1)),
        }
    }
}

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
/// A grid of every visible pitch by tick, with the playhead and selection, a status line always
/// showing the mode, and — in `Mode::Help` — a keybinding overlay on top of it all, the same way
/// Helix's own help popups sit over the buffer rather than replacing it.
pub fn render(frame: &mut Frame<'_>, editor: &Editor, playhead: Tick, viewport: &Viewport) {
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    frame.render_widget(Paragraph::new(grid(editor, playhead, viewport)), main_area);
    frame.render_widget(Paragraph::new(status_line(editor)), status_area);

    if editor.mode == Mode::Help {
        render_help(frame, main_area);
    }
}

/// A note's first tick is marked distinctly from the rest of its span, so two same-pitch notes
/// placed back to back (no gap) still show a visible seam instead of merging into one block.
const NOTE_START: &str = "▐";
const NOTE_BODY: &str = "█";
const EMPTY: &str = "·";

/// The track this UI shows and edits — always the first one, until multi-track UI exists.
const TRACK: TrackId = TrackId(0);

fn grid(editor: &Editor, playhead: Tick, viewport: &Viewport) -> Vec<Line<'static>> {
    let track = editor.playback.track(TRACK);
    let low = viewport.pitches.start().0;
    let high = viewport.pitches.end().0;
    (low..=high)
        .rev()
        .map(|raw_pitch| {
            let pitch = Pitch(raw_pitch);
            let spans = (viewport.ticks.start.0..viewport.ticks.end.0)
                .map(|raw_tick| {
                    let tick = Tick(raw_tick);
                    let sounding = track
                        .sounding_at(tick)
                        .find(|(position, _)| position.pitch == pitch);
                    let symbol = match sounding {
                        Some((position, _)) if position.tick == tick => NOTE_START,
                        Some(_) => NOTE_BODY,
                        None => EMPTY,
                    };

                    let mut modifier = Modifier::empty();
                    if tick == playhead {
                        modifier |= Modifier::REVERSED;
                    }
                    if editor
                        .selection
                        .ranges()
                        .any(|range| range.covers(Position { tick, pitch }))
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
        Mode::Visual => "VISUAL".to_string(),
        Mode::Help => "HELP".to_string(),
        Mode::Command => format!(":{}", editor.command_line),
    }
}

/// The help overlay's content — computed from the same `Command` tables that drive dispatch, not
/// a separately maintained wall of text that could drift out of sync with the real bindings.
fn render_help(frame: &mut Frame<'_>, over: Rect) {
    let mut lines = vec![Line::from("")];
    for (title, commands) in [
        ("Normal", NORMAL),
        ("Insert", INSERT),
        ("Visual", VISUAL),
        ("Help", HELP),
    ] {
        lines.push(Line::from(format!(" {title}")));
        for &command in commands {
            lines.push(Line::from(format!(
                "   {:<14} {}",
                keys_label(command),
                command.description()
            )));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(" Command line (`:`, then Enter)"));
    for &(name, description) in TEXT_COMMANDS {
        lines.push(Line::from(format!("   {name:<14} {description}")));
    }

    #[allow(clippy::cast_possible_truncation)] // help text is a handful of short, fixed lines
    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .saturating_add(2) as u16;
    #[allow(clippy::cast_possible_truncation)]
    let height = lines.len() as u16 + 2;
    let area = centered(over, width, height);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" osti — keybindings ")),
        area,
    );
}

fn keys_label(command: Command) -> String {
    command
        .keys()
        .iter()
        .map(|sequence| {
            sequence
                .iter()
                .map(|&code| key_label(code))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

fn key_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        other => format!("{other:?}"),
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
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

/// Every keyboard-reachable operation, and the key sequences (per mode) that reach it.
///
/// One table is the source for both dispatch and the help overlay's text, so the two can't drift
/// apart — the alternative (a hand-written help paragraph next to a separate match statement)
/// is exactly the duplication this avoids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    PreviousNote,
    NextNote,
    GotoStart,
    GotoEnd,
    EnterInsert,
    ToggleVisual,
    OpenCommandLine,
    ToggleHelp,
    Delete,
    Undo,
    Redo,
    PlayPause,
    SeekHere,
    PlaceNote,
    Leave,
}

impl Command {
    /// The key sequences that trigger this command — a chord is one sequence; several sequences
    /// are alternatives (e.g. `h` or `←`).
    const fn keys(self) -> &'static [&'static [KeyCode]] {
        use KeyCode::{Char, Down, Esc, Left, Right, Up};
        match self {
            Self::MoveLeft => &[&[Char('h')], &[Left]],
            Self::MoveRight => &[&[Char('l')], &[Right]],
            Self::MoveUp => &[&[Char('k')], &[Up]],
            Self::MoveDown => &[&[Char('j')], &[Down]],
            Self::PreviousNote => &[&[Char('b')]],
            Self::NextNote => &[&[Char('e')]],
            Self::GotoStart => &[&[Char('g'), Char('g')]],
            Self::GotoEnd => &[&[Char('g'), Char('e')]],
            Self::EnterInsert => &[&[Char('i')]],
            Self::ToggleVisual => &[&[Char('v')]],
            Self::OpenCommandLine => &[&[Char(':')]],
            Self::ToggleHelp => &[&[Char('?')]],
            Self::Delete => &[&[Char('d')]],
            Self::Undo => &[&[Char('u')]],
            Self::Redo => &[&[Char('U')]],
            Self::PlayPause | Self::PlaceNote => &[&[Char(' ')]],
            Self::SeekHere => &[&[KeyCode::Enter]],
            Self::Leave => &[&[Esc]],
        }
    }

    /// A one-line description, used only by the help overlay.
    const fn description(self) -> &'static str {
        match self {
            Self::MoveLeft => "move left",
            Self::MoveRight => "move right",
            Self::MoveUp => "move up (higher pitch)",
            Self::MoveDown => "move down (lower pitch)",
            Self::PreviousNote => "jump to the previous note",
            Self::NextNote => "jump to the next note",
            Self::GotoStart => "go to the start",
            Self::GotoEnd => "go to the end of what's visible",
            Self::EnterInsert => "insert mode",
            Self::ToggleVisual => "visual mode (movement extends the selection)",
            Self::OpenCommandLine => "command line",
            Self::ToggleHelp => "toggle this help",
            Self::Delete => "delete the note(s) at the cursor",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::PlayPause => "play / pause",
            Self::SeekHere => "seek the transport to the cursor",
            Self::PlaceNote => "place a note",
            Self::Leave => "back to normal mode",
        }
    }
}

/// Normal mode's commands.
const NORMAL: &[Command] = &[
    Command::MoveLeft,
    Command::MoveRight,
    Command::MoveUp,
    Command::MoveDown,
    Command::PreviousNote,
    Command::NextNote,
    Command::GotoStart,
    Command::GotoEnd,
    Command::EnterInsert,
    Command::ToggleVisual,
    Command::OpenCommandLine,
    Command::ToggleHelp,
    Command::Delete,
    Command::Undo,
    Command::Redo,
    Command::PlayPause,
    Command::SeekHere,
];

/// Insert mode's commands: still movement, plus placing a note and leaving.
const INSERT: &[Command] = &[
    Command::MoveLeft,
    Command::MoveRight,
    Command::MoveUp,
    Command::MoveDown,
    Command::PreviousNote,
    Command::NextNote,
    Command::GotoStart,
    Command::GotoEnd,
    Command::PlaceNote,
    Command::Leave,
];

/// Visual mode's commands: the same as normal mode, but movement extends (see `apply`) instead of
/// moving — plus leaving, either way (`v` again, like entering it, or `Esc`). `EnterInsert` and
/// `OpenCommandLine` work here too, same as normal mode, and the selection carries over into
/// wherever they lead: entering insert mode with a multi-tick selection still held is how a note
/// gets placed at that exact length in one pass (select the span, `i`, `Space`), rather than
/// needing to leave visual mode first.
const VISUAL: &[Command] = &[
    Command::MoveLeft,
    Command::MoveRight,
    Command::MoveUp,
    Command::MoveDown,
    Command::PreviousNote,
    Command::NextNote,
    Command::GotoStart,
    Command::GotoEnd,
    Command::EnterInsert,
    Command::OpenCommandLine,
    Command::Delete,
    Command::Undo,
    Command::Redo,
    Command::PlayPause,
    Command::SeekHere,
    Command::ToggleVisual,
    Command::Leave,
];

/// Help mode's only commands: close it, either way (`?` again, like opening it, or `Esc`).
const HELP: &[Command] = &[Command::ToggleHelp, Command::Leave];

const fn bindings(mode: Mode) -> &'static [Command] {
    match mode {
        Mode::Normal => NORMAL,
        Mode::Insert => INSERT,
        Mode::Visual => VISUAL,
        Mode::Help => HELP,
        Mode::Command => &[],
    }
}

/// Whether a pending sequence of key codes matches a command exactly, could still extend to one,
/// or matches nothing in `commands`.
enum Match {
    Exact(Command),
    Prefix,
    None,
}

fn match_commands(codes: &[KeyCode], commands: &[Command]) -> Match {
    if let Some(&command) = commands
        .iter()
        .find(|command| command.keys().contains(&codes))
    {
        return Match::Exact(command);
    }
    let is_prefix = commands.iter().any(|command| {
        command
            .keys()
            .iter()
            .any(|sequence| sequence.len() > codes.len() && sequence[..codes.len()] == *codes)
    });
    if is_prefix {
        Match::Prefix
    } else {
        Match::None
    }
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
    /// Doesn't continue any known sequence (or resolved to a command with nothing to do, like
    /// deleting where there's no note).
    Cancelled,
    /// A complete sequence, resolved to this action.
    Action(Action),
}

impl Keymap {
    /// Feed one key event, given the editor's current mode, content, and the current viewport —
    /// needed to resolve a relative key like "left" into an absolute `Action` (see
    /// `Editor::update`'s own docs on why actions themselves carry no such context) and to type
    /// into the command line.
    ///
    /// There is no keyboard shortcut that quits by itself — the `:` command line's `:q`/`:quit`
    /// is the only way.
    pub fn feed(&mut self, key: KeyEvent, editor: &Editor, viewport: &Viewport) -> Option<Action> {
        self.pending.push(key);
        match resolve(&self.pending, editor, viewport) {
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

fn resolve(pending: &[KeyEvent], editor: &Editor, viewport: &Viewport) -> Resolution {
    // Command mode is free-text typing, not chord-matching against a fixed table — every other
    // mode, help included, resolves the same way.
    if editor.mode == Mode::Command {
        return resolve_command(pending, editor);
    }
    let codes: Vec<KeyCode> = pending.iter().map(|key| key.code).collect();
    match match_commands(&codes, bindings(editor.mode)) {
        Match::Exact(command) => {
            apply(command, editor, viewport).map_or(Resolution::Cancelled, Resolution::Action)
        }
        Match::Prefix => Resolution::Pending,
        Match::None => Resolution::Cancelled,
    }
}

/// Turn a resolved [`Command`] into the absolute action it means right now — `None` only for
/// [`Command::Delete`] finding nothing to delete.
fn apply(command: Command, editor: &Editor, viewport: &Viewport) -> Option<Action> {
    // In visual mode, tick movement extends the selection (moves `head` only, keeping `anchor`);
    // everywhere else it moves (collapses both to the new position). Pitch movement is never
    // extended either way — a `Range` is one pitch row, so "extending" across rows would need a
    // second range entirely, not a change to this one.
    let extend = editor.mode == Mode::Visual;
    match command {
        Command::MoveLeft => Some(moved_tick(editor, extend, |range| {
            Tick(range.head.0.saturating_sub(1))
        })),
        Command::MoveRight => {
            let max = viewport.ticks.end.0.saturating_sub(1);
            Some(moved_tick(editor, extend, move |range| {
                Tick((range.head.0 + 1).min(max))
            }))
        }
        Command::MoveUp => {
            let highest = viewport.pitches.end().0;
            Some(moved_pitch(editor, move |range| {
                Pitch(range.pitch.0.saturating_add(1).min(highest))
            }))
        }
        Command::MoveDown => {
            let lowest = viewport.pitches.start().0;
            Some(moved_pitch(editor, move |range| {
                Pitch(range.pitch.0.saturating_sub(1).max(lowest))
            }))
        }
        Command::PreviousNote => {
            let track = editor.playback.track(TRACK);
            Some(moved_tick(editor, extend, |range| {
                track
                    .previous_note_start(range.pitch, range.head)
                    .unwrap_or(range.head)
            }))
        }
        Command::NextNote => {
            let track = editor.playback.track(TRACK);
            Some(moved_tick(editor, extend, |range| {
                track
                    .next_note_end(range.pitch, range.head)
                    .unwrap_or(range.head)
            }))
        }
        Command::GotoStart => Some(moved_tick(editor, extend, |_| Tick(0))),
        Command::GotoEnd => {
            let end = viewport.ticks.end.0.saturating_sub(1);
            Some(moved_tick(editor, extend, move |_| Tick(end)))
        }
        Command::EnterInsert => Some(Action::SetMode(Mode::Insert)),
        Command::ToggleVisual => Some(Action::SetMode(editor.mode.toggled(Mode::Visual))),
        Command::OpenCommandLine => Some(Action::SetMode(Mode::Command)),
        Command::ToggleHelp => Some(Action::SetMode(editor.mode.toggled(Mode::Help))),
        Command::Delete => delete_selection(editor),
        Command::Undo => Some(Action::Undo),
        Command::Redo => Some(Action::Redo),
        Command::PlayPause => {
            let intent = editor.playback.transport.intent.toggled();
            Some(Action::Playback(PlaybackAction::SetPlaybackIntent(intent)))
        }
        Command::SeekHere => {
            let tick = editor.selection.primary().head;
            Some(Action::Playback(PlaybackAction::Seek(tick)))
        }
        Command::PlaceNote => Some(place_note(editor)),
        Command::Leave => Some(Action::SetMode(Mode::Normal)),
    }
}

/// Move every range's head, per `new_head` — see [`Range::moved`] for what `extend` does to the
/// anchor.
fn moved_tick(editor: &Editor, extend: bool, mut new_head: impl FnMut(Range) -> Tick) -> Action {
    Action::SetSelection(
        editor
            .selection
            .clone()
            .map(|range| range.moved(new_head(range), extend)),
    )
}

fn moved_pitch(editor: &Editor, mut new_pitch: impl FnMut(Range) -> Pitch) -> Action {
    Action::SetSelection(
        editor
            .selection
            .clone()
            .map(|range| range.with_pitch(new_pitch(range))),
    )
}

/// Insert a note at every cursor, sized to that cursor's own span (see `Range::length`) — a
/// multi-tick visual selection places one correspondingly longer note.
fn place_note(editor: &Editor) -> Action {
    let insertions = editor
        .selection
        .ranges()
        .map(|range| PlaybackAction::InsertNote {
            track: TRACK,
            at: range.position(),
            length: range.length(),
        });
    #[allow(clippy::unwrap_used)] // a selection always has at least one range
    Action::Playback(PlaybackAction::batch(insertions).unwrap())
}

/// Delete every note whose start falls within any range's span — `None` if there's nothing there.
fn delete_selection(editor: &Editor) -> Option<Action> {
    let track = editor.playback.track(TRACK);
    let removals = editor
        .selection
        .ranges()
        .flat_map(|range| track.positions_in_span(range.pitch, range.start(), range.end()))
        .map(|at| PlaybackAction::RemoveNote { track: TRACK, at });
    PlaybackAction::batch(removals).map(Action::Playback)
}

/// The `:` command line: characters build up `editor.command_line`, `Enter` executes it.
fn resolve_command(pending: &[KeyEvent], editor: &Editor) -> Resolution {
    let [key] = pending else {
        return Resolution::Cancelled;
    };
    match key.code {
        KeyCode::Esc => Resolution::Action(Action::SetMode(Mode::Normal)),
        KeyCode::Enter => Resolution::Action(command(&editor.command_line)),
        KeyCode::Backspace => edited_command_line(editor, |text| {
            text.pop();
        }),
        KeyCode::Char(c) => edited_command_line(editor, |text| text.push(c)),
        _ => Resolution::Cancelled,
    }
}

fn edited_command_line(editor: &Editor, edit: impl FnOnce(&mut String)) -> Resolution {
    let mut text = editor.command_line.clone();
    edit(&mut text);
    Resolution::Action(Action::SetCommandLine(text))
}

/// The only commands so far are the ones needed to exit — this is also the *only* way to quit;
/// there's deliberately no keyboard shortcut for it. An unrecognized command just closes the
/// command line, same as Esc. Also the help overlay's source for what the command line can do —
/// one list, not a table plus a separately hand-written description of it.
const TEXT_COMMANDS: &[(&str, &str)] = &[("q", "quit"), ("quit", "quit")];

fn command(text: &str) -> Action {
    if TEXT_COMMANDS.iter().any(|&(name, _)| name == text) {
        Action::Quit
    } else {
        Action::SetMode(Mode::Normal)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // each `feed` here is set up to resolve to an action
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn feed(keymap: &mut Keymap, editor: &Editor, code: KeyCode) -> Option<Action> {
        let viewport = Viewport::fit(16, 13);
        keymap.feed(KeyEvent::from(code), editor, &viewport)
    }

    #[test]
    fn viewport_fits_the_given_size_centered_on_a4() {
        let viewport = Viewport::fit(16, 13);
        assert_eq!(viewport.ticks, Tick(0)..Tick(16));
        assert_eq!(viewport.pitches.start().0, Pitch::A4.0 - 6);
        assert_eq!(viewport.pitches.end().0, Pitch::A4.0 + 5);
    }

    #[test]
    fn a_note_marker_shows_its_start_distinctly_from_its_body() {
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(2),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(3),
        }));
        let viewport = Viewport::fit(8, 13);
        let a4_row = u16::from(viewport.pitches.end().0 - Pitch::A4.0);
        let mut terminal = Terminal::new(TestBackend::new(8, 13)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(0), &viewport))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, a4_row)].symbol(), NOTE_START);
        assert_eq!(buffer[(3, a4_row)].symbol(), NOTE_BODY);
        assert_eq!(buffer[(4, a4_row)].symbol(), NOTE_BODY);
        assert_eq!(buffer[(5, a4_row)].symbol(), EMPTY);
    }

    #[test]
    fn adjacent_same_pitch_notes_both_show_a_start_marker() {
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(0),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(2),
        }));
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(2),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(2),
        }));
        let viewport = Viewport::fit(8, 13);
        let a4_row = u16::from(viewport.pitches.end().0 - Pitch::A4.0);
        let mut terminal = Terminal::new(TestBackend::new(8, 13)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(10), &viewport)) // playhead elsewhere
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            (0..4)
                .map(|x| buffer[(x, a4_row)].symbol())
                .collect::<Vec<_>>(),
            vec![NOTE_START, NOTE_BODY, NOTE_START, NOTE_BODY],
        );
    }

    #[test]
    fn a_single_g_is_pending_gg_moves_the_cursor_to_the_start() {
        let mut editor = Editor::new();
        editor.update(Action::SetSelection(osti_core::Selection::single(
            Range::at(osti_core::Position {
                tick: Tick(5),
                pitch: Pitch::A4,
            }),
        )));
        let mut keymap = Keymap::default();

        assert!(feed(&mut keymap, &editor, KeyCode::Char('g')).is_none());
        let action = feed(&mut keymap, &editor, KeyCode::Char('g')).unwrap();
        editor.update(action);

        assert_eq!(editor.selection.primary().head, Tick(0));
    }

    #[test]
    fn visual_mode_extends_instead_of_moving() {
        let mut editor = Editor::new();
        editor.update(Action::SetMode(Mode::Visual));
        let mut keymap = Keymap::default();

        let action = feed(&mut keymap, &editor, KeyCode::Char('l')).unwrap();
        editor.update(action);

        let range = editor.selection.primary();
        assert_eq!(range.anchor, Tick(0)); // unchanged
        assert_eq!(range.head, Tick(1)); // moved
    }

    #[test]
    fn selecting_a_span_in_visual_mode_then_inserting_places_a_note_that_length() {
        // The direct Helix-style workflow: select a span, `i` straight from visual mode (no need
        // to leave it first), `Space` places one note the whole span long.
        let mut editor = Editor::new();
        editor.update(Action::SetMode(Mode::Visual));
        let mut keymap = Keymap::default();

        let extend = feed(&mut keymap, &editor, KeyCode::Char('l')).unwrap();
        editor.update(extend);
        let enter_insert = feed(&mut keymap, &editor, KeyCode::Char('i')).unwrap();
        editor.update(enter_insert);
        let place = feed(&mut keymap, &editor, KeyCode::Char(' ')).unwrap();

        assert_eq!(
            place,
            Action::Playback(PlaybackAction::InsertNote {
                track: TrackId(0),
                at: osti_core::Position {
                    tick: Tick(0),
                    pitch: Pitch::A4,
                },
                length: osti_core::Length(2),
            })
        );
    }

    #[test]
    fn space_places_a_note_in_insert_mode_but_not_normal_mode() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();
        assert!(feed(&mut keymap, &editor, KeyCode::Char(' ')).is_some()); // play/pause in normal mode

        editor.update(Action::SetMode(Mode::Insert));
        let action = feed(&mut keymap, &editor, KeyCode::Char(' ')).unwrap();

        assert!(matches!(
            action,
            Action::Playback(PlaybackAction::InsertNote { .. })
        ));
    }

    #[test]
    fn ctrl_c_does_not_quit_and_quitting_needs_the_command_line() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();
        let viewport = Viewport::fit(16, 13);

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), crossterm::event::KeyModifiers::CONTROL);
        assert_eq!(keymap.feed(ctrl_c, &editor, &viewport), None);

        let enter_command = feed(&mut keymap, &editor, KeyCode::Char(':')).unwrap();
        editor.update(enter_command);
        for c in "quit".chars() {
            let action = feed(&mut keymap, &editor, KeyCode::Char(c)).unwrap();
            editor.update(action);
        }
        assert_eq!(
            feed(&mut keymap, &editor, KeyCode::Enter),
            Some(Action::Quit)
        );
    }

    #[test]
    fn deleting_an_empty_cell_does_nothing() {
        let editor = Editor::new();
        let mut keymap = Keymap::default();
        assert!(feed(&mut keymap, &editor, KeyCode::Char('d')).is_none());
    }

    #[test]
    fn enter_seeks_the_transport_to_the_cursor() {
        let mut editor = Editor::new();
        editor.update(Action::SetSelection(osti_core::Selection::single(
            Range::at(osti_core::Position {
                tick: Tick(3),
                pitch: Pitch::A4,
            }),
        )));
        let mut keymap = Keymap::default();

        let action = feed(&mut keymap, &editor, KeyCode::Enter).unwrap();

        assert_eq!(action, Action::Playback(PlaybackAction::Seek(Tick(3))));
    }

    #[test]
    fn question_mark_opens_and_closes_help() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();

        let open = feed(&mut keymap, &editor, KeyCode::Char('?')).unwrap();
        editor.update(open);
        assert_eq!(editor.mode, Mode::Help);

        let close = feed(&mut keymap, &editor, KeyCode::Char('?')).unwrap();
        editor.update(close);
        assert_eq!(editor.mode, Mode::Normal);
    }

    #[test]
    fn short_and_long_quit_commands_both_quit() {
        for word in ["q", "quit"] {
            let mut editor = Editor::new();
            let mut keymap = Keymap::default();
            let open = feed(&mut keymap, &editor, KeyCode::Char(':')).unwrap();
            editor.update(open);
            for c in word.chars() {
                let action = feed(&mut keymap, &editor, KeyCode::Char(c)).unwrap();
                editor.update(action);
            }
            assert_eq!(
                feed(&mut keymap, &editor, KeyCode::Enter),
                Some(Action::Quit)
            );
        }
    }

    #[test]
    fn the_help_overlay_is_generated_from_the_same_bindings_that_dispatch() {
        assert!(NORMAL.contains(&Command::MoveLeft));
        assert_eq!(keys_label(Command::MoveLeft), "h / ←");
        assert_eq!(Command::MoveLeft.description(), "move left");
    }
}
