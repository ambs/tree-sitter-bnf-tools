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

impl GrammarNode {
    /// Returns all non-terminal names referenced anywhere in this node's subtree.
    ///
    /// Tree-sitter annotations ([`GrammarNode::Token`], [`GrammarNode::Field`], etc.) are
    /// transparent; only [`GrammarNode::Alias`] is special — its name child is a display label,
    /// not a rule reference, so only its body is traversed.
    pub fn nonterminal_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        self.collect_names(&mut names);
        names
    }

    /// Recursive accumulator for [`GrammarNode::nonterminal_names`].
    fn collect_names<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            GrammarNode::NonTerminal(name) => out.push(name),
            GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => {}
            GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
                for c in children {
                    c.collect_names(out);
                }
            }
            GrammarNode::Optional(inner)
            | GrammarNode::ZeroOrMore(inner)
            | GrammarNode::OneOrMore(inner)
            | GrammarNode::Token(inner)
            | GrammarNode::TokenImmediate(inner) => inner.collect_names(out),
            GrammarNode::Field(_, inner) => inner.collect_names(out),
            GrammarNode::Alias(body, _) => body.collect_names(out),
            GrammarNode::Prec(_, _, inner) => inner.collect_names(out),
        }
    }

    /// Returns `true` if this node or any descendant is a [`GrammarNode::NonTerminal`].
    pub fn contains_nonterminal(&self) -> bool {
        match self {
            GrammarNode::NonTerminal(_) => true,
            GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => false,
            GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
                children.iter().any(|c| c.contains_nonterminal())
            }
            GrammarNode::Optional(inner)
            | GrammarNode::ZeroOrMore(inner)
            | GrammarNode::OneOrMore(inner)
            | GrammarNode::Token(inner)
            | GrammarNode::TokenImmediate(inner) => inner.contains_nonterminal(),
            GrammarNode::Field(_, inner) => inner.contains_nonterminal(),
            GrammarNode::Alias(body, _) => body.contains_nonterminal(),
            GrammarNode::Prec(_, _, inner) => inner.contains_nonterminal(),
        }
    }
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
    /// `nonterminal_names` traverses every wrapper variant transparently.
    fn nonterminal_names_traverses_wrappers() {
        let node = Sequence(vec![
            Optional(Box::new(NonTerminal("a".into()))),
            ZeroOrMore(Box::new(NonTerminal("b".into()))),
            OneOrMore(Box::new(NonTerminal("c".into()))),
            Token(Box::new(NonTerminal("d".into()))),
            TokenImmediate(Box::new(NonTerminal("e".into()))),
            Field("f".into(), Box::new(NonTerminal("g".into()))),
            Prec(PrecKind::Left, Some(1), Box::new(NonTerminal("h".into()))),
            Choice(vec![NonTerminal("i".into()), TerminalLiteral("'x'".into())]),
        ]);
        assert_eq!(
            node.nonterminal_names(),
            vec!["a", "b", "c", "d", "e", "g", "h", "i"]
        );
    }

    #[test]
    /// `nonterminal_names` traverses an alias body but not its display name.
    fn nonterminal_names_skips_alias_name() {
        let node = Alias(
            Box::new(NonTerminal("body_rule".into())),
            Box::new(NonTerminal("display_label".into())),
        );
        assert_eq!(node.nonterminal_names(), vec!["body_rule"]);
    }

    // ── contains_nonterminal ───────────────────────────────────────────────────

    #[test]
    /// A bare NonTerminal node must report true.
    fn nonterminal_contains_nonterminal() {
        assert!(NonTerminal("a".into()).contains_nonterminal());
    }

    #[test]
    /// A literal terminal has no rule references.
    fn terminal_literal_does_not_contain_nonterminal() {
        assert!(!TerminalLiteral("'x'".into()).contains_nonterminal());
    }

    #[test]
    /// A regex pattern terminal has no rule references.
    fn terminal_pattern_does_not_contain_nonterminal() {
        assert!(!TerminalPattern("/a+/".into()).contains_nonterminal());
    }

    #[test]
    /// A sequence is non-leaf if any child is a NonTerminal.
    fn sequence_with_nonterminal_contains_nonterminal() {
        let node = Sequence(vec![TerminalLiteral("'x'".into()), NonTerminal("a".into())]);
        assert!(node.contains_nonterminal());
    }

    #[test]
    /// A sequence of only terminals is leaf.
    fn sequence_of_terminals_does_not_contain_nonterminal() {
        let node = Sequence(vec![
            TerminalLiteral("'x'".into()),
            TerminalPattern("/y/".into()),
        ]);
        assert!(!node.contains_nonterminal());
    }

    #[test]
    /// optional(…) propagates the check into its inner node.
    fn optional_nonterminal_contains_nonterminal() {
        assert!(Optional(Box::new(NonTerminal("a".into()))).contains_nonterminal());
    }

    #[test]
    /// token(…) wrapping a terminal is still leaf.
    fn token_wrapping_terminal_does_not_contain_nonterminal() {
        assert!(!Token(Box::new(TerminalPattern("/x/".into()))).contains_nonterminal());
    }

    #[test]
    /// prec(…) propagates the check into its inner node.
    fn prec_wrapping_nonterminal_contains_nonterminal() {
        assert!(
            Prec(PrecKind::Left, Some(1), Box::new(NonTerminal("a".into()))).contains_nonterminal()
        );
    }

    #[test]
    /// In alias(body, name) the name node is a display label, not a rule
    /// reference — only the body is checked.
    fn alias_body_checked_not_name() {
        let node = Alias(
            Box::new(TerminalLiteral("'x'".into())),
            Box::new(NonTerminal("label".into())),
        );
        assert!(!node.contains_nonterminal());
    }
}
