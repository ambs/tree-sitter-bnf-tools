use crate::dom::ReservedEntry;

use super::directive::{ConflictGroup, DirectiveItem, NameOrLiteral, PrecedenceGroup};
use super::{GrammarNode, Production};

/// Creates a [`Production`] with realistic defaults (`line: 1`, `filename: "test.bnf"`) for use in tests.
pub fn p(name: &str, body: GrammarNode) -> Production {
    Production {
        name: name.into(),
        body,
        line: 1,
        filename: "test.bnf".into(),
    }
}

/// Creates a [`Production`] with a specific filename, for tests that need to vary the source file.
pub fn p_named(name: &str, body: GrammarNode, filename: &str) -> Production {
    Production {
        name: name.into(),
        body,
        line: 1,
        filename: filename.into(),
    }
}

/// Creates a [`ReservedEntry`] with the given set name, rule names and source line (no filename).
pub fn re(set_name: &str, rule_names: &[NameOrLiteral], line: usize) -> ReservedEntry {
    ReservedEntry {
        set_name: set_name.into(),
        rule_names: rule_names.to_vec(),
        line,
        filename: String::new(),
    }
}

/// Creates a [`DirectiveItem`] with the given name and source line (no filename).
pub fn di(name: &str, line: usize) -> DirectiveItem {
    DirectiveItem {
        name: name.into(),
        line,
        filename: String::new(),
    }
}

/// Creates a [`GrammarNode::NonTerminal`] with the given name.
pub fn nt(name: &str) -> GrammarNode {
    GrammarNode::NonTerminal(name.into())
}

/// Creates a [`ConflictGroup`] with the given rule names and source line (no filename).
pub fn cg(rules: &[&str], line: usize) -> ConflictGroup {
    ConflictGroup {
        rules: rules.iter().map(|s| s.to_string()).collect(),
        line,
        filename: String::new(),
    }
}

/// Creates a [`PrecedenceGroup`] with the given items and source line (no filename).
pub fn pg(items: &[NameOrLiteral], line: usize) -> PrecedenceGroup {
    PrecedenceGroup {
        items: items.to_vec(),
        line,
        filename: String::new(),
    }
}
