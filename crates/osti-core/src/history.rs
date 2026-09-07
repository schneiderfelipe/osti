//! Undo/redo, as a self-contained abstraction `Editor::update` delegates to.

use crate::action::Action;

/// Where an action being applied came from — decides which stack (if any) its inverse belongs on
/// once it's known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// A brand new action, not a replay.
    Fresh,
    /// Replaying the top of the undo stack.
    Undo,
    /// Replaying the top of the redo stack.
    Redo,
}

/// Two stacks of inverse actions, plus the decision of what an incoming action actually means.
///
/// `Editor::update` has exactly one job left once this exists: apply one concrete action and
/// report its inverse (if any) — deciding *which* concrete action to apply (a fresh one, or
/// whatever `Undo`/`Redo` should replay) and *where the inverse goes* both live here instead.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Action>,
    redo: Vec<Action>,
}

/// A resolved action, ready to apply, and a receipt to settle once its inverse is known.
pub struct Resolved {
    /// The concrete action to actually apply.
    pub action: Action,
    origin: Origin,
}

impl History {
    /// Turn a requested action into the concrete action to apply.
    ///
    /// For `Undo`/`Redo`, that's whatever is on top of the corresponding stack — `None` if it's
    /// empty, there's nothing to replay. For anything else, it's `action` itself, unchanged.
    pub fn resolve(&mut self, action: Action) -> Option<Resolved> {
        match action {
            Action::Undo => Some(Resolved {
                action: self.undo.pop()?,
                origin: Origin::Undo,
            }),
            Action::Redo => Some(Resolved {
                action: self.redo.pop()?,
                origin: Origin::Redo,
            }),
            other => Some(Resolved {
                action: other,
                origin: Origin::Fresh,
            }),
        }
    }

    /// Record the inverse of whatever was just applied, in whichever stack `resolved` came from.
    ///
    /// A fresh action's inverse goes on the undo stack (clearing redo — a fresh edit abandons
    /// whatever was available to redo, same as everywhere else this pattern shows up); undoing
    /// something pushes what *that* undoes onto redo, and vice versa. Nothing to do if the action
    /// wasn't undoable (`inverse` is `None`).
    pub fn settle(&mut self, resolved: &Resolved, inverse: Option<Action>) {
        let Some(inverse) = inverse else {
            return;
        };
        match resolved.origin {
            Origin::Fresh => {
                self.redo.clear();
                self.undo.push(inverse);
            }
            Origin::Undo => self.redo.push(inverse),
            Origin::Redo => self.undo.push(inverse),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // each `resolve` here is set up to have something to resolve
mod tests {
    use super::*;
    use crate::mode::Mode;

    fn action(mode: Mode) -> Action {
        Action::SetMode(mode)
    }

    #[test]
    fn a_fresh_action_is_resolved_unchanged() {
        let mut history = History::default();
        let resolved = history.resolve(action(Mode::Insert)).unwrap();
        assert_eq!(resolved.action, action(Mode::Insert));
    }

    #[test]
    fn undo_with_nothing_recorded_resolves_to_nothing() {
        let mut history = History::default();
        assert!(history.resolve(Action::Undo).is_none());
    }

    #[test]
    fn undo_replays_the_most_recently_recorded_inverse() {
        let mut history = History::default();
        let resolved = history.resolve(action(Mode::Insert)).unwrap();
        history.settle(&resolved, Some(action(Mode::Normal)));

        let undo = history.resolve(Action::Undo).unwrap();

        assert_eq!(undo.action, action(Mode::Normal));
    }

    #[test]
    fn undoing_then_redoing_restores_the_original_action() {
        let mut history = History::default();
        let resolved = history.resolve(action(Mode::Insert)).unwrap();
        history.settle(&resolved, Some(action(Mode::Normal)));
        let undo = history.resolve(Action::Undo).unwrap();
        history.settle(&undo, Some(action(Mode::Insert)));

        let redo = history.resolve(Action::Redo).unwrap();

        assert_eq!(redo.action, action(Mode::Insert));
    }

    #[test]
    fn a_fresh_action_after_an_undo_clears_redo() {
        let mut history = History::default();
        let resolved = history.resolve(action(Mode::Insert)).unwrap();
        history.settle(&resolved, Some(action(Mode::Normal)));
        let undo = history.resolve(Action::Undo).unwrap();
        history.settle(&undo, Some(action(Mode::Insert)));

        let fresh = history.resolve(action(Mode::Help)).unwrap();
        history.settle(&fresh, Some(action(Mode::Normal)));

        assert!(history.resolve(Action::Redo).is_none());
    }

    #[test]
    fn settling_a_non_undoable_action_records_nothing() {
        let mut history = History::default();
        let resolved = history.resolve(action(Mode::Insert)).unwrap();

        history.settle(&resolved, None);

        assert!(history.resolve(Action::Undo).is_none());
    }
}
