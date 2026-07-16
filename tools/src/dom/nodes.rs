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

impl PrecKind {
    /// The tree-sitter surface name of this precedence kind, e.g. `"prec.left"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            PrecKind::Plain => "prec",
            PrecKind::Left => "prec.left",
            PrecKind::Right => "prec.right",
            PrecKind::Dynamic => "prec.dynamic",
        }
    }
}

/// A `prec(…)` level: either an integer or an already-quoted name.
pub enum PrecLevel {
    /// A numeric precedence level, e.g. the `1` in `prec(1, …)`.
    Integer(i32),
    /// A named precedence level, stored pre-quoted (e.g. `'unary'`) so
    /// `Display` can print it verbatim.
    Name(String),
}

impl Display for PrecLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PrecLevel::Integer(i) => write!(f, "{}", i),
            PrecLevel::Name(name) => write!(f, "{}", name),
        }
    }
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
    Prec(PrecKind, Option<PrecLevel>, Box<GrammarNode>),
    /// `reserved('setName', body)` — opts `body` into a named reserved-word set.
    Reserved(String, Box<GrammarNode>),
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
            GrammarNode::Reserved(_, inner) => inner.collect_names(out),
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
            GrammarNode::Reserved(_, inner) => inner.contains_nonterminal(),
        }
    }

    /// Returns `true` if this node reduces to a bare terminal (literal or
    /// regex), optionally wrapped in transparent annotations
    /// ([`GrammarNode::Token`], [`GrammarNode::TokenImmediate`],
    /// [`GrammarNode::Field`], [`GrammarNode::Alias`], [`GrammarNode::Prec`],
    /// [`GrammarNode::Reserved`]). Anything with real structure underneath
    /// (a [`GrammarNode::Sequence`], [`GrammarNode::Choice`], a reference to
    /// another rule, etc.) is not a pure token.
    pub fn is_pure_token(&self) -> bool {
        match self {
            GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => true,
            GrammarNode::Token(inner)
            | GrammarNode::TokenImmediate(inner)
            | GrammarNode::Field(_, inner)
            | GrammarNode::Alias(inner, _)
            | GrammarNode::Prec(_, _, inner)
            | GrammarNode::Reserved(_, inner) => inner.is_pure_token(),
            _ => false,
        }
    }

    /// Returns `true` if this node occupies exactly one step within its
    /// enclosing alternative: a bare terminal, a rule reference, or a
    /// `?`/`*`/`+` repetition, optionally wrapped in transparent annotations
    /// ([`GrammarNode::Token`], [`GrammarNode::TokenImmediate`],
    /// [`GrammarNode::Field`], [`GrammarNode::Alias`], [`GrammarNode::Prec`],
    /// [`GrammarNode::Reserved`]). A multi-element [`GrammarNode::Sequence`]
    /// is not atomic.
    pub fn is_atomic_node(&self) -> bool {
        match self {
            GrammarNode::TerminalLiteral(_)
            | GrammarNode::TerminalPattern(_)
            | GrammarNode::NonTerminal(_)
            | GrammarNode::Optional(_)
            | GrammarNode::OneOrMore(_)
            | GrammarNode::ZeroOrMore(_) => true,
            GrammarNode::Token(inner)
            | GrammarNode::TokenImmediate(inner)
            | GrammarNode::Field(_, inner)
            | GrammarNode::Alias(inner, _)
            | GrammarNode::Prec(_, _, inner)
            | GrammarNode::Reserved(_, inner) => inner.is_atomic_node(),
            _ => false,
        }
    }

    /// Returns `true` if every alternative of this production body has
    /// exactly one step: if `self` is a [`GrammarNode::Choice`], every
    /// element must be atomic; otherwise `self` is the production's only
    /// alternative, so it must be atomic on its own.
    pub fn single_choice_options(&self) -> bool {
        match self {
            GrammarNode::Token(inner)
            | GrammarNode::TokenImmediate(inner)
            | GrammarNode::Field(_, inner)
            | GrammarNode::Alias(inner, _)
            | GrammarNode::Prec(_, _, inner)
            | GrammarNode::Reserved(_, inner) => inner.single_choice_options(),
            GrammarNode::Choice(children) => children.iter().all(|c| c.is_atomic_node()),
            node if node.is_atomic_node() => true,
            _ => false,
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
                let name = kind.as_str();
                match level {
                    Some(n) => write!(f, "{}({}, {})", name, n, inner),
                    None => write!(f, "{}({})", name, inner),
                }
            }
            GrammarNode::Reserved(name, inner) => {
                write!(f, "reserved('{}', {})", name, inner)
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
            Prec(
                PrecKind::Left,
                Some(PrecLevel::Integer(1)),
                Box::new(NonTerminal("h".into())),
            ),
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

    #[test]
    /// `nonterminal_names` traverses a `Reserved` body; the set name (a plain
    /// `String`, not a node) cannot appear in the result.
    fn nonterminal_names_traverses_reserved_body() {
        let node = Reserved("kw".into(), Box::new(NonTerminal("a".into())));
        assert_eq!(node.nonterminal_names(), vec!["a"]);
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
            Prec(
                PrecKind::Left,
                Some(PrecLevel::Integer(1)),
                Box::new(NonTerminal("a".into()))
            )
            .contains_nonterminal()
        );
    }

    #[test]
    /// A negative precedence level is emitted verbatim in the prec(…) call.
    fn prec_negative_level_displays_verbatim() {
        let node = Prec(
            PrecKind::Plain,
            Some(PrecLevel::Integer(-1)),
            Box::new(NonTerminal("a".into())),
        );
        assert_eq!(node.to_string(), "prec(-1, $.a)");
    }

    #[test]
    /// A literal precedence level is emitted verbatim in the prec(…) call.
    fn prec_literal_level_displays_verbatim() {
        let node = Prec(
            PrecKind::Plain,
            Some(PrecLevel::Name("'unary'".into())),
            Box::new(NonTerminal("a".into())),
        );
        assert_eq!(node.to_string(), "prec('unary', $.a)");
    }

    #[test]
    /// reserved(…) propagates the check into its inner node.
    fn reserved_wrapping_nonterminal_contains_nonterminal() {
        assert!(Reserved("kw".into(), Box::new(NonTerminal("a".into()))).contains_nonterminal());
    }

    #[test]
    /// `Display` emits the tree-sitter `reserved('name', body)` call form.
    fn reserved_node_displays_as_function_call() {
        let node = Reserved("kw".into(), Box::new(NonTerminal("a".into())));
        assert_eq!(node.to_string(), "reserved('kw', $.a)");
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

    // ── is_pure_token ────────────────────────────────────────────────────────

    #[test]
    /// A bare string literal is a pure token.
    fn terminal_literal_is_pure_token() {
        assert!(TerminalLiteral("'x'".into()).is_pure_token());
    }

    #[test]
    /// A bare regex pattern is a pure token.
    fn terminal_pattern_is_pure_token() {
        assert!(TerminalPattern("/a+/".into()).is_pure_token());
    }

    #[test]
    /// A reference to another rule is never a pure token.
    fn nonterminal_is_not_pure_token() {
        assert!(!NonTerminal("a".into()).is_pure_token());
    }

    #[test]
    /// A sequence has real structure, even if every element is a terminal.
    fn sequence_of_terminals_is_not_pure_token() {
        let node = Sequence(vec![
            TerminalLiteral("'x'".into()),
            TerminalPattern("/y/".into()),
        ]);
        assert!(!node.is_pure_token());
    }

    #[test]
    /// A choice between terminals still has real structure.
    fn choice_of_terminals_is_not_pure_token() {
        let node = Choice(vec![
            TerminalLiteral("'x'".into()),
            TerminalLiteral("'y'".into()),
        ]);
        assert!(!node.is_pure_token());
    }

    #[test]
    /// token(…)/token.immediate(…)/field(…)/prec(…)/reserved(…) wrapping a
    /// bare terminal are all transparent.
    fn annotation_wrappers_around_terminal_are_pure_tokens() {
        assert!(Token(Box::new(TerminalPattern("/x/".into()))).is_pure_token());
        assert!(TokenImmediate(Box::new(TerminalLiteral("'x'".into()))).is_pure_token());
        assert!(Field("f".into(), Box::new(TerminalLiteral("'x'".into()))).is_pure_token());
        assert!(
            Prec(
                PrecKind::Left,
                Some(PrecLevel::Integer(1)),
                Box::new(TerminalLiteral("'x'".into())),
            )
            .is_pure_token()
        );
        assert!(Reserved("kw".into(), Box::new(TerminalLiteral("'x'".into()))).is_pure_token());
    }

    #[test]
    /// alias(body, name) checks the body, not the display-label name.
    fn alias_body_checked_for_pure_token() {
        let node = Alias(
            Box::new(TerminalLiteral("'x'".into())),
            Box::new(NonTerminal("label".into())),
        );
        assert!(node.is_pure_token());
    }

    #[test]
    /// Nested annotation wrappers around a bare terminal are still a pure token.
    fn nested_annotation_wrappers_around_terminal_are_pure_token() {
        let node = Token(Box::new(Field(
            "f".into(),
            Box::new(TerminalPattern("/x/".into())),
        )));
        assert!(node.is_pure_token());
    }

    #[test]
    /// An annotation wrapper around something with real structure is not a
    /// pure token.
    fn annotation_wrapper_around_nonterminal_is_not_pure_token() {
        assert!(!Token(Box::new(NonTerminal("a".into()))).is_pure_token());
        assert!(
            !Field(
                "f".into(),
                Box::new(Sequence(vec![NonTerminal("a".into())]))
            )
            .is_pure_token()
        );
    }

    // ── is_atomic_node ───────────────────────────────────────────────────────

    #[test]
    /// Bare terminals are atomic.
    fn terminals_are_atomic() {
        assert!(TerminalLiteral("'x'".into()).is_atomic_node());
        assert!(TerminalPattern("/a+/".into()).is_atomic_node());
    }

    #[test]
    /// A rule reference is atomic.
    fn nonterminal_is_atomic() {
        assert!(NonTerminal("a".into()).is_atomic_node());
    }

    #[test]
    /// `?`/`*`/`+` repetitions are atomic regardless of what they wrap, since
    /// the repetition itself is still a single step.
    fn repetitions_are_atomic_regardless_of_inner_structure() {
        let multi_step = || Sequence(vec![NonTerminal("a".into()), NonTerminal("b".into())]);
        assert!(Optional(Box::new(multi_step())).is_atomic_node());
        assert!(ZeroOrMore(Box::new(multi_step())).is_atomic_node());
        assert!(OneOrMore(Box::new(multi_step())).is_atomic_node());
    }

    #[test]
    /// A multi-element sequence spans more than one step, so it is not atomic.
    fn multi_element_sequence_is_not_atomic() {
        let node = Sequence(vec![NonTerminal("a".into()), NonTerminal("b".into())]);
        assert!(!node.is_atomic_node());
    }

    #[test]
    /// A choice found where a single step is expected is not atomic.
    fn choice_is_not_atomic() {
        let node = Choice(vec![NonTerminal("a".into()), NonTerminal("b".into())]);
        assert!(!node.is_atomic_node());
    }

    #[test]
    /// token(…)/token.immediate(…)/field(…)/prec(…)/reserved(…) wrapping an
    /// atomic node are all transparent.
    fn annotation_wrappers_around_atomic_node_are_atomic() {
        assert!(Token(Box::new(NonTerminal("a".into()))).is_atomic_node());
        assert!(TokenImmediate(Box::new(NonTerminal("a".into()))).is_atomic_node());
        assert!(Field("f".into(), Box::new(NonTerminal("a".into()))).is_atomic_node());
        assert!(
            Prec(
                PrecKind::Left,
                Some(PrecLevel::Integer(1)),
                Box::new(NonTerminal("a".into())),
            )
            .is_atomic_node()
        );
        assert!(Reserved("kw".into(), Box::new(NonTerminal("a".into()))).is_atomic_node());
    }

    #[test]
    /// alias(body, name) checks the body, not the display-label name.
    fn alias_body_checked_for_atomic_node() {
        let node = Alias(
            Box::new(NonTerminal("a".into())),
            Box::new(NonTerminal("label".into())),
        );
        assert!(node.is_atomic_node());
    }

    #[test]
    /// An annotation wrapper around a multi-element sequence is not atomic.
    fn annotation_wrapper_around_multi_element_sequence_is_not_atomic() {
        let node = Token(Box::new(Sequence(vec![
            NonTerminal("a".into()),
            NonTerminal("b".into()),
        ])));
        assert!(!node.is_atomic_node());
    }

    // ── single_choice_options ────────────────────────────────────────────────

    #[test]
    /// A single, unwrapped alternative with one step is valid.
    fn single_atomic_alternative_has_single_step() {
        assert!(NonTerminal("term".into()).single_choice_options());
    }

    #[test]
    /// A single, unwrapped alternative with more than one step is invalid.
    fn single_multi_step_alternative_fails() {
        let node = Sequence(vec![
            NonTerminal("term".into()),
            TerminalLiteral("'+'".into()),
            NonTerminal("term".into()),
        ]);
        assert!(!node.single_choice_options());
    }

    #[test]
    /// A precedence-annotated single alternative with one step is still valid.
    fn annotation_wrapped_atomic_alternative_has_single_step() {
        let node = Prec(
            PrecKind::Left,
            Some(PrecLevel::Integer(1)),
            Box::new(NonTerminal("term".into())),
        );
        assert!(node.single_choice_options());
    }

    #[test]
    /// A precedence-annotated single alternative with more than one step is
    /// invalid.
    fn annotation_wrapped_multi_step_alternative_fails() {
        let node = Prec(
            PrecKind::Left,
            Some(PrecLevel::Integer(1)),
            Box::new(Sequence(vec![
                NonTerminal("term".into()),
                TerminalLiteral("'+'".into()),
                NonTerminal("term".into()),
            ])),
        );
        assert!(!node.single_choice_options());
    }

    #[test]
    /// `expr -> term | unary | binary ;` — every alternative is a single step.
    fn choice_of_single_step_alternatives_passes() {
        let node = Choice(vec![
            NonTerminal("term".into()),
            NonTerminal("unary".into()),
            NonTerminal("binary".into()),
        ]);
        assert!(node.single_choice_options());
    }

    #[test]
    /// `expr -> term | term '+' term ;` — the second alternative has three steps.
    fn choice_with_one_multi_step_alternative_fails() {
        let node = Choice(vec![
            NonTerminal("term".into()),
            Sequence(vec![
                NonTerminal("term".into()),
                TerminalLiteral("'+'".into()),
                NonTerminal("term".into()),
            ]),
        ]);
        assert!(!node.single_choice_options());
    }
}
