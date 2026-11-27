//! Key maps: one table per screen plus the global one; each table drives both key handling and the `?` help.
//!
//! Reserved keys (ADR 0002): `?`, `q` and `Ctrl-C` belong to the global table and no screen table may bind them;
//! `Esc` is global (back / close the help) unless the screen handles it first (a view returns `Flow::Ignored`
//! for keys it does not use, then the global table is tried).

use crate::{KeyCode, KeyEvent, KeyModifiers};

/// One row of the `?` help.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HelpRow {
    /// Key label, for example `Enter` or `Ctrl-C` (at most 8 characters).
    pub(crate) label: &'static str,
    /// What it does, in English, at most 26 characters (the help box is 36 columns wide).
    pub(crate) what: &'static str,
}

/// A row of a key map: its help row, the action, the keys that trigger it.
pub(crate) struct Binding<A> {
    pub(crate) label: &'static str,
    pub(crate) what: &'static str,
    pub(crate) action: A,
    pub(crate) keys: &'static [(KeyCode, KeyModifiers)],
}

impl<A> Binding<A> {
    /// The help row of this binding.
    pub(crate) fn help_row(&self) -> HelpRow {
        HelpRow {
            label: self.label,
            what: self.what,
        }
    }
}

/// The action bound to `key` in `table` (first match), if any. Modifiers are compared exactly.
pub(crate) fn lookup<A: Copy>(table: &[Binding<A>], key: KeyEvent) -> Option<A> {
    table
        .iter()
        .find(|binding| binding.keys.contains(&(key.code, key.modifiers)))
        .map(|binding| binding.action)
}

/// The help rows of `table`, in order.
pub(crate) fn help_rows<A>(table: &[Binding<A>]) -> Vec<HelpRow> {
    table.iter().map(Binding::help_row).collect()
}

/// `true` if `table` binds one of the reserved keys (`?`, `q`, `Ctrl-C`); each screen's tests assert it is not.
#[cfg(test)]
pub(crate) fn uses_reserved_key<A>(table: &[Binding<A>]) -> bool {
    table.iter().any(|binding| {
        binding.keys.iter().any(|&(code, modifiers)| {
            matches!(code, KeyCode::Char('?' | 'q'))
                || (code == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL)
        })
    })
}

/// Global actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Global {
    Quit,
    ToggleHelp,
    /// Closes the help, else goes back to the previous screen (nothing on the Repo list).
    Back,
}

/// The global key map, shown last in the help.
pub(crate) const GLOBAL: [Binding<Global>; 4] = [
    Binding {
        label: "? / F1",
        what: "Toggle this help",
        action: Global::ToggleHelp,
        keys: &[
            (KeyCode::Char('?'), KeyModifiers::NONE),
            (KeyCode::Char('?'), KeyModifiers::SHIFT),
            (KeyCode::F(1), KeyModifiers::NONE),
        ],
    },
    Binding {
        label: "Esc",
        what: "Back / close this help",
        action: Global::Back,
        keys: &[(KeyCode::Esc, KeyModifiers::NONE)],
    },
    Binding {
        label: "q",
        what: "Quit",
        action: Global::Quit,
        keys: &[(KeyCode::Char('q'), KeyModifiers::NONE)],
    },
    Binding {
        label: "Ctrl-C",
        what: "Quit",
        action: Global::Quit,
        keys: &[(KeyCode::Char('c'), KeyModifiers::CONTROL)],
    },
];

/// `true` for `F1`, which opens the help from anywhere, text entry included (`?` is text there).
pub(crate) fn is_f1(key: KeyEvent) -> bool {
    key.code == KeyCode::F(1) && key.modifiers == KeyModifiers::NONE
}

/// `true` for `Ctrl-C`, which quits from anywhere, text entry included.
pub(crate) fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_global_binding_has_a_key_that_triggers_its_action() {
        for binding in &GLOBAL {
            let &(code, modifiers) = binding.keys.first().expect("a key");
            assert_eq!(
                lookup(&GLOBAL, KeyEvent::new(code, modifiers)),
                Some(binding.action)
            );
        }
    }

    #[test]
    fn an_unbound_key_does_nothing() {
        assert_eq!(
            lookup(
                &GLOBAL,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
            ),
            None
        );
        assert_eq!(
            lookup(
                &GLOBAL,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    #[test]
    fn reserved_keys_are_detected() {
        assert!(uses_reserved_key(&GLOBAL));
        let table = [Binding {
            label: "r",
            what: "Refresh",
            action: (),
            keys: &[(KeyCode::Char('r'), KeyModifiers::NONE)],
        }];
        assert!(!uses_reserved_key(&table));
    }
}
