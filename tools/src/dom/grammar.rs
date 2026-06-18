use std::collections::HashSet;

use crate::dom::NameOrLiteral;

use super::analysis::{
    count_leaf_rules, count_left_recursive, count_unique_terminals, first_set_stats,
};
use super::diagnostic::Diagnostic;
use super::directive::{ConflictGroup, DirectiveItem, PrecedenceGroup, ReservedEntry, loc};
use super::summary::GrammarSummary;
use super::types::Grammar;

impl Grammar {
    /// Checks `%reserved` directives and rule-level annotations for undefined references.
    ///
    /// Warns for each `ReservedEntry` rule name not in `known`, and for each rule-level
    /// `%reserved` annotation whose set name has no matching `ReservedEntry`.
    fn reserved_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        let mut known_reserved_sets: HashSet<&str> = HashSet::new();
        let mut not_referenced: Vec<Diagnostic> = self
            .reserved_sets
            .iter()
            .flat_map(
                |ReservedEntry {
                     set_name,
                     rule_names,
                     line,
                     filename,
                 }| {
                    known_reserved_sets.insert(set_name.as_str());
                    let location = loc(filename, *line);
                    rule_names.iter().filter_map(move |item| {
                        if let NameOrLiteral::Name(name) = item
                            && !known.contains(name.as_str())
                        {
                            Some(Diagnostic::warning(format!(
                                "%reserved references undefined rule '{name}' ({location})"
                            )))
                        } else {
                            None
                        }
                    })
                },
            )
            .collect();

        not_referenced.extend(self.reserved_set_refs.iter().filter_map(
            |DirectiveItem {
                 name,
                 line,
                 filename,
             }| {
                if !known_reserved_sets.contains(name.as_str()) {
                    let location = loc(filename, *line);
                    Some(Diagnostic::warning(format!(
                        "%reserved annotation references undeclared set '{name}' ({location})"
                    )))
                } else {
                    None
                }
            },
        ));

        not_referenced
    }

    /// Returns a warning for every rule name in `%conflicts` that has no definition.
    fn conflicts_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.conflicts
            .iter()
            .flat_map(
                |ConflictGroup {
                     rules,
                     line,
                     filename,
                 }| {
                    let location = loc(filename, *line);
                    rules.iter().filter_map(move |name| {
                        if known.contains(name.as_str()) {
                            return None;
                        }
                        Some(Diagnostic::warning(format!(
                            "%conflicts references undefined rule '{name}' ({location})"
                        )))
                    })
                },
            )
            .collect()
    }

    /// Checks each `%precedences` group and warns for any `Name` item not in `known`.
    fn precedences_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.precedences
            .iter()
            .flat_map(
                |PrecedenceGroup {
                     items,
                     line,
                     filename,
                 }| {
                    let location = loc(filename, *line);
                    items.iter().filter_map(move |item| {
                        if let NameOrLiteral::Name(name) = item
                            && !known.contains(name.as_str())
                        {
                            Some(Diagnostic::warning(format!(
                                "%precedences references undefined rule '{name}' ({location})"
                            )))
                        } else {
                            None
                        }
                    })
                },
            )
            .collect()
    }

    /// Returns a warning for every rule name in `%inline` that has no definition.
    fn inline_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.inline
            .iter()
            .filter(|item| !known.contains(item.name.as_str()))
            .map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    Diagnostic::warning(format!(
                        "%inline references undefined rule '{name}' ({})",
                        loc(filename, *line)
                    ))
                },
            )
            .collect()
    }

    /// Returns a warning for every rule name in `%supertypes` that has no definition.
    fn supertypes_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.supertypes
            .iter()
            .filter(|item| !known.contains(item.name.as_str()))
            .map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    Diagnostic::warning(format!(
                        "%supertypes references undefined rule '{name}' ({})",
                        loc(filename, *line)
                    ))
                },
            )
            .collect()
    }

    /// Returns a warning for every rule reference in `%extras` that has no definition.
    fn extras_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.extras
            .iter()
            .filter(|item| !item.name.starts_with('/') && !known.contains(item.name.as_str()))
            .map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    Diagnostic::warning(format!(
                        "%extras references undefined rule '{name}' ({})",
                        loc(filename, *line)
                    ))
                },
            )
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

    /// Returns a warning for every rule that is never referenced by any other rule or directive.
    ///
    /// When `%axiom` is set, only that rule is exempt as the entry point.
    /// Otherwise the first-declared rule is exempt as the implicit root.
    /// Rules mentioned in `%extras` are also exempt: they are legitimately used
    /// without appearing in any rule body (e.g. whitespace handlers).
    fn unreachable_rules_check(&self) -> Vec<Diagnostic> {
        let mut referenced: std::collections::HashSet<&str> =
            self.rhs_nonterminals.iter().map(String::as_str).collect();
        for item in self.extras.iter().filter(|i| !i.name.starts_with('/')) {
            referenced.insert(&item.name);
        }

        let Some(root) = self.root_rule() else {
            return vec![];
        };

        self.productions
            .keys()
            .filter(|name| name.as_str() != root && !referenced.contains(name.as_str()))
            .map(|name| {
                let (line, filename) = self
                    .productions
                    .get(name.as_str())
                    .map(|p| (p.line, p.filename.as_str()))
                    .unwrap_or((0, ""));
                Diagnostic::warning(format!(
                    "rule '{name}' is never referenced ({})",
                    loc(filename, line)
                ))
            })
            .collect()
    }

    /// Returns the number of non-terminal references in rule bodies that have no definition.
    ///
    /// Exposed for use by the summary builder; the full diagnostic list is produced by [`check`](Self::check).
    pub(crate) fn count_undefined_refs(&self) -> usize {
        self.undefined_refs_check(&self.known_rules()).len()
    }

    /// Returns the number of rules that are never referenced from the root.
    ///
    /// Exposed for use by the summary builder; the full diagnostic list is produced by [`check`](Self::check).
    pub(crate) fn count_unreachable_rules(&self) -> usize {
        self.unreachable_rules_check().len()
    }

    /// Merges `other` into `self`, treating its contents as if inlined at the `%include` site.
    ///
    /// **Duplicate rules**: the last definition wins (the incoming rule from `other` replaces the
    /// existing one), but a warning is emitted so the author is aware of the shadowing.
    ///
    /// **`%axiom`**: first declaration wins — if `self` already has one, the incoming axiom is
    /// rejected with an error diagnostic rather than silently overriding the root.
    pub(crate) fn merge_from(&mut self, mut other: Grammar) {
        let other_axiom = other.take_axiom();
        let other_word = other.take_word();
        for (name, prod) in other.productions {
            if self.productions.contains_key(&name) {
                self.parse_diagnostics.push(Diagnostic::warning(format!(
                    "rule '{}' is defined more than once ({})",
                    name,
                    loc(&prod.filename, prod.line)
                )));
            }
            self.productions.insert(name, prod);
        }
        if let Some(axiom) = other_axiom
            && let Some(diag) = self.declare_axiom(axiom)
        {
            self.parse_diagnostics.push(diag);
        }
        if let Some(word) = other_word
            && let Some(diag) = self.declare_word(word)
        {
            self.parse_diagnostics.push(diag);
        }
        self.conflicts.extend(other.conflicts);
        self.precedences.extend(other.precedences);
        self.inline.extend(other.inline);
        self.supertypes.extend(other.supertypes);
        self.extras.extend(other.extras);
        self.externals.extend(other.externals);
        self.rhs_nonterminals.extend(other.rhs_nonterminals);
        self.parse_diagnostics.extend(other.parse_diagnostics);
        self.reserved_set_refs.extend(other.reserved_set_refs);
        self.reserved_sets.extend(other.reserved_sets);
    }

    /// Builds a [`GrammarSummary`] by running all summary analyses over this grammar.
    ///
    /// This includes FIRST-set computation, which is not free; only call it
    /// when the caller has explicitly requested a summary (e.g. `check --summary`).
    pub fn summarise(&self) -> GrammarSummary {
        let (unique_literals, unique_patterns) = count_unique_terminals(self);
        let (left_recursive_direct, left_recursive_mutual) = count_left_recursive(self);
        GrammarSummary {
            rules: self.productions.len(),
            leaf_rules: count_leaf_rules(self),
            unreachable_rules: self.count_unreachable_rules(),
            unique_literals,
            unique_patterns,
            undefined_refs: self.count_undefined_refs(),
            left_recursive_direct,
            left_recursive_mutual,
            first_sets: first_set_stats(self),
        }
    }

    /// Runs all cross-reference checks and returns any diagnostics.
    ///
    /// Diagnostics are sorted by message so output is stable across runs regardless
    /// of `HashSet` iteration order in the individual checks.
    pub fn check(&self) -> Vec<Diagnostic> {
        let mut known = self.known_rules();
        known.extend(self.externals.iter().filter_map(|e| match e {
            NameOrLiteral::Name(n) => Some(n.as_str()),
            NameOrLiteral::Literal(_) => None,
        }));

        let mut diagnostics = Vec::new();
        diagnostics.extend(self.parse_diagnostics.iter().cloned());
        diagnostics.extend(check_directive_ref(
            self.axiom_directive(),
            "%axiom",
            &known,
        ));
        diagnostics.extend(check_directive_ref(self.word.as_ref(), "%word", &known));
        diagnostics.extend(self.conflicts_check(&known));
        diagnostics.extend(self.inline_check(&known));
        diagnostics.extend(self.supertypes_check(&known));
        diagnostics.extend(self.extras_check(&known));
        diagnostics.extend(self.precedences_check(&known));
        diagnostics.extend(self.undefined_refs_check(&known));
        diagnostics.extend(self.reserved_check(&known));
        diagnostics.extend(self.unreachable_rules_check());
        diagnostics.sort_by(|a, b| a.message.cmp(&b.message));
        diagnostics
    }
}

/// Returns an error if a directive refers an undefined rule.
fn check_directive_ref(
    directive: Option<&DirectiveItem>,
    name: &str,
    known: &HashSet<&str>,
) -> Vec<Diagnostic> {
    match directive {
        Some(DirectiveItem {
            name: rule,
            line,
            filename,
        }) if !known.contains(rule.as_str()) => {
            vec![Diagnostic::error(format!(
                "{name} references undefined rule '{rule}' ({})",
                loc(filename, *line)
            ))]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::TerminalLiteral;
    use crate::dom::test_utils::{cg, di, p};
    use crate::dom::{GrammarNode, Severity};

    /// Renders each diagnostic to its full display string for easy comparison.
    fn strs(diagnostics: &[Diagnostic]) -> Vec<String> {
        diagnostics.iter().map(|d| d.to_string()).collect()
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

    // ── precedences_check ────────────────────────────────────────────────────

    #[test]
    fn precedences_check_warns_on_undefined_name() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.precedences = vec![pg(&[NameOrLiteral::Name("ghost".into())], 0)];
        assert_eq!(
            strs(&g.precedences_check(&g.known_rules())),
            vec!["warning: %precedences references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn precedences_check_literal_never_warns() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.precedences = vec![pg(&[NameOrLiteral::Literal("'call'".into())], 0)];
        assert!(g.precedences_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn precedences_check_no_warnings_when_all_defined() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", TerminalLiteral("'y'".into())),
        ]);
        g.precedences = vec![pg(
            &[
                NameOrLiteral::Name("a".into()),
                NameOrLiteral::Name("b".into()),
            ],
            0,
        )];
        assert!(g.precedences_check(&g.known_rules()).is_empty());
    }

    // ── reserved_check ───────────────────────────────────────────────────────

    #[test]
    fn reserved_check_warns_on_undefined_rule_name() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Name("ghost".into())], 0)];
        assert_eq!(
            strs(&g.reserved_check(&g.known_rules())),
            vec!["warning: %reserved references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    fn reserved_check_literal_never_warns() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Literal("'if'".into())], 0)];
        assert!(g.reserved_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn reserved_check_warns_on_undeclared_set_reference() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_set_refs = vec![di("ghost_set", 0)];
        assert_eq!(
            strs(&g.reserved_check(&g.known_rules())),
            vec!["warning: %reserved annotation references undeclared set 'ghost_set' (line 0)"]
        );
    }

    #[test]
    /// `reserved_set_refs` is a `Vec`, not a `HashSet`: two occurrences of the same
    /// undeclared set name produce two separate warnings, not one deduplicated warning.
    fn reserved_check_two_undeclared_refs_produce_two_warnings() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_set_refs = vec![di("ghost", 0), di("ghost", 1)];
        assert_eq!(g.reserved_check(&g.known_rules()).len(), 2);
    }

    #[test]
    fn reserved_check_no_warnings_when_all_correct() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Name("a".into())], 0)];
        g.reserved_set_refs = vec![di("kw", 0)];
        assert!(g.reserved_check(&g.known_rules()).is_empty());
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

    // ── externals in known set ───────────────────────────────────────────────

    #[test]
    /// A `Name` item declared in `%externals` is treated as known: referencing it in a
    /// rule body must not trigger an undefined-rule-reference warning from `check()`.
    fn check_externals_name_not_flagged_as_undefined() {
        let mut g = Grammar::new();
        g.externals = vec![NameOrLiteral::Name("token".into())];
        g.rhs_nonterminals.insert("token".into());
        assert!(g.check().is_empty());
    }

    #[test]
    /// Without a matching `%externals` declaration, the same reference is still flagged.
    fn check_undefined_ref_without_externals_declaration_still_warns() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("token".into());
        assert_eq!(
            strs(&g.check()),
            vec!["warning: undefined rule reference 'token'"]
        );
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
    /// Direct left recursion is a valid tree-sitter idiom; `check` must not flag it (#197).
    fn check_accepts_direct_left_recursion() {
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
        assert!(g.check().is_empty());
    }

    #[test]
    /// Mutual left recursion is a valid tree-sitter idiom; `check` must not flag it (#197).
    fn check_accepts_mutual_left_recursion() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let mut g = Grammar::from_rules([
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
        g.rhs_nonterminals.insert("a".into());
        g.rhs_nonterminals.insert("b".into());
        assert!(g.check().is_empty());
    }

    #[test]
    fn unreachable_rules_warns_on_unreferenced_rule() {
        let g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("orphan", TerminalLiteral("'y'".into())),
        ]);
        assert_eq!(
            strs(&g.unreachable_rules_check()),
            vec!["warning: rule 'orphan' is never referenced (test.bnf:1)"]
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
    fn axiom_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.declare_axiom(di("ghost", 3));
        assert_eq!(
            strs(&check_directive_ref(
                g.axiom_directive(),
                "%axiom",
                &g.known_rules()
            )),
            vec!["error: %axiom references undefined rule 'ghost' (line 3)"]
        );
    }

    #[test]
    fn axiom_check_no_error_when_rule_defined() {
        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.declare_axiom(di("root", 1));
        assert!(check_directive_ref(g.axiom_directive(), "%axiom", &g.known_rules()).is_empty());
    }

    #[test]
    fn axiom_check_no_error_when_absent() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(check_directive_ref(g.axiom_directive(), "%axiom", &g.known_rules()).is_empty());
    }

    #[test]
    fn unreachable_rules_axiom_replaces_implicit_root() {
        // `first` is the first-declared rule but not the axiom; it should be flagged.
        let mut g = Grammar::from_rules([
            p("first", TerminalLiteral("'a'".into())),
            p("real_root", TerminalLiteral("'b'".into())),
        ]);
        g.declare_axiom(di("real_root", 1));
        let diags = strs(&g.unreachable_rules_check());
        assert!(diags.iter().any(|s| s.contains("'first'")));
        assert!(!diags.iter().any(|s| s.contains("'real_root'")));
    }

    #[test]
    fn unreachable_rules_no_warning_for_axiom_rule() {
        let mut g = Grammar::from_rules([p("entry", TerminalLiteral("'x'".into()))]);
        g.declare_axiom(di("entry", 1));
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    fn duplicate_axiom_is_an_error() {
        let src = "%axiom foo\n%axiom bar\nfoo -> 'x' ;\nbar -> 'y' ;\n";
        let (_, diags) = crate::visitors::parse_source(src).unwrap();
        assert!(diags.iter().any(|d| {
            d.severity == Severity::Error && d.message.contains("%axiom declared more than once")
        }));
    }

    // ── count_undefined_refs ──────────────────────────────────────────────────

    #[test]
    /// Zero when all referenced rules are defined.
    fn count_undefined_refs_zero_when_all_defined() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.rhs_nonterminals.insert("a".into());
        assert_eq!(g.count_undefined_refs(), 0);
    }

    #[test]
    /// Counts each distinct undefined reference once, regardless of how many rules use it.
    fn count_undefined_refs_counts_distinct_names() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("ghost".into());
        g.rhs_nonterminals.insert("phantom".into());
        assert_eq!(g.count_undefined_refs(), 2);
    }

    // ── count_unreachable_rules ───────────────────────────────────────────────

    #[test]
    /// Zero when every rule is reachable from the root.
    fn count_unreachable_rules_zero_when_all_reachable() {
        let mut g = Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("helper".into())),
            p("helper", TerminalLiteral("'x'".into())),
        ]);
        g.rhs_nonterminals.insert("helper".into());
        assert_eq!(g.count_unreachable_rules(), 0);
    }

    #[test]
    /// Counts rules that are never referenced and not the root.
    fn count_unreachable_rules_counts_orphans() {
        let g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("orphan", TerminalLiteral("'y'".into())),
        ]);
        assert_eq!(g.count_unreachable_rules(), 1);
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
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("left-recursive"))
        );
    }

    // ── Grammar::summarise ────────────────────────────────────────────────────

    #[test]
    /// An empty grammar produces a zeroed summary with no FIRST sets.
    fn summarise_empty_grammar() {
        let s = Grammar::new().summarise();
        assert_eq!(s.rules, 0);
        assert_eq!(s.leaf_rules, 0);
        assert_eq!(s.unreachable_rules, 0);
        assert_eq!(s.unique_literals, 0);
        assert_eq!(s.unique_patterns, 0);
        assert_eq!(s.undefined_refs, 0);
        assert_eq!(s.left_recursive_direct, 0);
        assert_eq!(s.left_recursive_mutual, 0);
        assert!(s.first_sets.is_none());
    }

    #[test]
    /// Rule count, leaf count, and terminal counts are correct for a simple grammar.
    fn summarise_counts_rules_and_terminals() {
        let g = Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("tok".into())),
            p("tok", TerminalLiteral("'x'".into())),
        ]);
        let s = g.summarise();
        assert_eq!(s.rules, 2);
        assert_eq!(s.leaf_rules, 1); // "tok" has no non-terminals
        assert_eq!(s.unique_literals, 1);
        assert_eq!(s.unique_patterns, 0);
    }

    #[test]
    /// Unreachable rules are counted correctly.
    fn summarise_unreachable_rules() {
        let g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("orphan", TerminalLiteral("'y'".into())),
        ]);
        let s = g.summarise();
        assert_eq!(s.unreachable_rules, 1);
    }

    #[test]
    /// Undefined rule references are counted correctly.
    fn summarise_undefined_refs() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("ghost".into());
        let s = g.summarise();
        assert_eq!(s.undefined_refs, 1);
    }

    #[test]
    /// Direct left-recursion is counted in the direct bucket, not mutual.
    fn summarise_left_recursive_direct() {
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
        let s = g.summarise();
        assert_eq!(s.left_recursive_direct, 1);
        assert_eq!(s.left_recursive_mutual, 0);
    }

    // ── word_check ────────────────────────────────────────────────────────────

    #[test]
    fn word_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.declare_word(di("ghost", 3));
        assert_eq!(
            strs(&check_directive_ref(
                g.word.as_ref(),
                "%word",
                &g.known_rules()
            )),
            vec!["error: %word references undefined rule 'ghost' (line 3)"]
        );
    }

    #[test]
    fn word_check_no_error_when_rule_defined() {
        let mut g = Grammar::from_rules([p("ident", TerminalLiteral("'x'".into()))]);
        g.declare_word(di("ident", 1));
        assert!(check_directive_ref(g.word.as_ref(), "%word", &g.known_rules()).is_empty());
    }

    #[test]
    fn word_check_no_error_when_absent() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(check_directive_ref(g.word.as_ref(), "%word", &g.known_rules()).is_empty());
    }

    #[test]
    /// FIRST-set stats are present and non-trivial for a non-empty grammar.
    fn summarise_first_sets_present_for_nonempty_grammar() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p(
                "b",
                GrammarNode::Choice(vec![
                    TerminalLiteral("'y'".into()),
                    TerminalLiteral("'z'".into()),
                ]),
            ),
        ]);
        let s = g.summarise();
        let fs = s
            .first_sets
            .expect("first_sets must be Some for non-empty grammar");
        assert_eq!(fs.min, 1);
        assert_eq!(fs.max, 2);
    }
}
