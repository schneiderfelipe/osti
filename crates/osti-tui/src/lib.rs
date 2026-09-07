//! Terminal UI for `osti`.
//!
//! This crate is the one place that knows about `crossterm`/`ratatui` — everything it hands back
//! (`Action`) is plain data from `osti-core`, decoupled from which physical keys produced it.

use std::io;
use std::ops::Range as TickRange;
use std::ops::RangeInclusive;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use osti_core::{
    Action, Editor, Mode, Note, Pitch, PlaybackAction, Position, Range, Tick, TrackId,
};
pub use ratatui::DefaultTerminal;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// Rows reserved above the grid, for the beat/step ruler (see `header`).
const HEADER_ROWS: u16 = 2;

/// Columns reserved to the left of the grid, for each row's label — a pitch name on a grid row,
/// `"beat"`/`"step"` on the ruler above it (see `gutter`, which actually produces this width).
const GUTTER_COLS: u16 = 5;

/// The visible window of the grid: as many pitches and ticks as fit on screen, centered on A4.
///
/// No scrolling yet — this is simply sized to the terminal, not bigger than it, recomputed every
/// frame so a resize is reflected immediately. Callers outside this crate only ever construct one
/// (`fit`) and hand it back to `render`/`Keymap::feed`; nothing outside needs to look inside, so
/// the pitch/tick ranges themselves stay private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// The pitch rows shown, high to low.
    pitches: RangeInclusive<Pitch>,
    /// The tick columns shown.
    ticks: TickRange<Tick>,
}

impl Viewport {
    /// Fit as many rows and columns as `(width, height)` allows, once the ruler, gutter, and
    /// status line's own space is set aside.
    #[must_use]
    pub fn fit(width: u16, height: u16) -> Self {
        let rows = height.saturating_sub(HEADER_ROWS + 1).max(1); // + 1 for the status line
        #[allow(clippy::cast_possible_truncation)] // terminals aren't 256+ rows tall
        let rows = rows.min(u16::from(u8::MAX)) as u8;
        let half = rows / 2;
        let low = Pitch::A4.0.saturating_sub(half);
        let high = low.saturating_add(rows.saturating_sub(1));
        let cols = width.saturating_sub(GUTTER_COLS).max(1);
        Self {
            pitches: Pitch(low)..=Pitch(high),
            ticks: Tick(0)..Tick(cols),
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
/// A beat/step ruler above a grid of every visible pitch (labeled with its note name) by tick,
/// with the playhead and selection shown within it and a currently-sounding note picked out in an
/// accent color; a colored mode badge always shows the current mode, Helix-style; and — in
/// `Mode::Help` — a keybinding overlay on top of everything, the same way Helix's own help popups
/// sit over the buffer rather than replacing it.
pub fn render(frame: &mut Frame<'_>, editor: &Editor, playhead: Tick, viewport: &Viewport) {
    let area = frame.area();
    let [header_area, grid_area, status_area] = Layout::vertical([
        Constraint::Length(HEADER_ROWS),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(Paragraph::new(Vec::from(header(viewport))), header_area);
    frame.render_widget(Paragraph::new(grid(editor, playhead, viewport)), grid_area);
    frame.render_widget(Paragraph::new(status_line(editor)), status_area);

    if editor.mode == Mode::Help {
        render_help(frame, area);
    }
}

/// A note's first tick is marked distinctly from the rest of its span, so two same-pitch notes
/// placed back to back (no gap) still show a visible seam instead of merging into one block.
const NOTE_START: &str = "▐";
const NOTE_BODY: &str = "█";
const EMPTY: &str = "·";

/// The color a note is drawn in while it's actually sounding at the playhead — distinct from the
/// playhead column marker (`Modifier::REVERSED`) itself, which marks *where* the transport is
/// regardless of whether anything happens to be sounding there.
const PLAYING_COLOR: Color = Color::LightGreen;

/// The track this UI shows and edits — always the first one, until multi-track UI exists.
const TRACK: TrackId = TrackId(0);

/// Build the fixed-width label at the start of a row: a pitch name on a grid row,
/// `"beat"`/`"step"` on the ruler above it. Right-aligned against one column of padding — the `4`
/// here, plus that one column, is `GUTTER_COLS`.
fn gutter(label: &str) -> Span<'static> {
    Span::raw(format!("{label:>4} "))
}

/// Build one row: its gutter label, followed by one cell per visible tick — the shape shared by
/// every row this crate draws, ruler and grid alike.
fn row(label: &str, cells: impl Iterator<Item = Span<'static>>) -> Line<'static> {
    Line::from(
        std::iter::once(gutter(label))
            .chain(cells)
            .collect::<Vec<_>>(),
    )
}

/// Ticks per beat, and beats per bar — purely a display grouping for the ruler above the grid,
/// not a real tempo/time-signature concept (there's no tempo model at all yet, see
/// `osti_audio::TICK_SECS`'s own docs). Both stay single digits, so the ruler remains one
/// character per tick, same as the grid below it.
const STEPS_PER_BEAT: u16 = 4;
const BEATS_PER_BAR: u16 = 4;

/// Build the two-row ruler above the grid: which beat, and which step within it — read top to
/// bottom, coarse to fine, the way a time signature itself is read.
fn header(viewport: &Viewport) -> [Line<'static>; 2] {
    let beat = ruler_row("beat", viewport, |tick| {
        (tick.0 % STEPS_PER_BEAT == 0)
            .then(|| (((tick.0 / STEPS_PER_BEAT) % BEATS_PER_BAR) + 1).to_string())
    });
    let step = ruler_row("step", viewport, |tick| {
        Some(((tick.0 % STEPS_PER_BEAT) + 1).to_string())
    });
    [beat, step]
}

fn ruler_row(
    label: &str,
    viewport: &Viewport,
    mut cell: impl FnMut(Tick) -> Option<String>,
) -> Line<'static> {
    let cells = (viewport.ticks.start.0..viewport.ticks.end.0)
        .map(|raw_tick| Span::raw(cell(Tick(raw_tick)).unwrap_or_else(|| " ".to_string())));
    row(label, cells)
}

fn grid(editor: &Editor, playhead: Tick, viewport: &Viewport) -> Vec<Line<'static>> {
    let track = editor.playback.track(TRACK);
    let low = viewport.pitches.start().0;
    let high = viewport.pitches.end().0;

    // One bounded query per visible tick, not one per (pitch, tick) cell below — every row reuses
    // this instead of re-querying the same tick's chord once per pitch it happens to draw.
    let chords: Vec<Vec<Note>> = (viewport.ticks.start.0..viewport.ticks.end.0)
        .map(|raw_tick| track.sounding_at(Tick(raw_tick)).collect())
        .collect();
    let playing_chord: Vec<Note> = track.sounding_at(playhead).collect();

    (low..=high)
        .rev()
        .map(|raw_pitch| {
            let pitch = Pitch(raw_pitch);
            // At most one note per pitch can be sounding at any given tick (see `Track::insert`'s
            // own docs), so there's at most one note here to highlight as "currently playing".
            let playing = playing_chord
                .iter()
                .find(|note| note.position.pitch == pitch)
                .copied();
            let cells = chords
                .iter()
                .zip(viewport.ticks.start.0..viewport.ticks.end.0)
                .map(|(chord, raw_tick)| {
                    let tick = Tick(raw_tick);
                    let sounding = chord
                        .iter()
                        .find(|note| note.position.pitch == pitch)
                        .copied();
                    let symbol = match sounding {
                        Some(note) if note.position.tick == tick => NOTE_START,
                        Some(_) => NOTE_BODY,
                        None => EMPTY,
                    };

                    let mut style = Style::default();
                    if playing.is_some_and(|note| note.covers(tick)) {
                        style = style.fg(PLAYING_COLOR).add_modifier(Modifier::BOLD);
                    }
                    if tick == playhead {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    if editor
                        .selection
                        .ranges()
                        .any(|range| range.covers(Position { tick, pitch }))
                    {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(symbol, style)
                });
            row(&pitch.name(), cells)
        })
        .collect()
}

/// Return the status line's badge for a mode: its label, and its own accent color as the badge's
/// background — the same idea as Helix's own colored mode indicator (not the same literal
/// palette, which there depends on the active theme): one glance at the color says which mode
/// you're in.
const fn mode_badge(mode: Mode) -> (&'static str, Color) {
    match mode {
        Mode::Normal => ("NORMAL", Color::Blue),
        Mode::Insert => ("INSERT", Color::Green),
        Mode::Visual => ("VISUAL", Color::Yellow),
        Mode::Command => ("COMMAND", Color::Cyan),
        Mode::Help => ("HELP", Color::Magenta),
    }
}

fn status_line(editor: &Editor) -> Line<'static> {
    let (label, color) = mode_badge(editor.mode);
    let badge = Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(Color::Black)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    );
    if editor.mode == Mode::Command {
        Line::from(vec![badge, Span::raw(format!(" :{}", editor.command_line))])
    } else {
        Line::from(badge)
    }
}

/// Build the help overlay's content — computed from the same `Command` tables that drive
/// dispatch, not a separately maintained wall of text that could drift out of sync with the real
/// bindings.
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
    let label = match code {
        KeyCode::Char(' ') => "Space",
        KeyCode::Char(c) => return c.to_string(),
        KeyCode::Left => "←",
        KeyCode::Right => "→",
        KeyCode::Up => "↑",
        KeyCode::Down => "↓",
        KeyCode::Esc => "Esc",
        other => return format!("{other:?}"),
    };
    label.to_string()
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
    /// Return the key sequences that trigger this command — a chord is one sequence; several
    /// sequences are alternatives (e.g. `h` or `←`).
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

    /// Return a one-line description, used only by the help overlay.
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

/// Handle the `:` command line: characters build up `editor.command_line`, `Enter` executes it.
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
        assert_eq!(viewport.ticks, Tick(0)..Tick(11)); // 16 - GUTTER_COLS
        assert_eq!(viewport.pitches.start().0, Pitch::A4.0 - 5);
        assert_eq!(viewport.pitches.end().0, Pitch::A4.0 + 4); // 13 - HEADER_ROWS - 1, 10 rows
    }

    /// Return where a grid cell for `(tick, pitch)` lands in the rendered buffer, given
    /// `viewport`.
    fn cell(viewport: &Viewport, tick: u16, pitch: Pitch) -> (u16, u16) {
        let x = GUTTER_COLS + tick;
        let y = HEADER_ROWS + u16::from(viewport.pitches.end().0 - pitch.0);
        (x, y)
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
        let viewport = Viewport::fit(13, 15); // same 8 ticks x 12 pitches as before the ruler/gutter
        let mut terminal = Terminal::new(TestBackend::new(13, 15)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(0), &viewport))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[cell(&viewport, 2, Pitch::A4)].symbol(), NOTE_START);
        assert_eq!(buffer[cell(&viewport, 3, Pitch::A4)].symbol(), NOTE_BODY);
        assert_eq!(buffer[cell(&viewport, 4, Pitch::A4)].symbol(), NOTE_BODY);
        assert_eq!(buffer[cell(&viewport, 5, Pitch::A4)].symbol(), EMPTY);
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
        let viewport = Viewport::fit(13, 15);
        let mut terminal = Terminal::new(TestBackend::new(13, 15)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(10), &viewport)) // playhead elsewhere
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(
            (0..4)
                .map(|tick| buffer[cell(&viewport, tick, Pitch::A4)].symbol())
                .collect::<Vec<_>>(),
            vec![NOTE_START, NOTE_BODY, NOTE_START, NOTE_BODY],
        );
    }

    #[test]
    fn a_sounding_note_is_highlighted_at_the_playhead_but_not_elsewhere() {
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(2),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(3),
        }));
        let viewport = Viewport::fit(13, 15);
        let mut terminal = Terminal::new(TestBackend::new(13, 15)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(3), &viewport)) // inside the note's span
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[cell(&viewport, 2, Pitch::A4)].fg, PLAYING_COLOR);
        assert_eq!(buffer[cell(&viewport, 4, Pitch::A4)].fg, PLAYING_COLOR);
        assert_ne!(buffer[cell(&viewport, 7, Pitch::A4)].fg, PLAYING_COLOR); // outside the note

        // A different, silent pitch at the very same tick is never highlighted just because the
        // playhead is passing through its column.
        let other_pitch = Pitch(Pitch::A4.0 - 1);
        assert_ne!(buffer[cell(&viewport, 3, other_pitch)].fg, PLAYING_COLOR);
    }

    #[test]
    fn the_gutter_shows_each_row_its_own_pitch_name() {
        let editor = Editor::new();
        let viewport = Viewport::fit(13, 15);
        let mut terminal = Terminal::new(TestBackend::new(13, 15)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(0), &viewport))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let (_, a4_row) = cell(&viewport, 0, Pitch::A4);
        let label: String = (0..GUTTER_COLS)
            .map(|x| buffer[(x, a4_row)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert_eq!(label.trim(), "A4");
    }

    #[test]
    fn the_ruler_counts_steps_within_each_beat() {
        let editor = Editor::new();
        let viewport = Viewport::fit(13, 15);
        let mut terminal = Terminal::new(TestBackend::new(13, 15)).unwrap();

        terminal
            .draw(|frame| render(frame, &editor, Tick(0), &viewport))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let step_row = HEADER_ROWS - 1;
        let steps: String = (0..4)
            .map(|tick| {
                buffer[(GUTTER_COLS + tick, step_row)]
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        assert_eq!(steps, "1234");
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
    fn b_and_e_jump_between_note_boundaries() {
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(5),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(3), // covers ticks 5..7
        }));
        let mut keymap = Keymap::default();

        let jump_end = feed(&mut keymap, &editor, KeyCode::Char('e')).unwrap();
        editor.update(jump_end);
        assert_eq!(editor.selection.primary().head, Tick(7)); // the note's own end

        let jump_start = feed(&mut keymap, &editor, KeyCode::Char('b')).unwrap();
        editor.update(jump_start);
        assert_eq!(editor.selection.primary().head, Tick(5)); // the note's own start
    }

    #[test]
    fn u_and_shift_u_map_to_undo_and_redo() {
        let editor = Editor::new();
        let mut keymap = Keymap::default();

        assert_eq!(
            feed(&mut keymap, &editor, KeyCode::Char('u')),
            Some(Action::Undo)
        );
        assert_eq!(
            feed(&mut keymap, &editor, KeyCode::Char('U')),
            Some(Action::Redo)
        );
    }

    #[test]
    fn j_and_k_move_the_cursor_between_pitch_rows() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();

        let down = feed(&mut keymap, &editor, KeyCode::Char('j')).unwrap();
        editor.update(down);
        assert_eq!(editor.selection.primary().pitch, Pitch(Pitch::A4.0 - 1));

        let up = feed(&mut keymap, &editor, KeyCode::Char('k')).unwrap();
        editor.update(up);
        assert_eq!(editor.selection.primary().pitch, Pitch::A4);
    }

    #[test]
    fn g_then_e_goes_to_the_end_of_the_viewport() {
        let mut editor = Editor::new();
        let mut keymap = Keymap::default();

        assert!(feed(&mut keymap, &editor, KeyCode::Char('g')).is_none());
        let action = feed(&mut keymap, &editor, KeyCode::Char('e')).unwrap();
        editor.update(action);

        assert_eq!(editor.selection.primary().head, Tick(10)); // viewport.ticks.end - 1
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
    fn escape_returns_to_normal_mode() {
        let mut editor = Editor::new();
        editor.update(Action::SetMode(Mode::Insert));
        let mut keymap = Keymap::default();

        let action = feed(&mut keymap, &editor, KeyCode::Esc).unwrap();
        editor.update(action);

        assert_eq!(editor.mode, Mode::Normal);
    }

    #[test]
    fn space_toggles_play_pause_in_normal_mode() {
        let editor = Editor::new(); // starts paused
        let mut keymap = Keymap::default();

        let action = feed(&mut keymap, &editor, KeyCode::Char(' ')).unwrap();

        assert_eq!(
            action,
            Action::Playback(PlaybackAction::SetPlaybackIntent(
                osti_core::PlaybackIntent::Playing
            ))
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
    fn d_deletes_the_note_at_the_cursor() {
        let mut editor = Editor::new();
        editor.update(Action::Playback(PlaybackAction::InsertNote {
            track: TrackId(0),
            at: osti_core::Position {
                tick: Tick(0),
                pitch: Pitch::A4,
            },
            length: osti_core::Length(2),
        }));
        let mut keymap = Keymap::default();

        let action = feed(&mut keymap, &editor, KeyCode::Char('d')).unwrap();
        editor.update(action);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            0
        );
    }

    #[test]
    fn visual_mode_delete_removes_every_note_the_selection_spans() {
        let mut editor = Editor::new();
        for tick in [0, 1] {
            editor.update(Action::Playback(PlaybackAction::InsertNote {
                track: TrackId(0),
                at: osti_core::Position {
                    tick: Tick(tick),
                    pitch: Pitch::A4,
                },
                length: osti_core::Length(1),
            }));
        }
        editor.update(Action::SetMode(Mode::Visual));
        let mut keymap = Keymap::default();

        let extend = feed(&mut keymap, &editor, KeyCode::Char('l')).unwrap(); // cover both notes
        editor.update(extend);
        let delete = feed(&mut keymap, &editor, KeyCode::Char('d')).unwrap();
        editor.update(delete);

        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(0)).count(),
            0
        );
        assert_eq!(
            editor.playback.tracks.first().sounding_at(Tick(1)).count(),
            0
        );
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
