//! Editing mode — which keys mean what.

/// The editor's current mode, following the Vim/Kakoune/Helix lineage DESIGN.md draws on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Movement and commands, not insertion.
    #[default]
    Normal,
    /// Inserting a note.
    Insert,
    /// Movement extends the selection instead of moving it (Helix's select mode).
    Visual,
    /// Typing a command on the `:` command line.
    Command,
    /// Showing the keybinding help overlay.
    Help,
}
