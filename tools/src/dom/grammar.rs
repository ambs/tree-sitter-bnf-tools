use std::collections::{HashSet, VecDeque};

use crate::dom::NameOrLiteral;

use super::analysis::{
    count_leaf_rules, count_left_recursive, count_unique_terminals, first_set_stats,
};
use super::diagnostic::Diagnostic;
use super::directive::{ConflictGroup, DirectiveItem, PrecedenceGroup, ReservedEntry, loc};
use super::summary::GrammarSummary;
use super::types::Grammar;

impl Grammar {
    /// Returns an error when the resolved start rule (via `%axiom`, or the implicit
    /// first-declared rule) is hidden, either because its name starts with `_` or
    /// because it's listed in `%supertypes` (which unconditionally hides a rule).
    ///
    /// Upstream `tree-sitter generate` requires the start rule to be visible; the
    /// diagnostic's location is the `%axiom` directive's line when `%axiom` produced
    /// the hidden root, or the rule's own declaration line otherwise.
    fn hidden_start_rule_check(&self) -> Vec<Diagnostic> {
        let Some(root) = self.root_rule() else {
            return vec![];
        };

        let reason = if root.starts_with('_') {
            "rule names starting with '_' are not allowed as the grammar's start symbol"
        } else if self.supertypes.iter().any(|item| item.name == root) {
            "rules listed in %supertypes are hidden and cannot be the grammar's start symbol"
        } else {
            return vec![];
        };

        let (line, filename) = match self.axiom_directive() {
            Some(DirectiveItem {
                name,
                line,
                filename,
            }) if name == root => (*line, filename.as_str()),
            _ => self
                .productions
                .get(root)
                .map(|p| (p.line, p.filename.as_str()))
                .unwrap_or((0, "")),
        };

        vec![Diagnostic::error(format!(
            "start rule '{root}' cannot be hidden ({reason}) ({})",
            loc(filename, line)
        ))]
    }

    /// Checks `%reserved` directives and rule-level annotations for undefined references.
    ///
    /// Errors for each `ReservedEntry` rule name not in `known`, and for each rule-level
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
                            Some(Diagnostic::error(format!(
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
                    Some(Diagnostic::error(format!(
                        "%reserved annotation references undeclared set '{name}' ({location})"
                    )))
                } else {
                    None
                }
            },
        ));

        not_referenced
    }

    /// Returns an error for every `%externals` name that is also defined as a rule.
    fn externals_check(&self) -> Vec<Diagnostic> {
        self.externals
            .iter()
            .filter_map(|item| {
                let NameOrLiteral::Name(name) = item else {
                    return None;
                };
                let production = self.productions.get(name)?;
                Some(Diagnostic::error(format!(
                    "%externals declares '{name}', but it is also defined as a rule ({})",
                    loc(&production.filename, production.line)
                )))
            })
            .collect()
    }

    /// Checks rule-level `%prec 'name'` annotations against declared `%precedences` names.
    ///
    /// Errors for each `prec_name_refs` entry whose normalized name has no matching
    /// `Literal` item across all `%precedences` groups.
    fn prec_name_check(&self) -> Vec<Diagnostic> {
        let named_precedences: HashSet<&str> = self
            .precedences
            .iter()
            .flat_map(
                |PrecedenceGroup {
                     items,
                     line: _,
                     filename: _,
                 }| {
                    items.iter().filter_map(|item| match item {
                        NameOrLiteral::Name(_) => None,
                        NameOrLiteral::Literal(literal) => Some(literal.as_str()),
                    })
                },
            )
            .collect();

        self.prec_name_refs
            .iter()
            .filter_map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    if !named_precedences.contains(name.as_str()) {
                        Some(Diagnostic::error(format!(
                            "%precedence references undefined precedence literal {} ({})",
                            name,
                            loc(filename, *line)
                        )))
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Returns an error for every rule name in `%conflicts` that has no definition.
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
                        Some(Diagnostic::error(format!(
                            "%conflicts references undefined rule '{name}' ({location})"
                        )))
                    })
                },
            )
            .collect()
    }

    /// Checks each `%precedences` group and errors for any `Name` item not in `known`.
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
                            Some(Diagnostic::error(format!(
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

    /// Returns an error for every rule name in `%inline` that has no definition,
    /// plus, for each name that resolves to a defined rule, an error for each of
    /// upstream `tree-sitter generate`'s further `process_inlines` constraints it
    /// violates: being the resolved start rule (`ProcessInlinesError::FirstRule`),
    /// also being declared via `%externals` (`ProcessInlinesError::ExternalToken`),
    /// or having a body that reduces to a pure token (`ProcessInlinesError::Token`).
    fn inline_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        let root = self.root_rule();
        self.inline
            .iter()
            .flat_map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    let location = loc(filename, *line);
                    if !known.contains(name.as_str()) {
                        return vec![Diagnostic::error(format!(
                            "%inline references undefined rule '{name}' ({location})"
                        ))];
                    }
                    let mut diagnostics = Vec::new();
                    if Some(name.as_str()) == root {
                        diagnostics.push(Diagnostic::error(format!(
                            "%inline rule '{name}' cannot be the grammar's start rule ({location})"
                        )));
                    }
                    if self
                        .externals
                        .iter()
                        .any(|e| matches!(e, NameOrLiteral::Name(n) if n == name))
                    {
                        diagnostics.push(Diagnostic::error(format!(
                            "%inline rule '{name}' cannot also be declared via %externals ({location})"
                        )));
                    }
                    if let Some(production) = self.productions.get(name.as_str())
                        && production.body.is_pure_token()
                    {
                        diagnostics.push(Diagnostic::error(format!(
                            "%inline rule '{name}' must not be a pure token ({})",
                            loc(&production.filename, production.line)
                        )));
                    }
                    diagnostics
                },
            )
            .collect()
    }

    /// Returns an error when `%word`'s target rule has no definition, plus,
    /// when it resolves to a defined rule, an error for each of upstream
    /// `tree-sitter generate`'s `NonTerminalWordTokenError` constraints it
    /// violates: a body that isn't a pure token, or a body identical to
    /// another rule's (naming the conflicting rule, like upstream's
    /// `conflicting_symbol_name`).
    fn word_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        let Some(DirectiveItem {
            name,
            line,
            filename,
        }) = self.word.as_ref()
        else {
            return vec![];
        };
        if !known.contains(name.as_str()) {
            return vec![Diagnostic::error(format!(
                "%word references undefined rule '{name}' ({})",
                loc(filename, *line)
            ))];
        }
        let Some(production) = self.productions.get(name) else {
            return vec![];
        };
        let mut diagnostics = Vec::new();
        if !production.body.is_pure_token() {
            diagnostics.push(Diagnostic::error(format!(
                "%word rule '{name}' must be a pure token ({})",
                loc(&production.filename, production.line)
            )));
        }
        let body = production.body.to_string();
        if let Some((conflicting_name, _)) = self.productions.iter().find(|(other_name, other)| {
            other_name.as_str() != name && other.body.to_string() == body
        }) {
            diagnostics.push(Diagnostic::error(format!(
                "%word rule '{name}' has the same body as rule '{conflicting_name}' ({})",
                loc(&production.filename, production.line)
            )));
        }
        diagnostics
    }

    /// Returns an error for every rule name in `%supertypes` that has no definition
    /// (a name only declared via `%externals` does not count: a supertype must be an
    /// actual rule with a body), plus, for each name that resolves to a defined rule,
    /// an error for each of upstream `tree-sitter generate`'s two further constraints
    /// it violates: a body that reduces to a pure token (`SupertypeTerminal`), or an
    /// alternative spanning more than one step (`InvalidSupertype`).
    fn supertypes_check(&self) -> Vec<Diagnostic> {
        self.supertypes
            .iter()
            .flat_map(
                |DirectiveItem {
                     name,
                     line,
                     filename,
                 }| {
                    let Some(production) = self.productions.get(name) else {
                        return vec![Diagnostic::error(format!(
                            "%supertypes references undefined rule '{name}' ({})",
                            loc(filename, *line)
                        ))];
                    };
                    let mut diagnostics = Vec::new();
                    if production.body.is_pure_token() {
                        diagnostics.push(Diagnostic::error(format!(
                            "%supertypes rule '{name}' must not be a pure token ({})",
                            loc(&production.filename, production.line)
                        )));
                    }
                    if !production.body.single_choice_options() {
                        diagnostics.push(Diagnostic::error(format!(
                            "%supertypes rule '{name}' has an alternative with more than one step ({})",
                            loc(&production.filename, production.line)
                        )));
                    }
                    diagnostics
                },
            )
            .collect()
    }

    /// Returns an error for every rule reference in `%extras` that has no definition.
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
                    Diagnostic::error(format!(
                        "%extras references undefined rule '{name}' ({})",
                        loc(filename, *line)
                    ))
                },
            )
            .collect()
    }

    /// Returns an error for every non-terminal referenced in a rule body that has no definition.
    fn undefined_refs_check(&self, known: &HashSet<&str>) -> Vec<Diagnostic> {
        self.rhs_nonterminals
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| Diagnostic::error(format!("undefined rule reference '{name}'")))
            .collect()
    }

    /// Returns a warning for every rule that is unreachable from the root.
    ///
    /// When `%axiom` is set, that rule is the root. Otherwise the first-declared
    /// rule is the root. Rules mentioned in `%extras` are additional roots: they
    /// are legitimately used without appearing in any rule body (e.g. whitespace
    /// handlers), and anything reachable from their own bodies is exempt too.
    ///
    /// Reachability is computed by BFS over each production's own body, not by
    /// flat "is this name mentioned anywhere" membership — a rule referencing
    /// only itself, or a cycle disconnected from the root, is still unreachable
    /// and must still warn (#304).
    fn unreachable_rules_check(&self) -> Vec<Diagnostic> {
        let Some(root) = self.root_rule() else {
            return vec![];
        };

        let mut reachable: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<&str> = VecDeque::new();

        reachable.insert(root);
        queue.push_back(root);
        for item in self.extras.iter().filter(|i| !i.name.starts_with('/')) {
            if reachable.insert(&item.name) {
                queue.push_back(&item.name);
            }
        }

        while let Some(current) = queue.pop_front() {
            if let Some(production) = self.productions.get(current) {
                for name in production.body.nonterminal_names() {
                    if reachable.insert(name) {
                        queue.push_back(name);
                    }
                }
            }
        }

        self.productions
            .keys()
            .filter(|name| !reachable.contains(name.as_str()))
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
    /// existing one), but a warning is emitted so the author is aware of the shadowing. The
    /// caller (`visit_include_directive`, issue #301) never calls this twice for the same
    /// canonical file within one parse, so this warning only fires for two genuinely distinct
    /// files that happen to declare the same rule name — not for a diamond include (the same
    /// file reached via more than one `%include` path), which is skipped before reaching here.
    ///
    /// **`%axiom`**: scoped to the top-level file (issue #295) — an included file's `%axiom` is
    /// discarded unconditionally. It never overrides `self`'s axiom, is never adopted when `self`
    /// has none, and never triggers a duplicate-axiom diagnostic; the included file's own rules
    /// are also never candidates for `self`'s implicit-first-rule fallback (see
    /// [`Grammar::root_rule`]). An included file used standalone still resolves its own `%axiom`
    /// exactly as documented.
    pub(crate) fn merge_from(&mut self, mut other: Grammar) {
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
        diagnostics.extend(self.word_check(&known));
        diagnostics.extend(self.conflicts_check(&known));
        diagnostics.extend(self.inline_check(&known));
        diagnostics.extend(self.supertypes_check());
        diagnostics.extend(self.extras_check(&known));
        diagnostics.extend(self.precedences_check(&known));
        diagnostics.extend(self.undefined_refs_check(&known));
        diagnostics.extend(self.reserved_check(&known));
        diagnostics.extend(self.unreachable_rules_check());
        diagnostics.extend(self.prec_name_check());
        diagnostics.extend(self.externals_check());
        diagnostics.extend(self.hidden_start_rule_check());
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
    use crate::dom::GrammarNode::{NonTerminal, TerminalLiteral};
    use crate::dom::test_utils::{cg, di, p};
    use crate::dom::{GrammarNode, Severity};

    /// Renders each diagnostic to its full display string for easy comparison.
    fn strs(diagnostics: &[Diagnostic]) -> Vec<String> {
        diagnostics.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    /// Errors when a `%conflicts` group names a rule that has no definition.
    fn conflicts_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.conflicts = vec![cg(&["a", "ghost"], 0)];
        assert_eq!(
            strs(&g.conflicts_check(&g.known_rules())),
            vec!["error: %conflicts references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// No errors when every rule named in `%conflicts` is defined.
    fn conflicts_check_no_errors_when_all_rules_defined() {
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", TerminalLiteral("'y'".into())),
        ]);
        g.conflicts = vec![cg(&["a", "b"], 0)];
        assert!(g.conflicts_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Errors when `%supertypes` names a rule that has no definition.
    fn supertypes_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.supertypes = vec![di("ghost", 0)];
        assert_eq!(
            strs(&g.supertypes_check()),
            vec!["error: %supertypes references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// No error when the rule named in `%supertypes` is defined with a
    /// non-token, single-step body (a reference to another rule).
    fn supertypes_check_no_errors_when_all_rules_defined() {
        let mut g = Grammar::from_rules([
            p("expression", NonTerminal("statement".into())),
            p("statement", TerminalLiteral("'x'".into())),
        ]);
        g.supertypes = vec![di("expression", 0)];
        assert!(g.supertypes_check().is_empty());
    }

    #[test]
    /// A name declared only via `%externals` (no rule body) is undefined for
    /// `%supertypes` purposes — a supertype must be an actual rule.
    fn supertypes_check_errors_on_external_only_name() {
        use crate::dom::NameOrLiteral;

        let mut g = Grammar::from_rules([p("expression", TerminalLiteral("'x'".into()))]);
        g.externals = vec![NameOrLiteral::Name("foo".into())];
        g.supertypes = vec![di("foo", 0)];
        assert_eq!(
            strs(&g.supertypes_check()),
            vec!["error: %supertypes references undefined rule 'foo' (line 0)"]
        );
    }

    #[test]
    /// Errors when a `%supertypes` rule's body is a bare token (`SupertypeTerminal`);
    /// the location points at the rule's own definition, not at the directive's
    /// distinct, deliberately mismatched line/file.
    fn supertypes_check_errors_on_terminal_rule() {
        use crate::dom::Production;

        let mut g = Grammar::from_rules([Production {
            name: "ident".into(),
            body: TerminalLiteral("'x'".into()),
            line: 42,
            filename: "ident.bnf".into(),
        }]);
        g.supertypes = vec![di("ident", 99)];
        assert_eq!(
            strs(&g.supertypes_check()),
            vec!["error: %supertypes rule 'ident' must not be a pure token (ident.bnf:42)"]
        );
    }

    #[test]
    /// Errors when one alternative of a `%supertypes` rule has more than one
    /// step (`InvalidSupertype`); the location points at the offending rule's
    /// own definition, distinct from the other rule's and the directive's.
    fn supertypes_check_errors_on_multi_step_alternative() {
        use crate::dom::GrammarNode::{Choice, Sequence};
        use crate::dom::Production;

        let mut g = Grammar::from_rules([
            Production {
                name: "expr".into(),
                body: Choice(vec![
                    NonTerminal("term".into()),
                    Sequence(vec![
                        NonTerminal("term".into()),
                        TerminalLiteral("'+'".into()),
                        NonTerminal("term".into()),
                    ]),
                ]),
                line: 7,
                filename: "expr.bnf".into(),
            },
            Production {
                name: "term".into(),
                body: TerminalLiteral("'1'".into()),
                line: 13,
                filename: "term.bnf".into(),
            },
        ]);
        g.supertypes = vec![di("expr", 99)];
        assert_eq!(
            strs(&g.supertypes_check()),
            vec![
                "error: %supertypes rule 'expr' has an alternative with more than one step (expr.bnf:7)"
            ]
        );
    }

    #[test]
    /// No error when every alternative of a `%supertypes` rule is a single
    /// step, mirroring `expr -> term | unary | binary ;`.
    fn supertypes_check_no_errors_on_single_step_alternatives() {
        use crate::dom::GrammarNode::Choice;

        let mut g = Grammar::from_rules([
            p(
                "expr",
                Choice(vec![
                    NonTerminal("term".into()),
                    NonTerminal("unary".into()),
                    NonTerminal("binary".into()),
                ]),
            ),
            p("term", TerminalLiteral("'1'".into())),
            p("unary", TerminalLiteral("'-'".into())),
            p("binary", TerminalLiteral("'+'".into())),
        ]);
        g.supertypes = vec![di("expr", 0)];
        assert!(g.supertypes_check().is_empty());
    }

    #[test]
    /// Errors when `%inline` names a rule that has no definition.
    fn inline_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.inline = vec![di("ghost", 0)];
        assert_eq!(
            strs(&g.inline_check(&g.known_rules())),
            vec!["error: %inline references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// No error when the rule named in `%inline` is defined, is not the start
    /// rule, is not also declared via `%externals`, and is not a pure token.
    fn inline_check_no_errors_when_all_rules_defined() {
        let mut g = Grammar::from_rules([
            p("root", NonTerminal("a".into())),
            p("a", NonTerminal("b".into())),
            p("b", TerminalLiteral("'x'".into())),
        ]);
        g.inline = vec![di("a", 0)];
        assert!(g.inline_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Errors when `%inline` names the resolved start rule
    /// (`ProcessInlinesError::FirstRule` upstream).
    fn inline_check_errors_on_start_rule() {
        let mut g = Grammar::from_rules([
            p("root", NonTerminal("a".into())),
            p("a", TerminalLiteral("'x'".into())),
        ]);
        g.inline = vec![di("root", 0)];
        assert_eq!(
            strs(&g.inline_check(&g.known_rules())),
            vec!["error: %inline rule 'root' cannot be the grammar's start rule (line 0)"]
        );
    }

    #[test]
    /// Errors when `%inline` names a rule also declared via `%externals`
    /// (`ProcessInlinesError::ExternalToken` upstream).
    fn inline_check_errors_on_external_token() {
        use crate::dom::NameOrLiteral;

        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.externals = vec![NameOrLiteral::Name("foo".into())];
        g.inline = vec![di("foo", 0)];
        let mut known = g.known_rules();
        known.insert("foo");
        assert_eq!(
            strs(&g.inline_check(&known)),
            vec!["error: %inline rule 'foo' cannot also be declared via %externals (line 0)"]
        );
    }

    #[test]
    /// Errors when `%inline` names a rule whose body is a bare token
    /// (`ProcessInlinesError::Token` upstream); the location points at the
    /// rule's own definition, not at the directive's distinct, deliberately
    /// mismatched line/file.
    fn inline_check_errors_on_pure_token_rule() {
        use crate::dom::Production;

        let mut g = Grammar::from_rules([
            p("root", NonTerminal("ident".into())),
            Production {
                name: "ident".into(),
                body: TerminalLiteral("'x'".into()),
                line: 42,
                filename: "ident.bnf".into(),
            },
        ]);
        g.inline = vec![di("ident", 99)];
        assert_eq!(
            strs(&g.inline_check(&g.known_rules())),
            vec!["error: %inline rule 'ident' must not be a pure token (ident.bnf:42)"]
        );
    }

    // ── precedences_check ────────────────────────────────────────────────────

    #[test]
    /// Errors when a `%precedences` group's `Name` item has no matching rule.
    fn precedences_check_errors_on_undefined_name() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.precedences = vec![pg(&[NameOrLiteral::Name("ghost".into())], 0)];
        assert_eq!(
            strs(&g.precedences_check(&g.known_rules())),
            vec!["error: %precedences references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// A `%precedences` group's `Literal` item is never checked against rule names.
    fn precedences_check_literal_never_errors() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.precedences = vec![pg(&[NameOrLiteral::Literal("'call'".into())], 0)];
        assert!(g.precedences_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// No errors when every `Name` item across a `%precedences` group is defined.
    fn precedences_check_no_errors_when_all_defined() {
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
    /// Errors when a `%reserved` set's `Name` item has no matching rule.
    fn reserved_check_errors_on_undefined_rule_name() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Name("ghost".into())], 0)];
        assert_eq!(
            strs(&g.reserved_check(&g.known_rules())),
            vec!["error: %reserved references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// A `%reserved` set's `Literal` item is never checked against rule names.
    fn reserved_check_literal_never_errors() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Literal("'if'".into())], 0)];
        assert!(g.reserved_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Errors when a rule-level `%reserved` annotation names a set that was never declared.
    fn reserved_check_errors_on_undeclared_set_reference() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_set_refs = vec![di("ghost_set", 0)];
        assert_eq!(
            strs(&g.reserved_check(&g.known_rules())),
            vec!["error: %reserved annotation references undeclared set 'ghost_set' (line 0)"]
        );
    }

    #[test]
    /// `reserved_set_refs` is a `Vec`, not a `HashSet`: two occurrences of the same
    /// undeclared set name produce two separate errors, not one deduplicated error.
    fn reserved_check_two_undeclared_refs_produce_two_errors() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_set_refs = vec![di("ghost", 0), di("ghost", 1)];
        assert_eq!(g.reserved_check(&g.known_rules()).len(), 2);
    }

    #[test]
    /// No errors when the reserved set's rule is defined and the annotation matches a declared set.
    fn reserved_check_no_errors_when_all_correct() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Name("a".into())], 0)];
        g.reserved_set_refs = vec![di("kw", 0)];
        assert!(g.reserved_check(&g.known_rules()).is_empty());
    }

    // ── prec_name_check ──────────────────────────────────────────────────────

    #[test]
    /// Errors when a `%prec 'name'` annotation has no matching `%precedences` literal.
    fn prec_name_check_errors_on_undeclared_name() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.prec_name_refs = vec![di("'unary'", 0)];
        assert_eq!(
            strs(&g.prec_name_check()),
            vec!["error: %precedence references undefined precedence literal 'unary' (line 0)"]
        );
    }

    #[test]
    /// No error when the `%prec` name matches a declared `%precedences` literal.
    fn prec_name_check_no_errors_when_declared() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.precedences = vec![pg(&[NameOrLiteral::Literal("'unary'".into())], 0)];
        g.prec_name_refs = vec![di("'unary'", 0)];
        assert!(g.prec_name_check().is_empty());
    }

    #[test]
    /// An integer level never reaches `prec_name_refs` (only named levels do), so it must
    /// never be checked against `%precedences` names, even when one is declared.
    fn prec_name_check_integer_level_never_checked() {
        use crate::dom::test_utils::{nt, pg};
        use crate::dom::{NameOrLiteral, PrecKind, PrecLevel};
        let mut g = Grammar::from_rules([p(
            "a",
            GrammarNode::Prec(
                PrecKind::Plain,
                Some(PrecLevel::Integer(1)),
                Box::new(nt("a")),
            ),
        )]);
        g.precedences = vec![pg(&[NameOrLiteral::Literal("'unary'".into())], 0)];
        assert!(g.prec_name_check().is_empty());
    }

    #[test]
    /// `%precedences` literal items are normalized at parse time (`visit_precedences_directive`),
    /// the same way `%prec` annotation names are, so a quote-style mismatch in the source
    /// (`"unary"` declared, `'unary'` referenced) must not produce a false-positive error.
    fn prec_name_check_mixed_quote_style_normalizes() {
        let src = "%precedences [\"unary\"]\na -> 'x' %prec 'unary' ;\n";
        let (g, _) = crate::visitors::parse_source(src).unwrap();
        assert!(g.prec_name_check().is_empty());
    }

    #[test]
    /// Errors when `%extras` names a rule that has no definition.
    fn extras_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.extras = vec![di("/\\s/", 0), di("ghost", 0)];
        assert_eq!(
            strs(&g.extras_check(&g.known_rules())),
            vec!["error: %extras references undefined rule 'ghost' (line 0)"]
        );
    }

    #[test]
    /// A pattern item (e.g. `/\s/`) in `%extras` is never checked against rule names.
    fn extras_check_no_error_for_pattern() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.extras = vec![di("/\\s/", 0)];
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// No error when the `%extras` rule is defined, alongside an exempt pattern item.
    fn extras_check_no_errors_when_rule_defined() {
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("comment", TerminalLiteral("'#'".into())),
        ]);
        g.extras = vec![di("/\\s/", 0), di("comment", 0)];
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Errors for a rule-body reference that has no matching rule definition.
    fn undefined_refs_check_errors_on_missing_rule() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("term".into());
        assert_eq!(
            strs(&g.undefined_refs_check(&g.known_rules())),
            vec!["error: undefined rule reference 'term'"]
        );
    }

    #[test]
    /// No error when the referenced rule is defined.
    fn undefined_refs_check_no_error_when_defined() {
        let mut g = Grammar::from_rules([p("term", TerminalLiteral("'x'".into()))]);
        g.rhs_nonterminals.insert("term".into());
        assert!(g.undefined_refs_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Exactly one error is produced per distinct undefined name.
    fn undefined_refs_check_deduplicates() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("ghost".into());
        assert_eq!(g.undefined_refs_check(&g.known_rules()).len(), 1);
    }

    // ── externals_check ──────────────────────────────────────────────────────

    #[test]
    /// Errors when a `%externals` name is also defined as a rule.
    fn externals_check_errors_when_name_also_defined_as_rule() {
        let mut g = Grammar::from_rules([p("foo", TerminalLiteral("'x'".into()))]);
        g.externals = vec![NameOrLiteral::Name("foo".into())];
        assert_eq!(
            strs(&g.externals_check()),
            vec!["error: %externals declares 'foo', but it is also defined as a rule (test.bnf:1)"]
        );
    }

    #[test]
    /// No error when a `%externals` name has no matching rule (the legitimate, scanner-defined case).
    fn externals_check_no_error_for_undefined_external_name() {
        let mut g = Grammar::new();
        g.externals = vec![NameOrLiteral::Name("token".into())];
        assert!(g.externals_check().is_empty());
    }

    #[test]
    /// A `Literal` item in `%externals` never collides with a rule, even if a rule shares its text.
    fn externals_check_no_error_for_literal_item() {
        let mut g = Grammar::from_rules([p("'bar'", TerminalLiteral("'x'".into()))]);
        g.externals = vec![NameOrLiteral::Literal("'bar'".into())];
        assert!(g.externals_check().is_empty());
    }

    // ── externals in known set ───────────────────────────────────────────────

    #[test]
    /// A `Name` item declared in `%externals` is treated as known: referencing it in a
    /// rule body must not trigger an undefined-rule-reference error from `check()`.
    fn check_externals_name_not_flagged_as_undefined() {
        let mut g = Grammar::new();
        g.externals = vec![NameOrLiteral::Name("token".into())];
        g.rhs_nonterminals.insert("token".into());
        assert!(g.check().is_empty());
    }

    #[test]
    /// Without a matching `%externals` declaration, the same reference is still flagged.
    fn check_undefined_ref_without_externals_declaration_still_errors() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("token".into());
        assert_eq!(
            strs(&g.check()),
            vec!["error: undefined rule reference 'token'"]
        );
    }

    #[test]
    /// End-to-end: `check()` reports the collision when `%externals` and a same-named rule
    /// are both present, even though the name is also referenced in a rule body.
    fn check_errors_when_externals_name_collides_with_rule() {
        let mut g = Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("foo".into())),
            p("foo", TerminalLiteral("'x'".into())),
        ]);
        g.rhs_nonterminals.insert("foo".into());
        g.externals = vec![NameOrLiteral::Name("foo".into())];
        assert_eq!(
            strs(&g.check()),
            vec!["error: %externals declares 'foo', but it is also defined as a rule (test.bnf:1)"]
        );
    }

    #[test]
    /// `check()` returns diagnostics sorted alphabetically by message, regardless of insertion order.
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
    /// Warns when a rule is never referenced by another rule or directive.
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
    /// No warning for the implicit root (first-declared) rule.
    fn unreachable_rules_no_warning_for_root() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    /// No warning when a rule is referenced from another rule's body.
    fn unreachable_rules_no_warning_when_referenced_in_body() {
        let mut g = Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("helper".into())),
            p("helper", TerminalLiteral("'x'".into())),
        ]);
        g.rhs_nonterminals.insert("helper".into());
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    /// No warning for a rule that is only ever referenced via `%extras`.
    fn unreachable_rules_no_warning_for_extras_rule() {
        let mut g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("ws", TerminalLiteral("' '".into())),
        ]);
        g.extras = vec![di("ws", 1)];
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    /// A rule that only references itself is still unreachable from the root and must
    /// still warn (#304): a self-reference must not count as being "referenced". Uses
    /// `parse_source` (not `Grammar::from_rules`) because the bug only manifests through
    /// the real parser, which is what populated the old flat `rhs_nonterminals` cache.
    fn unreachable_rules_self_reference_still_warns() {
        let src = "A -> 'foo' ;\nB -> B ;\n";
        let (g, _) = crate::visitors::parse_source(src).unwrap();
        assert_eq!(
            strs(&g.unreachable_rules_check()),
            vec!["warning: rule 'B' is never referenced (line 2)"]
        );
    }

    #[test]
    /// A mutual cycle disconnected from the root is still unreachable and must still warn
    /// (#304): each rule in the cycle references the other, but neither is reachable from
    /// the root, so both must be flagged.
    fn unreachable_rules_disconnected_mutual_cycle_still_warns() {
        let src = "A -> 'foo' ;\nB -> C ;\nC -> B ;\n";
        let (g, _) = crate::visitors::parse_source(src).unwrap();
        assert_eq!(
            strs(&g.unreachable_rules_check()),
            vec![
                "warning: rule 'B' is never referenced (line 2)",
                "warning: rule 'C' is never referenced (line 3)",
            ]
        );
    }

    #[test]
    /// A rule reachable only through an `%extras` rule's own body must not warn: `%extras`
    /// rules are themselves additional entry points, not just exempt leaves.
    fn unreachable_rules_reachable_via_extras_body_not_warned() {
        use crate::dom::test_utils::nt;
        let mut g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("ws", nt("comment")),
            p("comment", TerminalLiteral("'#'".into())),
        ]);
        g.extras = vec![di("ws", 1)];
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    /// Errors when `%axiom` names a rule that has no definition.
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
    /// No error when the `%axiom` rule is defined.
    fn axiom_check_no_error_when_rule_defined() {
        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.declare_axiom(di("root", 1));
        assert!(check_directive_ref(g.axiom_directive(), "%axiom", &g.known_rules()).is_empty());
    }

    #[test]
    /// No error when no `%axiom` directive is present.
    fn axiom_check_no_error_when_absent() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(check_directive_ref(g.axiom_directive(), "%axiom", &g.known_rules()).is_empty());
    }

    #[test]
    /// When `%axiom` is declared, the first-declared rule loses its implicit-root exemption.
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
    /// No warning for the rule declared as `%axiom`.
    fn unreachable_rules_no_warning_for_axiom_rule() {
        let mut g = Grammar::from_rules([p("entry", TerminalLiteral("'x'".into()))]);
        g.declare_axiom(di("entry", 1));
        assert!(g.unreachable_rules_check().is_empty());
    }

    #[test]
    /// Declaring `%axiom` more than once in the same source is a parse-time error.
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
    /// Right recursion is not left recursion; `check` must not flag it as left-recursive.
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
    /// Errors when `%word` names a rule that has no definition.
    fn word_check_errors_on_undefined_rule() {
        let mut g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        g.declare_word(di("ghost", 3));
        assert_eq!(
            strs(&g.word_check(&g.known_rules())),
            vec!["error: %word references undefined rule 'ghost' (line 3)"]
        );
    }

    #[test]
    /// No error when the `%word` rule is defined and its body is a pure token.
    fn word_check_no_error_when_rule_defined() {
        let mut g = Grammar::from_rules([p("ident", TerminalLiteral("'x'".into()))]);
        g.declare_word(di("ident", 1));
        assert!(g.word_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// No error when no `%word` directive is present.
    fn word_check_no_error_when_absent() {
        let g = Grammar::from_rules([p("root", TerminalLiteral("'x'".into()))]);
        assert!(g.word_check(&g.known_rules()).is_empty());
    }

    #[test]
    /// Errors when the `%word` rule's body isn't a pure token, e.g. a `seq`
    /// referencing other rules, mirroring upstream's `NonTerminalWordTokenError`.
    fn word_check_errors_on_non_token_body() {
        let mut g = Grammar::from_rules([
            p(
                "identifier",
                GrammarNode::Sequence(vec![NonTerminal("letter".into())]),
            ),
            p("letter", TerminalLiteral("'a'".into())),
        ]);
        g.declare_word(di("identifier", 1));
        assert_eq!(
            strs(&g.word_check(&g.known_rules())),
            vec!["error: %word rule 'identifier' must be a pure token (test.bnf:1)"]
        );
    }

    #[test]
    /// Errors when the `%word` rule's body is structurally identical to another
    /// rule's, naming the conflicting rule, mirroring upstream's
    /// `conflicting_symbol_name`.
    fn word_check_errors_on_duplicate_body() {
        let mut g = Grammar::from_rules([
            p("ident", TerminalLiteral("'x'".into())),
            p("other_name", TerminalLiteral("'x'".into())),
        ]);
        g.declare_word(di("ident", 1));
        assert_eq!(
            strs(&g.word_check(&g.known_rules())),
            vec!["error: %word rule 'ident' has the same body as rule 'other_name' (test.bnf:1)"]
        );
    }

    // ── hidden_start_rule_check ──────────────────────────────────────────────

    #[test]
    /// Errors when `%axiom` names a hidden rule, using the `%axiom` directive's own location.
    fn hidden_start_rule_check_errors_when_axiom_is_hidden() {
        let mut g = Grammar::from_rules([
            p("_hidden", TerminalLiteral("'x'".into())),
            p("visible", TerminalLiteral("'y'".into())),
        ]);
        g.declare_axiom(di("_hidden", 5));
        assert_eq!(
            strs(&g.hidden_start_rule_check()),
            vec![
                "error: start rule '_hidden' cannot be hidden (rule names starting with '_' are not allowed as the grammar's start symbol) (line 5)"
            ]
        );
    }

    #[test]
    /// Errors when there is no `%axiom` and the implicit first-declared rule is hidden,
    /// using that rule's own declaration location.
    fn hidden_start_rule_check_errors_when_implicit_first_rule_is_hidden() {
        let g = Grammar::from_rules([
            p("_hidden", TerminalLiteral("'x'".into())),
            p("visible", TerminalLiteral("'y'".into())),
        ]);
        assert_eq!(
            strs(&g.hidden_start_rule_check()),
            vec![
                "error: start rule '_hidden' cannot be hidden (rule names starting with '_' are not allowed as the grammar's start symbol) (test.bnf:1)"
            ]
        );
    }

    #[test]
    /// No error when the resolved start rule is visible, whether via `%axiom` or implicitly.
    fn hidden_start_rule_check_no_error_when_visible() {
        let mut g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", TerminalLiteral("'y'".into())),
        ]);
        assert!(g.hidden_start_rule_check().is_empty());

        g.declare_axiom(di("b", 1));
        assert!(g.hidden_start_rule_check().is_empty());
    }

    #[test]
    /// Errors when there is no `%axiom` and the implicit first-declared rule is hidden
    /// via `%supertypes` membership, using that rule's own declaration location.
    fn hidden_start_rule_check_errors_when_implicit_first_rule_is_supertype() {
        let mut g = Grammar::from_rules([
            p("expr", TerminalLiteral("'x'".into())),
            p("other", TerminalLiteral("'y'".into())),
        ]);
        g.supertypes = vec![di("expr", 0)];
        assert_eq!(
            strs(&g.hidden_start_rule_check()),
            vec![
                "error: start rule 'expr' cannot be hidden (rules listed in %supertypes are hidden and cannot be the grammar's start symbol) (test.bnf:1)"
            ]
        );
    }

    #[test]
    /// Errors when `%axiom` names a rule that's hidden via `%supertypes` membership,
    /// using the `%axiom` directive's own location.
    fn hidden_start_rule_check_errors_when_axiom_is_supertype() {
        let mut g = Grammar::from_rules([
            p("expr", TerminalLiteral("'x'".into())),
            p("other", TerminalLiteral("'y'".into())),
        ]);
        g.supertypes = vec![di("expr", 0)];
        g.declare_axiom(di("expr", 5));
        assert_eq!(
            strs(&g.hidden_start_rule_check()),
            vec![
                "error: start rule 'expr' cannot be hidden (rules listed in %supertypes are hidden and cannot be the grammar's start symbol) (line 5)"
            ]
        );
    }

    #[test]
    /// No error when `%supertypes` lists a rule other than the resolved start rule.
    fn hidden_start_rule_check_no_error_when_supertype_is_not_start_rule() {
        let mut g = Grammar::from_rules([
            p("root", TerminalLiteral("'x'".into())),
            p("expr", TerminalLiteral("'y'".into())),
        ]);
        g.supertypes = vec![di("expr", 0)];
        assert!(g.hidden_start_rule_check().is_empty());
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
