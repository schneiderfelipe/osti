//! Undo/redo: two stacks of inverse actions, owned by `Editor`.

use crate::action::Action;

/// Two stacks of inverse actions.
///
/// Recording an action's inverse, then undoing, then redoing all use the same idea: applying an
/// inverse produces a further inverse of its own (undoing an undo is a redo, and vice versa), so
/// each direction just needs to pop from one stack and push onto the other.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Action>,
    redo: Vec<Action>,
}

impl History {
    /// Record a fresh action's inverse. Clears redo — a fresh edit after an undo abandons
    /// whatever was available to redo, same as everywhere else this shows up.
    pub fn record(&mut self, inverse: Action) {
        self.redo.clear();
        self.undo.push(inverse);
    }

    /// Pop the most recent inverse off the undo stack, if any.
    pub fn pop_undo(&mut self) -> Option<Action> {
        self.undo.pop()
    }

    /// Pop the most recent inverse off the redo stack, if any.
    pub fn pop_redo(&mut self) -> Option<Action> {
        self.redo.pop()
    }

    /// Push onto the undo stack, without touching redo (unlike `record`) — used when replaying a
    /// redo, which shouldn't clear anything.
    pub fn push_undo(&mut self, action: Action) {
        self.undo.push(action);
    }

    /// Push onto the redo stack — used when replaying an undo.
    pub fn push_redo(&mut self, action: Action) {
        self.redo.push(action);
    }
}
