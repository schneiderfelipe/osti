//! Editing mode — which keys mean what.

/// The editor's current mode, following the Vim/Kakoune/Helix lineage DESIGN.md draws on: a
/// handful of modes change what's on screen, normal mode is for movement and non-inserting
/// commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Movement and commands, not insertion.
    #[default]
    Normal,
    /// Inserting a note.
    Insert,
}
