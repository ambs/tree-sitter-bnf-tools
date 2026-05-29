use super::directive::{ConflictGroup, DirectiveItem};
use super::{GrammarNode, Production};

/// Creates a [`Production`] with `line: 0` for use in tests.
pub fn p(name: &str, body: GrammarNode) -> Production {
    Production {
        name: name.into(),
        body,
        line: 0,
        filename: String::new(),
    }
}

/// Creates a [`DirectiveItem`] with the given name and source line.
pub fn di(name: &str, line: usize) -> DirectiveItem {
    DirectiveItem {
        name: name.into(),
        line,
    }
}

/// Creates a [`ConflictGroup`] with the given rule names and source line.
pub fn cg(rules: &[&str], line: usize) -> ConflictGroup {
    ConflictGroup {
        rules: rules.iter().map(|s| s.to_string()).collect(),
        line,
    }
}
