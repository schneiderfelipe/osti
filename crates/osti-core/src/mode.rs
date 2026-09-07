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

impl Mode {
    /// Toggle into `target`: return to `Normal` if already there, switch to `target` otherwise —
    /// how both `v` (`Visual`) and `?` (`Help`) behave, press once to enter, again to leave.
    #[must_use]
    pub fn toggled(self, target: Self) -> Self {
        if self == target { Self::Normal } else { target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_into_the_current_mode_returns_to_normal() {
        assert_eq!(Mode::Visual.toggled(Mode::Visual), Mode::Normal);
    }

    #[test]
    fn toggling_into_a_different_mode_switches_to_it() {
        assert_eq!(Mode::Normal.toggled(Mode::Help), Mode::Help);
    }
}
