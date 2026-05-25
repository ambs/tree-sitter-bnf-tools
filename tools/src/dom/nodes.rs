use std::fmt;
use std::fmt::{Display, Formatter};

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
}
