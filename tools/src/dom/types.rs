use std::collections::HashSet;
use std::fmt;
use std::fmt::{Display, Formatter};

use indexmap::IndexMap;

use super::diagnostic::Diagnostic;
use super::directive::{loc, ConflictGroup, DirectiveItem};
use super::production::Production;

/// The complete grammar: all productions and any declared conflict or inline groups.
pub struct Grammar {
    /// Named grammar rules in declaration order, keyed by rule name for O(1) lookup.
    pub productions: IndexMap<String, Production>,
    /// Explicit root rule declared with `%axiom`, if any.
    ///
    /// Private so all consumers go through [`root_rule`](Self::root_rule) for
    /// start-symbol resolution; the raw directive is reachable in-crate via
    /// [`axiom_directive`](Self::axiom_directive) for line-number diagnostics.
    axiom: Option<DirectiveItem>,
    /// Conflict groups declared with `%conflicts`.
    pub conflicts: Vec<ConflictGroup>,
    /// Rule names declared with `%inline`.
    pub inline: Vec<DirectiveItem>,
    /// Rule names declared with `%supertypes`.
    pub supertypes: Vec<DirectiveItem>,
    /// Items declared with `%extras` (regex patterns or rule names).
    pub extras: Vec<DirectiveItem>,
    /// All non-terminal names that appear on right-hand sides of rules, accumulated by the visitor.
    pub rhs_nonterminals: HashSet<String>,
    /// Diagnostics accumulated during parsing (before cross-reference checks).
    pub parse_diagnostics: Vec<Diagnostic>,
}

impl Default for Grammar {
    fn default() -> Self {
        Self::new()
    }
}

impl Grammar {
    /// Creates an empty grammar with no productions, conflicts, inline, supertypes, or extras.
    pub fn new() -> Self {
        Self {
            productions: IndexMap::new(),
            axiom: None,
            conflicts: Vec::new(),
            inline: Vec::new(),
            supertypes: Vec::new(),
            extras: Vec::new(),
            rhs_nonterminals: HashSet::new(),
            parse_diagnostics: Vec::new(),
        }
    }

    /// Creates a grammar pre-populated with the given productions in iteration order.
    pub fn from_rules(productions: impl IntoIterator<Item = Production>) -> Self {
        let mut g = Self::new();
        for p in productions {
            g.productions.insert(p.name.clone(), p);
        }
        g
    }

    /// Returns the set of all defined rule names, used for cross-reference checks.
    pub fn known_rules(&self) -> HashSet<&str> {
        self.productions.keys().map(|k| k.as_str()).collect()
    }

    /// Returns the grammar's start symbol: the rule named by `%axiom` when
    /// declared and defined as a production, otherwise the first production in
    /// declaration order, or `None` for an empty grammar.
    ///
    /// An `%axiom` naming an undefined rule is treated as absent (graceful
    /// fallback); reporting that case as an error is `axiom_check`'s job.
    pub fn root_rule(&self) -> Option<&str> {
        self.axiom
            .as_ref()
            .map(|item| item.name.as_str())
            .filter(|name| self.productions.contains_key(*name))
            .or_else(|| self.productions.keys().next().map(String::as_str))
    }

    /// Records the `%axiom` directive, enforcing first-declaration-wins.
    ///
    /// Returns an error diagnostic when an axiom is already declared; the
    /// existing declaration is kept and the incoming one is discarded.
    pub(crate) fn declare_axiom(&mut self, item: DirectiveItem) -> Option<Diagnostic> {
        if self.axiom.is_some() {
            return Some(Diagnostic::error(format!(
                "%axiom declared more than once ({})",
                loc(&item.filename, item.line)
            )));
        }
        self.axiom = Some(item);
        None
    }

    /// Returns the raw `%axiom` directive, for line-number diagnostics and
    /// directive emission; start-symbol resolution belongs to [`root_rule`](Self::root_rule).
    pub(crate) fn axiom_directive(&self) -> Option<&DirectiveItem> {
        self.axiom.as_ref()
    }

    /// Returns a mutable reference to the `%axiom` directive, for rule renaming.
    pub(crate) fn axiom_directive_mut(&mut self) -> Option<&mut DirectiveItem> {
        self.axiom.as_mut()
    }

    /// Removes and returns the `%axiom` directive, for `%include` merging.
    pub(crate) fn take_axiom(&mut self) -> Option<DirectiveItem> {
        self.axiom.take()
    }
}

impl Display for Grammar {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for production in self.productions.values() {
            write!(fmt, "\n{}", production)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{di, p};
    use crate::dom::GrammarNode::TerminalLiteral;

    /// Builds a grammar with rules `a` and `b` in that order.
    fn ab() -> Grammar {
        Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", TerminalLiteral("'y'".into())),
        ])
    }

    #[test]
    fn root_rule_is_axiom_when_defined() {
        let mut g = ab();
        g.declare_axiom(di("b", 1));
        assert_eq!(g.root_rule(), Some("b"));
    }

    #[test]
    fn root_rule_falls_back_to_first_production_when_axiom_undefined() {
        let mut g = ab();
        g.declare_axiom(di("ghost", 1));
        assert_eq!(g.root_rule(), Some("a"));
    }

    #[test]
    fn root_rule_is_first_production_without_axiom() {
        assert_eq!(ab().root_rule(), Some("a"));
    }

    #[test]
    fn root_rule_is_none_for_empty_grammar() {
        assert_eq!(Grammar::new().root_rule(), None);
    }

    #[test]
    fn declare_axiom_first_declaration_wins() {
        let mut g = ab();
        assert!(g.declare_axiom(di("a", 1)).is_none());
        let diag = g.declare_axiom(di("b", 2)).expect("duplicate must error");
        assert!(diag.message.contains("%axiom declared more than once"));
        assert_eq!(g.root_rule(), Some("a"));
    }
}
