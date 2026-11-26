//! The key map: one table drives both `App::key` and the key help.

use crate::{KeyCode, KeyEvent, KeyModifiers};

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Quit,
    ToggleHelp,
    /// Closes the help (and, later, overlays and screens).
    Back,
}

/// A row of the key map: the label and description shown in the help, the action, the keys that trigger it.
pub(crate) struct Binding {
    pub(crate) label: &'static str,
    pub(crate) what: &'static str,
    action: Action,
    keys: &'static [(KeyCode, KeyModifiers)],
}

/// Every key the application handles, in help order.
pub(crate) const BINDINGS: [Binding; 4] = [
    Binding {
        label: "?",
        what: "Toggle this help",
        action: Action::ToggleHelp,
        keys: &[
            (KeyCode::Char('?'), KeyModifiers::NONE),
            (KeyCode::Char('?'), KeyModifiers::SHIFT),
        ],
    },
    Binding {
        label: "Esc",
        what: "Back / close this help",
        action: Action::Back,
        keys: &[(KeyCode::Esc, KeyModifiers::NONE)],
    },
    Binding {
        label: "q",
        what: "Quit",
        action: Action::Quit,
        keys: &[(KeyCode::Char('q'), KeyModifiers::NONE)],
    },
    Binding {
        label: "Ctrl-C",
        what: "Quit",
        action: Action::Quit,
        keys: &[(KeyCode::Char('c'), KeyModifiers::CONTROL)],
    },
];

/// The action bound to `key`, if any.
pub(crate) fn action(key: KeyEvent) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|binding| binding.keys.contains(&(key.code, key.modifiers)))
        .map(|binding| binding.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_has_a_key_that_triggers_its_action() {
        for binding in &BINDINGS {
            let &(code, modifiers) = binding.keys.first().expect("a key");
            assert_eq!(action(KeyEvent::new(code, modifiers)), Some(binding.action));
        }
    }

    #[test]
    fn an_unbound_key_does_nothing() {
        assert_eq!(
            action(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            action(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            None
        );
    }
}
