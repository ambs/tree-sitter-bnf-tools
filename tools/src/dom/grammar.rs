use std::collections::HashSet;

use super::analysis::left_recursive_rules;
use super::types::Grammar;

impl Grammar {
    /// Returns a warning for every rule name in `%conflicts` that has no definition.
    fn conflicts_check(&self, known: &HashSet<&str>) -> Vec<String> {
        self.conflicts
            .iter()
            .flatten()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| format!("warning: %conflicts references undefined rule '{name}'"))
            .collect()
    }

    /// Returns a warning for every rule name in `%inline` that has no definition.
    fn inline_check(&self, known: &HashSet<&str>) -> Vec<String> {
        self.inline
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| format!("warning: %inline references undefined rule '{name}'"))
            .collect()
    }

    /// Returns a warning for every rule name in `%supertypes` that has no definition.
    fn supertypes_check(&self, known: &HashSet<&str>) -> Vec<String> {
        self.supertypes
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| format!("warning: %supertypes references undefined rule '{name}'"))
            .collect()
    }

    /// Returns a warning for every rule reference in `%extras` that has no definition.
    fn extras_check(&self, known: &HashSet<&str>) -> Vec<String> {
        self.extras
            .iter()
            .filter(|item| !item.starts_with('/') && !known.contains(item.as_str()))
            .map(|name| format!("warning: %extras references undefined rule '{name}'"))
            .collect()
    }

    /// Returns a warning for every non-terminal referenced in a rule body that has no definition.
    fn undefined_refs_check(&self, known: &HashSet<&str>) -> Vec<String> {
        self.rhs_nonterminals
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .map(|name| format!("warning: undefined rule reference '{name}'"))
            .collect()
    }

    /// Returns a warning for every left-recursive rule (direct or mutual).
    fn left_recursive_check(&self) -> Vec<String> {
        left_recursive_rules(self)
            .into_iter()
            .map(|(rule, is_direct)| {
                let kind = if is_direct { "directly" } else { "mutually" };
                format!("warning: rule '{rule}' is {kind} left-recursive")
            })
            .collect()
    }

    /// Runs all cross-reference checks and returns any diagnostic messages.
    pub fn check(&self) -> Vec<String> {
        let known = self.known_rules();
        let mut warnings = Vec::new();
        warnings.extend(self.conflicts_check(&known));
        warnings.extend(self.inline_check(&known));
        warnings.extend(self.supertypes_check(&known));
        warnings.extend(self.extras_check(&known));
        warnings.extend(self.undefined_refs_check(&known));
        warnings.extend(self.left_recursive_check());
        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::TerminalLiteral;
    use crate::dom::Production;

    #[test]
    fn grammar_display() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "b".into(),
                    body: crate::dom::GrammarNode::NonTerminal("a".into()),
                },
            ],
            ..Grammar::new()
        };
        assert_eq!(g.to_string(), "\na -> 'x'\nb -> $.a");
    }

    #[test]
    fn conflicts_check_warns_on_undefined_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            conflicts: vec![vec!["a".into(), "ghost".into()]],
            ..Grammar::new()
        };
        let warnings = g.conflicts_check(&g.known_rules());
        assert_eq!(
            warnings,
            vec!["warning: %conflicts references undefined rule 'ghost'"]
        );
    }

    #[test]
    fn conflicts_check_no_warnings_when_all_rules_defined() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "b".into(),
                    body: TerminalLiteral("'y'".into()),
                },
            ],
            conflicts: vec![vec!["a".into(), "b".into()]],
            ..Grammar::new()
        };
        assert!(g.conflicts_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn supertypes_check_warns_on_undefined_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            supertypes: vec!["ghost".into()],
            ..Grammar::new()
        };
        let warnings = g.supertypes_check(&g.known_rules());
        assert_eq!(
            warnings,
            vec!["warning: %supertypes references undefined rule 'ghost'"]
        );
    }

    #[test]
    fn supertypes_check_no_warnings_when_all_rules_defined() {
        let g = Grammar {
            productions: vec![Production {
                name: "expression".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            supertypes: vec!["expression".into()],
            ..Grammar::new()
        };
        assert!(g.supertypes_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn inline_check_warns_on_undefined_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            inline: vec!["ghost".into()],
            ..Grammar::new()
        };
        let warnings = g.inline_check(&g.known_rules());
        assert_eq!(
            warnings,
            vec!["warning: %inline references undefined rule 'ghost'"]
        );
    }

    #[test]
    fn inline_check_no_warnings_when_all_rules_defined() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            inline: vec!["a".into()],
            ..Grammar::new()
        };
        assert!(g.inline_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn extras_check_warns_on_undefined_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            extras: vec!["/\\s/".into(), "ghost".into()],
            ..Grammar::new()
        };
        let warnings = g.extras_check(&g.known_rules());
        assert_eq!(
            warnings,
            vec!["warning: %extras references undefined rule 'ghost'"]
        );
    }

    #[test]
    fn extras_check_no_warning_for_pattern() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            extras: vec!["/\\s/".into()],
            ..Grammar::new()
        };
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn extras_check_no_warnings_when_rule_defined() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "comment".into(),
                    body: TerminalLiteral("'#'".into()),
                },
            ],
            extras: vec!["/\\s/".into(), "comment".into()],
            ..Grammar::new()
        };
        assert!(g.extras_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn undefined_refs_check_warns_on_missing_rule() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("term".into());
        let warnings = g.undefined_refs_check(&g.known_rules());
        assert_eq!(warnings, vec!["warning: undefined rule reference 'term'"]);
    }

    #[test]
    fn undefined_refs_check_no_warning_when_defined() {
        let mut g = Grammar {
            productions: vec![Production {
                name: "term".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            ..Grammar::new()
        };
        g.rhs_nonterminals.insert("term".into());
        assert!(g.undefined_refs_check(&g.known_rules()).is_empty());
    }

    #[test]
    fn undefined_refs_check_deduplicates() {
        let mut g = Grammar::new();
        g.rhs_nonterminals.insert("ghost".into());
        let warnings = g.undefined_refs_check(&g.known_rules());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn check_detects_direct_left_recursion() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar {
            productions: vec![Production {
                name: "expr".into(),
                body: Choice(vec![
                    Sequence(vec![
                        NonTerminal("expr".into()),
                        TerminalLiteral("'+'".into()),
                        TerminalLiteral("'n'".into()),
                    ]),
                    TerminalLiteral("'n'".into()),
                ]),
            }],
            ..Grammar::new()
        };
        let warnings = g.check();
        assert!(warnings
            .iter()
            .any(|w| w.contains("expr") && w.contains("directly left-recursive")));
    }

    #[test]
    fn check_detects_mutual_left_recursion() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: Choice(vec![
                        Sequence(vec![NonTerminal("b".into()), TerminalLiteral("'x'".into())]),
                        TerminalLiteral("'a'".into()),
                    ]),
                },
                Production {
                    name: "b".into(),
                    body: Choice(vec![
                        Sequence(vec![NonTerminal("a".into()), TerminalLiteral("'y'".into())]),
                        TerminalLiteral("'b'".into()),
                    ]),
                },
            ],
            ..Grammar::new()
        };
        let warnings = g.check();
        assert!(warnings
            .iter()
            .any(|w| w.contains("'a'") && w.contains("mutually left-recursive")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("'b'") && w.contains("mutually left-recursive")));
    }

    #[test]
    fn check_no_warning_for_right_recursive_rule() {
        use crate::dom::GrammarNode::{Choice, NonTerminal, Sequence};
        let g = Grammar {
            productions: vec![Production {
                name: "list".into(),
                body: Choice(vec![
                    Sequence(vec![
                        TerminalLiteral("'x'".into()),
                        NonTerminal("list".into()),
                    ]),
                    TerminalLiteral("'x'".into()),
                ]),
            }],
            ..Grammar::new()
        };
        let warnings = g.check();
        assert!(!warnings.iter().any(|w| w.contains("left-recursive")));
    }
}
