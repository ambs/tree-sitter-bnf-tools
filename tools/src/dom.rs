use std::collections::HashSet;
use std::fmt;
use std::fmt::{Display, Formatter};

/// Errors that can occur while converting a tree-sitter BNF parse tree into the DOM.
#[derive(Debug)]
pub enum ParseError {
    /// A node had a different kind than required; carries the expected and actual kind strings.
    UnexpectedNodeType {
        /// The node kind that was required at this position.
        expected: String,
        /// The node kind that was actually encountered.
        got: String,
    },
    /// A node kind was not recognised by any visitor branch.
    UnknownNodeKind(String),
    /// The left-hand side of a production rule was not a non-terminal.
    MalformedProduction,
    /// The source text contains tree-sitter syntax errors.
    SyntaxError,
    /// The tree-sitter parser returned no tree for the input.
    ParseFailed,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedNodeType { expected, got } => {
                write!(f, "expected node type '{}', got '{}'", expected, got)
            }
            ParseError::UnknownNodeKind(kind) => write!(f, "unknown node kind '{}'", kind),
            ParseError::MalformedProduction => {
                write!(f, "non-terminal expected on left-hand side of production")
            }
            ParseError::SyntaxError => write!(f, "input contains syntax errors"),
            ParseError::ParseFailed => write!(f, "parser returned no tree"),
        }
    }
}

impl std::error::Error for ParseError {}

/// The flavour of tree-sitter precedence annotation.
pub enum PrecKind {
    /// `prec(n, …)` — plain (non-associative) precedence.
    Plain,
    /// `prec.left(n, …)` — left-associative precedence.
    Left,
    /// `prec.right(n, …)` — right-associative precedence.
    Right,
    /// `prec.dynamic(n, …)` — dynamic precedence resolved at runtime.
    Dynamic,
}

/// A node in the grammar rule tree, mirroring tree-sitter combinator functions.
pub enum GrammarNode {
    /// `seq(…)` — an ordered sequence of sub-nodes.
    Sequence(Vec<GrammarNode>),
    /// `choice(…)` — an ordered set of alternatives.
    Choice(Vec<GrammarNode>),
    /// `optional(…)` — zero or one occurrence.
    Optional(Box<GrammarNode>),
    /// A quoted string literal terminal (single- or double-quoted).
    TerminalLiteral(String),
    /// A regex pattern terminal enclosed in `/…/`.
    TerminalPattern(String),
    /// A reference to another grammar rule (`$.name`).
    NonTerminal(String),
    /// `repeat(…)` — zero or more occurrences (Kleene star).
    ZeroOrMore(Box<GrammarNode>),
    /// `repeat1(…)` — one or more occurrences (Kleene plus).
    OneOrMore(Box<GrammarNode>),
    /// `token(…)` — forces the inner expression to be lexed as a single token.
    Token(Box<GrammarNode>),
    /// `token.immediate(…)` — like `token`, but only matches when no whitespace precedes it.
    TokenImmediate(Box<GrammarNode>),
    /// `field('name', …)` — attaches a named field to a child node.
    Field(String, Box<GrammarNode>),
    /// `alias(body, name)` — renames a node in the syntax tree.
    Alias(Box<GrammarNode>, Box<GrammarNode>),
    /// `prec[.left|.right|.dynamic](level, …)` — precedence annotation.
    Prec(PrecKind, Option<u32>, Box<GrammarNode>),
}

impl Display for GrammarNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GrammarNode::Sequence(s) => {
                write!(
                    f,
                    "seq({})",
                    s.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            GrammarNode::Choice(c) => {
                write!(
                    f,
                    "choice({})",
                    c.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            GrammarNode::TerminalLiteral(l) => {
                write!(f, "{}", l)
            }
            GrammarNode::TerminalPattern(p) => {
                write!(f, "{}", p)
            }
            GrammarNode::NonTerminal(nt) => {
                write!(f, "$.{}", nt)
            }
            GrammarNode::ZeroOrMore(zm) => {
                write!(f, "repeat({})", zm)
            }
            GrammarNode::OneOrMore(om) => {
                write!(f, "repeat1({})", om)
            }
            GrammarNode::Optional(om) => {
                write!(f, "optional({})", om)
            }
            GrammarNode::Token(inner) => {
                write!(f, "token({})", inner)
            }
            GrammarNode::TokenImmediate(inner) => {
                write!(f, "token.immediate({})", inner)
            }
            GrammarNode::Field(name, inner) => {
                write!(f, "field('{}', {})", name, inner)
            }
            GrammarNode::Alias(body, name) => {
                write!(f, "alias({}, {})", body, name)
            }
            GrammarNode::Prec(kind, level, inner) => {
                let name = match kind {
                    PrecKind::Plain => "prec",
                    PrecKind::Left => "prec.left",
                    PrecKind::Right => "prec.right",
                    PrecKind::Dynamic => "prec.dynamic",
                };
                match level {
                    Some(n) => write!(f, "{}({}, {})", name, n, inner),
                    None => write!(f, "{}({})", name, inner),
                }
            }
        }
    }
}

/// A single named grammar rule (`name -> body`).
pub struct Production {
    /// The rule name (left-hand side of `->`)
    pub name: String,
    /// The rule body (right-hand side of `->`).
    pub body: GrammarNode,
}

impl Display for Production {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        fmt.write_str(&self.name)?;
        fmt.write_str(" -> ")?;
        write!(fmt, "{}", &self.body)?;
        Ok(())
    }
}

/// The complete grammar: all productions and any declared conflict or inline groups.
pub struct Grammar {
    /// Ordered list of grammar rules.
    pub productions: Vec<Production>,
    /// Conflict groups declared with `%conflicts`; each inner `Vec` is one group.
    pub conflicts: Vec<Vec<String>>,
    /// Rule names declared with `%inline` that should be inlined at every call site.
    pub inline: Vec<String>,
    /// Abstract rule names declared with `%supertypes` that group concrete subtypes.
    pub supertypes: Vec<String>,
    /// Extra items declared with `%extras`; each is either a regex pattern (starts with `/`) or a rule name.
    pub extras: Vec<String>,
    /// All non-terminal names that appear on right-hand sides of rules, accumulated by the visitor.
    pub rhs_nonterminals: HashSet<String>,
}

impl Grammar {
    /// Creates an empty grammar with no productions, conflicts, inline, supertypes, or extras.
    pub fn new() -> Self {
        Self {
            productions: Vec::new(),
            conflicts: Vec::new(),
            inline: Vec::new(),
            supertypes: Vec::new(),
            extras: Vec::new(),
            rhs_nonterminals: HashSet::new(),
        }
    }

    /// Returns the set of all defined rule names, used for cross-reference checks.
    pub fn known_rules(&self) -> HashSet<&str> {
        self.productions.iter().map(|p| p.name.as_str()).collect()
    }

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

    /// Runs all cross-reference checks and prints any warnings to stderr.
    pub fn check(&self) {
        let known = self.known_rules();
        let mut warnings = Vec::new();
        warnings.extend(self.conflicts_check(&known));
        warnings.extend(self.inline_check(&known));
        warnings.extend(self.supertypes_check(&known));
        warnings.extend(self.extras_check(&known));
        warnings.extend(self.undefined_refs_check(&known));
        for warning in warnings {
            eprintln!("{warning}");
        }
    }
}

impl Display for Grammar {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for production in &self.productions {
            write!(fmt, "\n{}", production)?;
        }
        Ok(())
    }
}

/// A wrapper that renders a [`Grammar`] as a complete `grammar.js` file.
pub struct Scaffold<'a> {
    /// The grammar to render.
    pub grammar: &'a Grammar,
    /// The grammar name passed to tree-sitter's `grammar({ name: … })`.
    pub name: &'a str,
}

impl Display for Scaffold<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "module.exports = grammar({{")?;
        writeln!(f, "  name: \"{}\",", self.name)?;
        writeln!(f)?;
        if !self.grammar.extras.is_empty() {
            let items = self
                .grammar
                .extras
                .iter()
                .map(|item| {
                    if item.starts_with('/') {
                        item.clone()
                    } else {
                        format!("$.{item}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  extras: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.inline.is_empty() {
            let items = self
                .grammar
                .inline
                .iter()
                .map(|n| format!("$.{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  inline: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.supertypes.is_empty() {
            let items = self
                .grammar
                .supertypes
                .iter()
                .map(|n| format!("$.{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  supertypes: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.conflicts.is_empty() {
            writeln!(f, "  conflicts: $ => [")?;
            for group in &self.grammar.conflicts {
                let items = group
                    .iter()
                    .map(|n| format!("$.{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(f, "    [{items}],")?;
            }
            writeln!(f, "  ],")?;
            writeln!(f)?;
        }
        writeln!(f, "  rules: {{")?;
        for production in &self.grammar.productions {
            writeln!(f, "    {}: $ => {},", production.name, production.body)?;
        }
        writeln!(f, "  }}")?;
        write!(f, "}});")
    }
}

#[cfg(test)]
mod tests {
    use super::GrammarNode::*;
    use super::*;

    #[test]
    fn nonterminal_display() {
        assert_eq!(NonTerminal("foo".into()).to_string(), "$.foo");
    }

    #[test]
    fn terminal_literal_display() {
        assert_eq!(TerminalLiteral("'x'".into()).to_string(), "'x'");
    }

    #[test]
    fn terminal_pattern_display() {
        assert_eq!(TerminalPattern("/a+/".into()).to_string(), "/a+/");
    }

    #[test]
    fn zero_or_more_display() {
        assert_eq!(
            ZeroOrMore(Box::new(NonTerminal("a".into()))).to_string(),
            "repeat($.a)"
        );
    }

    #[test]
    fn one_or_more_display() {
        assert_eq!(
            OneOrMore(Box::new(NonTerminal("a".into()))).to_string(),
            "repeat1($.a)"
        );
    }

    #[test]
    fn optional_display() {
        assert_eq!(
            Optional(Box::new(NonTerminal("a".into()))).to_string(),
            "optional($.a)"
        );
    }

    #[test]
    fn token_display() {
        assert_eq!(
            Token(Box::new(TerminalPattern("/[0-9]+/".into()))).to_string(),
            "token(/[0-9]+/)"
        );
    }

    #[test]
    fn token_immediate_display() {
        assert_eq!(
            TokenImmediate(Box::new(TerminalPattern("/[0-9]+/".into()))).to_string(),
            "token.immediate(/[0-9]+/)"
        );
    }

    #[test]
    fn field_display() {
        assert_eq!(
            Field("lhs".into(), Box::new(NonTerminal("expr".into()))).to_string(),
            "field('lhs', $.expr)"
        );
    }

    #[test]
    fn alias_display() {
        assert_eq!(
            Alias(
                Box::new(NonTerminal("foo".into())),
                Box::new(NonTerminal("bar".into()))
            )
            .to_string(),
            "alias($.foo, $.bar)"
        );
    }

    #[test]
    fn prec_plain_display() {
        assert_eq!(
            Prec(PrecKind::Plain, Some(1), Box::new(NonTerminal("a".into()))).to_string(),
            "prec(1, $.a)"
        );
    }

    #[test]
    fn prec_left_display() {
        assert_eq!(
            Prec(PrecKind::Left, Some(2), Box::new(NonTerminal("a".into()))).to_string(),
            "prec.left(2, $.a)"
        );
    }

    #[test]
    fn prec_right_no_level_display() {
        assert_eq!(
            Prec(PrecKind::Right, None, Box::new(NonTerminal("a".into()))).to_string(),
            "prec.right($.a)"
        );
    }

    #[test]
    fn prec_dynamic_display() {
        assert_eq!(
            Prec(
                PrecKind::Dynamic,
                Some(3),
                Box::new(TerminalLiteral("'x'".into()))
            )
            .to_string(),
            "prec.dynamic(3, 'x')"
        );
    }

    #[test]
    fn token_sequence_display() {
        assert_eq!(
            Token(Box::new(Sequence(vec![
                TerminalPattern("/[A-Za-z_]/".into()),
                TerminalPattern("/[A-Za-z0-9_]*/".into()),
            ])))
            .to_string(),
            "token(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))"
        );
    }

    #[test]
    fn sequence_display() {
        assert_eq!(
            Sequence(vec![
                NonTerminal("a".into()),
                NonTerminal("b".into()),
                NonTerminal("c".into()),
            ])
            .to_string(),
            "seq($.a, $.b, $.c)"
        );
    }

    #[test]
    fn choice_display() {
        assert_eq!(
            Choice(vec![NonTerminal("a".into()), NonTerminal("b".into())]).to_string(),
            "choice($.a, $.b)"
        );
    }

    #[test]
    fn production_display() {
        let p = Production {
            name: "expr".into(),
            body: NonTerminal("a".into()),
        };
        assert_eq!(p.to_string(), "expr -> $.a");
    }

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
                    body: NonTerminal("a".into()),
                },
            ],
            ..Grammar::new()
        };
        assert_eq!(g.to_string(), "\na -> 'x'\nb -> $.a");
    }

    #[test]
    fn scaffold_single_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "expr".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            ..Grammar::new()
        };
        assert_eq!(
            Scaffold { grammar: &g, name: "expr" }.to_string(),
            "module.exports = grammar({\n  name: \"expr\",\n\n  rules: {\n    expr: $ => 'x',\n  }\n});"
        );
    }

    #[test]
    fn scaffold_multi_rule() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "b".into(),
                    body: NonTerminal("a".into()),
                },
            ],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "test",
        }
        .to_string();
        assert!(out.contains("    a: $ => 'x',"));
        assert!(out.contains("    b: $ => $.a,"));
        assert!(out.starts_with("module.exports = grammar({"));
        assert!(out.ends_with("});"));
    }

    #[test]
    fn scaffold_name_appears_in_output() {
        let g = Grammar {
            productions: vec![Production {
                name: "r".into(),
                body: TerminalLiteral("'y'".into()),
            }],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "mygrammar",
        }
        .to_string();
        assert!(out.contains("name: \"mygrammar\""));
    }

    #[test]
    fn scaffold_no_conflicts_omits_key() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("conflicts"));
    }

    #[test]
    fn scaffold_with_single_conflict_group() {
        let g = Grammar {
            productions: vec![Production {
                name: "expr".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            conflicts: vec![vec!["expr".into(), "term".into()]],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  conflicts: $ => ["));
        assert!(out.contains("    [$.expr, $.term],"));
        assert!(out.contains("  ],"));
        // conflicts block appears before rules block
        assert!(out.find("conflicts").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_with_multiple_conflict_groups() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            conflicts: vec![
                vec!["a".into(), "b".into()],
                vec!["c".into(), "d".into(), "e".into()],
            ],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("    [$.a, $.b],"));
        assert!(out.contains("    [$.c, $.d, $.e],"));
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
    fn scaffold_with_supertypes() {
        let g = Grammar {
            productions: vec![Production {
                name: "expression".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            supertypes: vec!["expression".into(), "statement".into()],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  supertypes: $ => [$.expression, $.statement],"));
        assert!(out.find("supertypes").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_no_supertypes_omits_key() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("supertypes"));
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
    fn scaffold_with_extras_pattern_and_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            extras: vec!["/\\s/".into(), "comment".into()],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  extras: $ => [/\\s/, $.comment],"));
        assert!(out.find("extras").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_no_extras_omits_key() {
        let g = Grammar {
            productions: vec![Production {
                name: "a".into(),
                body: TerminalLiteral("'x'".into()),
            }],
            ..Grammar::new()
        };
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("extras"));
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
}
