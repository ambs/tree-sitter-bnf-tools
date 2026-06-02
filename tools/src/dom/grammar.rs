use std::collections::HashSet;

use super::analysis::left_recursive_rules;
use super::diagnostic::Diagnostic;
use super::directive::{ConflictGroup, DirectiveItem};
use super::types::Grammar;

impl Grammar {
    /// Returns a warning for every rule name in `%conflicts` that has no definition.
    fn conflicts_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.conflicts
            .iter()
            .flat_map(|ConflictGroup { rules, line }| {
                rules.iter().filter_map(move |name| {
                    if known.contains(name.as_str()) {
                        return None;
                    }
                    Some(Diagnostic::warning(format!(
                        "%conflicts references undefined rule '{name}' (line {line})"
                    )))
                })
            })
            .collect()
    }

    /// Returns a warning for every rule name in `%inline` that has no definition.
    fn inline_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.inline
            .iter()
            .filter(|item| !known.contains(item.name.as_str()))
            .map(|DirectiveItem { name, line }| {
                Diagnostic::warning(format!(
                    "%inline references undefined rule '{name}' (line {line})"
                ))
            })
            .collect()
    }

    /// Returns a warning for every rule name in `%supertypes` that has no definition.
    fn supertypes_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.supertypes
            .iter()
            .filter(|item| !known.contains(item.name.as_str()))
            .map(|DirectiveItem { name, line }| {
                Diagnostic::warning(format!(
                    "%supertypes references undefined rule '{name}' (line {line})"
                ))
            })
            .collect()
    }

    /// Returns a warning for every rule reference in `%extras` that has no definition.
    fn extras_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.extras
            .iter()
            .filter(|item| !item.name.starts_with('/') && !known.contains(item.name.as_str()))
            .map(|DirectiveItem { name, line }| {
                Diagnostic::warning(format!(
                    "%extras references undefined rule '{name}' (line {line})"
                ))
            })
            .collect()
    }

    /// Returns a warning for every non-terminal referenced in a rule body that has no definition.
    fn undefined_refs_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.rhs_nonterminals
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| Diagnostic::warning(format!("undefined rule reference '{name}'")))
            .collect()
    }

    /// Returns an error for every left-recursive rule (direct or mutual).
    ///
    /// Left-recursion is an error, not a warning, because tree-sitter cannot generate a
    /// parser for a left-recursive grammar regardless of any other options.
    fn left_recursive_check(&self) -> Vec<Diagnostic> {
        left_recursive_rules(self)
            .into_iter()
            .map(|(rule, is_direct)| {
                let kind = if is_direct { "directly" } else { "mutually" };
                let line = self.productions.get(rule).map(|p| p.line).unwrap_or(0);
                Diagnostic::error(format!(
                    "rule '{rule}' is {kind} left-recursive (line {line})"
                ))
            })
            .collect()
    }

    /// Returns a warning for every rule that is never referenced by any other rule or directive.
    ///
    /// The first rule (root) is exempt — it is the implicit entry point.
    /// Rules mentioned in `%extras` are also exempt: they are legitimately used
    /// without appearing in any rule body (e.g. whitespace handlers).
    fn unreachable_rules_check(&self) -> Vec<Diagnostic> {
        let mut rules = self.productions.keys();
        let Some(_root) = rules.next() else {
            return vec![];
        };

        let mut referenced: std::collections::HashSet<&str> =
            self.rhs_nonterminals.iter().map(String::as_str).collect();
        for item in self.extras.iter().filter(|i| !i.name.starts_with('/')) {
            referenced.insert(&item.name);
        }

        rules
            .filter(|name| !referenced.contains(name.as_str()))
            .map(|name| {
                let line = self
                    .productions
                    .get(name.as_str())
                    .map(|p| p.line)
                    .unwrap_or(0);
                Diagnostic::warning(format!("rule '{name}' is never referenced (line {line})"))
            })
            .collect()
    }

    /// Runs all cross-reference checks and returns any diagnostics.
    ///
    /// Diagnostics are sorted by message so output is stable across runs regardless
    /// of `HashSet` iteration order in the individual checks.
    pub fn check(&self) -> Vec<Diagnostic> {
        let known = self.known_rules();
        let mut diagnostics = Vec::new();
        diagnostics.extend(self.parse_diagnostics.iter().cloned());
        diagnostics.extend(self.conflicts_check(&known));
        diagnostics.extend(self.inline_check(&known));
        diagnostics.extend(self.supertypes_check(&known));
        diagnostics.extend(self.extras_check(&known));
        diagnostics.extend(self.undefined_refs_check(&known));
        diagnostics.extend(self.unreachable_rules_check());
        diagnostics.extend(self.left_recursive_check());
        diagnostics.sort_by(|a, b| a.message.cmp(&b.message));
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{cg, di, p};
    use crate::dom::GrammarNode::TerminalLiteral;
    use crate::dom::{GrammarNode, Severity};

    /// Renders each diagnostic to its full display string for easy comparison.
    fn strs(diagnostics: &[Diagnostic]) -> Vec<String> {
        diagnostics.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn grammar_display() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", GrammarNode::NonTerminal("a".into())),
        ]);
        assert_eq!(g.to_string(), "\na -> 'x'\nb -> $.a");
    }

    #[test]
    fn conflicts_check_warns_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.conflicts = vec![cg(&["a", "ghost"], 0)];
        assert_eq!(
            strs(&g.conflicts_check(&g.known_rules())),
            vec!["warning: %conflicts references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn conflicts_check_no_warnings_when_all_rules_defined() {
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", TerminalLiteral("'y'".into())),
        ]);
        g.conflicts = vec![cg(&["a", "b"], 0)];
        assert!(g.conflicts_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn supertypes_check_warns_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.supertypes = vec![di("ghost", 0)];
        assert_eq!(
            strs(&g.supertypes_check(&g.known_rules())),
            vec!["warning: %supertypes references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn supertypes_check_no_warnings_when_all_rules_defined() {
        let mut g = Grammar::from_rules([p("expression", TerminalLiteral("'x'".into()))]);
        g.supertypes = vec![di("expression", 0)];
        assert!(g.supertypes_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn inline_check_warns_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.inline = vec![di("ghost", 0)];
        assert_eq!(
            strs(&g.inline_check(&g.known_rules())),
            vec!["warning: %inline references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn inline_check_no_warnings_when_all_rules_defined() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.inline = vec![di("a", 0)];
        assert!(g.inline_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn extras_check_warns_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.extras = vec![di("/\\s/", 0), di("ghost", 0)];
        assert_eq!(
            strs(&g.extras_check(&g.known_rules())),
            vec!["warning: %extras references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn extras_check_no_warning_for_pattern() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.extras = vec![di("/\\s/", 0)];
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn extras_check_no_warnings_when_rule_defined() {
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("comment", TerminalLiteral("'#'".into())),
        ]);
        g.extras = vec![di("/\\s/", 0), di("comment", 0)];
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn undefined_refs_check_warns_on_missing_rule() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("term".into());
        assert_eq!(
            strs(&g.undefined_refs_check(&g.known_rules())),
            vec!["warning: undefined rule reference 'term'"]
        );
    }

    #[test]
    fn undefined_refs_check_no_warning_when_defined() {
        let mut g = Grammar::from_rules([p("term", TerminalLiteral("'x'".into()))]);
        g.rhs_nonterminals.insert("term".into());
        assert!(g.undefined_refs_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn undefined_refs_check_deduplicates() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("ghost".into());
        assert_eq!(g.undefined_refs_check(&g.known_rules()).len(), 1);
    }

    #[test]
    fn check_diagnostics_are_sorted_by_message() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("zebra".into());
        g.rhs_nonterminals.insert("alpha".into());
        g.rhs_nonterminals.insert("middle".into());
        let messages: Vec<String> = g.check().iter().map(|d| d.message.clone()).collect();
        let mut sorted = messages.clone();
        sorted.sort();
        assert_eq!(messages, sorted);
    }

    #[test]
    fn check_detects_direct_left_recursion() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar::from_rules([p(
            "expr",
            Choice(vec![
                Sequence(vec![
                    NonTerminal("expr".into()),
                    TerminalLiteral("'+'".into()),
                    TerminalLiteral("'n'".into()),
                ]),
                TerminalLiteral("'n'".into()),
            ]),
        )]);
        let diagnostics = g.check();
        assert!(diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && d.message.contains("expr")
                && d.message.contains("directly left-recursive")
        }));
    }

    #[test]
    fn check_detects_mutual_left_recursion() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar::from_rules([
            p(
                "a",
                Choice(vec![
                    Sequence(vec![NonTerminal("b".into()), TerminalLiteral("'x'".into())]),
                    TerminalLiteral("'a'".into()),
                ]),
            ),
            p(
                "b",
                Choice(vec![
                    Sequence(vec![NonTerminal("a".into()), TerminalLiteral("'y'".into())]),
                    TerminalLiteral("'b'".into()),
                ]),
            ),
        ]);
        let diagnostics = g.check();
        assert!(diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && d.message.contains("'a'")
                && d.message.contains("mutually left-recursive")
        }));
        assert!(diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && d.message.contains("'b'")
                && d.message.contains("mutually left-recursive")
        }));
    }

    #[test]
    fn unreachable_rules_warns_on_unreferenced_rule() {
        let g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("orphan", TerminalLiteral("'y'".into())),
        ]);
        assert_eq!(
            strs(&g.unreachable_rules_check()),
            vec!["warning: rule 'orphan' is never referenced (line 1)"]
        );
    }

    #[test]
    fn unreachable_rules_no_warning_for_root() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    fn unreachable_rules_no_warning_when_referenced_in_body() {
        let mut g = Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("helper".into())),
            p("helper", TerminalLiteral("'x'".into())),
        ]);
        g.rhs_nonterminals.insert("helper".into());
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    fn unreachable_rules_no_warning_for_extras_rule() {
        let mut g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("ws", TerminalLiteral("' '".into())),
        ]);
        g.extras = vec![di("ws", 1)];
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    fn check_no_warning_for_right_recursive_rule() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar::from_rules([p(
            "list",
            Choice(vec![
                Sequence(vec![
                    TerminalLiteral("'x'".into()),
                    NonTerminal("list".into()),
                ]),
                TerminalLiteral("'x'".into()),
            ]),
        )]);
        let diagnostics = g.check();
        assert!(!diagnostics
            .iter()
            .any(|d| d.message.contains("left-recursive")));
    }
}
