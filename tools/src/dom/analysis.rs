//! Semantic analyses over a [`Grammar`]: FIRST sets, and more to come.
//!
//! All functions are pure: they take a `&Grammar` and return computed results
//! without modifying the grammar.
//!
//! # Available analyses
//!
//! | Function | Returns | Purpose |
//! |---|---|---|
//! | [`first_sets`] | Leading terminals per rule | LL(1) feasibility checks |
//!
//! # FIRST sets
//!
//! The FIRST set of a rule is the set of terminals that can appear as the first
//! token of any string derived from that rule.  It is the key ingredient in
//! LL(1) parsing: if two alternatives in a `choice(…)` share a terminal in
//! their FIRST sets, a single token of look-ahead cannot distinguish them.
//!
//! ```
//! use ts_bnf_tool::dom::{Grammar, GrammarNode, Production};
//! use ts_bnf_tool::dom::analysis::{first_sets, FirstTerminal};
//!
//! let g = Grammar {
//!     productions: vec![
//!         Production { name: "sign".into(), body: GrammarNode::Choice(vec![
//!             GrammarNode::TerminalLiteral("'+'".into()),
//!             GrammarNode::TerminalLiteral("'-'".into()),
//!         ])},
//!     ],
//!     ..Grammar::new()
//! };
//! let f = first_sets(&g);
//! assert!(f["sign"].contains(&FirstTerminal::Literal("'+'")));
//! assert!(f["sign"].contains(&FirstTerminal::Literal("'-'")));
//! ```

use std::collections::{HashMap, HashSet};

use super::grammar::Grammar;
use super::nodes::GrammarNode;

/// Returns `true` if `node` can produce the empty string given the current
/// set of known-nullable non-terminal names.
///
/// Tree-sitter does not support ε-productions (rules with an empty body), so
/// nullability arises only from `optional(…)` and `repeat(…)` (zero-or-more)
/// combinators, possibly nested inside named rules.  This function is used
/// internally to propagate FIRST sets correctly past nullable prefixes in
/// sequences.
// Kept for the future full implementation of `nullable_rules`.
#[allow(dead_code)]
fn is_nullable(node: &GrammarNode, nullable: &HashSet<&str>) -> bool {
    match node {
        GrammarNode::Optional(_) | GrammarNode::ZeroOrMore(_) => true,
        GrammarNode::Sequence(children) => children.iter().all(|c| is_nullable(c, nullable)),
        GrammarNode::Choice(children) => children.iter().any(|c| is_nullable(c, nullable)),
        GrammarNode::NonTerminal(name) => nullable.contains(name.as_str()),
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => false,
        GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner) => is_nullable(inner, nullable),
        GrammarNode::Field(_, inner) => is_nullable(inner, nullable),
        GrammarNode::Alias(body, _) => is_nullable(body, nullable),
        GrammarNode::Prec(_, _, inner) => is_nullable(inner, nullable),
    }
}

/// A terminal symbol appearing in a FIRST set.
///
/// Borrows its content directly from the [`Grammar`] passed to [`first_sets`],
/// so its lifetime is tied to that grammar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FirstTerminal<'g> {
    /// A quoted string literal terminal, e.g. `'x'` or `"hello"`.
    Literal(&'g str),
    /// A regex pattern terminal, e.g. `/[0-9]+/`.
    Pattern(&'g str),
}

/// Collects the leading terminals of `node` into `result`.
///
/// Uses the current `first` map for non-terminal lookups and `nullable` to
/// decide whether to continue past a nullable prefix in a sequence.
/// Returns `true` if `node` itself can produce the empty string.
fn collect_first<'g>(
    node: &'g GrammarNode,
    first: &HashMap<&str, HashSet<FirstTerminal<'g>>>,
    nullable: &HashSet<&str>,
    result: &mut HashSet<FirstTerminal<'g>>,
) -> bool {
    match node {
        // A terminal contributes itself and is never empty.
        GrammarNode::TerminalLiteral(s) => {
            result.insert(FirstTerminal::Literal(s));
            false
        }
        GrammarNode::TerminalPattern(s) => {
            result.insert(FirstTerminal::Pattern(s));
            false
        }

        // A non-terminal contributes whatever its rule can start with (as
        // computed so far in `first`).  It is nullable only if the fixpoint
        // already recorded it as such.
        GrammarNode::NonTerminal(name) => {
            if let Some(set) = first.get(name.as_str()) {
                result.extend(set.iter().cloned());
            }
            nullable.contains(name.as_str())
        }

        // A sequence contributes the FIRST of its first element.  If that
        // element is nullable we continue to the next, and so on — stopping
        // as soon as we hit a non-nullable element.  The whole sequence is
        // nullable only if every element is.
        GrammarNode::Sequence(children) => {
            for child in children {
                if !collect_first(child, first, nullable, result) {
                    return false;
                }
            }
            true
        }

        // A choice contributes the union of FIRST sets of all alternatives.
        // It is nullable if any alternative is nullable.
        GrammarNode::Choice(children) => {
            let mut any_nullable = false;
            for child in children {
                if collect_first(child, first, nullable, result) {
                    any_nullable = true;
                }
            }
            any_nullable
        }

        // optional / repeat(zero-or-more): contribute the inner FIRST but are
        // always nullable — the inner expression may be skipped entirely.
        GrammarNode::Optional(inner) | GrammarNode::ZeroOrMore(inner) => {
            collect_first(inner, first, nullable, result);
            true
        }

        // repeat1 / token / token.immediate are transparent to FIRST: the
        // first token is determined solely by the inner expression.
        // repeat1 requires at least one occurrence, so it is nullable iff the
        // inner expression is (unusual, but correct by definition).
        GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner) => collect_first(inner, first, nullable, result),

        // field, alias, prec are purely structural annotations that do not
        // change which terminal appears first.
        GrammarNode::Field(_, inner) => collect_first(inner, first, nullable, result),
        GrammarNode::Alias(body, _) => collect_first(body, first, nullable, result),
        GrammarNode::Prec(_, _, inner) => collect_first(inner, first, nullable, result),
    }
}

/// Returns the set of non-terminal names that can produce the empty string.
///
/// In a full LL-parsing context this would be computed by fixpoint iteration:
/// a rule is nullable if its body is nullable (via `optional`, `repeat`, or a
/// sequence of nullable non-terminals), and so on transitively.
///
/// Tree-sitter does not support ε-productions — named rules with a body that
/// can always be skipped entirely are not a recognised construct — so in
/// practice this always returns an empty set.  The function exists as a
/// placeholder for a future full implementation.
fn nullable_rules(_grammar: &Grammar) -> HashSet<&str> {
    HashSet::new()
}

/// Computes the FIRST sets for all non-terminals in the grammar.
///
/// The FIRST set of a rule is the set of terminals that can appear as the
/// **first token** of any string derived from that rule.  This is the standard
/// notion from LL-parsing theory and is useful for checking whether a grammar
/// is LL(1)-feasible: if two alternatives in a `choice(…)` share a terminal in
/// their FIRST sets, a single token of look-ahead cannot distinguish them.
///
/// Wrappers that are transparent to FIRST (`field`, `token`, `token.immediate`,
/// `prec`, `alias`) delegate to their inner expression.  The `optional` and
/// `repeat` (zero-or-more) combinators contribute their inner FIRST set without
/// blocking the rest of the sequence (they are nullable).
///
/// The returned map borrows terminal strings directly from `grammar`, so its
/// lifetime is tied to the grammar.
///
/// # Examples
///
/// Simple literal and pattern terminals:
///
/// ```
/// use ts_bnf_tool::dom::{Grammar, GrammarNode, Production};
/// use ts_bnf_tool::dom::analysis::{first_sets, FirstTerminal};
///
/// let g = Grammar {
///     productions: vec![
///         Production { name: "word".into(), body: GrammarNode::TerminalPattern("/[a-z]+/".into()) },
///         Production { name: "kw".into(),   body: GrammarNode::TerminalLiteral("'if'".into()) },
///     ],
///     ..Grammar::new()
/// };
/// let f = first_sets(&g);
/// assert!(f["word"].contains(&FirstTerminal::Pattern("/[a-z]+/")));
/// assert!(f["kw"].contains(&FirstTerminal::Literal("'if'")));
/// ```
///
/// Propagation through a chain of non-terminals:
///
/// ```
/// use ts_bnf_tool::dom::{Grammar, GrammarNode, Production};
/// use ts_bnf_tool::dom::analysis::{first_sets, FirstTerminal};
///
/// // digit -> /[0-9]/ ;  num -> digit ;
/// let g = Grammar {
///     productions: vec![
///         Production { name: "digit".into(), body: GrammarNode::TerminalPattern("/[0-9]/".into()) },
///         Production { name: "num".into(),   body: GrammarNode::NonTerminal("digit".into()) },
///     ],
///     ..Grammar::new()
/// };
/// let f = first_sets(&g);
/// assert_eq!(f["num"], f["digit"]);
/// ```
pub fn first_sets(grammar: &Grammar) -> HashMap<&str, HashSet<FirstTerminal<'_>>> {
    let nullable = nullable_rules(grammar);

    // Seed every rule with an empty set.
    let mut first: HashMap<&str, HashSet<FirstTerminal<'_>>> = grammar
        .productions
        .iter()
        .map(|p| (p.name.as_str(), HashSet::new()))
        .collect();

    // Iterate until no new terminals are added to any set.  Each pass may
    // propagate terminals through non-terminal references; the fixpoint is
    // reached when a full pass adds nothing new.  We snapshot `first` before
    // each pass so that reads and writes don't alias.
    loop {
        let snapshot = first.clone();
        let mut changed = false;
        for prod in &grammar.productions {
            let mut new_first = HashSet::new();
            collect_first(&prod.body, &snapshot, &nullable, &mut new_first);
            let entry = first.get_mut(prod.name.as_str()).unwrap();
            for token in new_first {
                if entry.insert(token) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::*;
    use crate::dom::{Grammar, Production};

    fn prod(name: &str, body: GrammarNode) -> Production {
        Production {
            name: name.into(),
            body,
        }
    }

    fn grammar(prods: Vec<Production>) -> Grammar {
        Grammar {
            productions: prods,
            ..Grammar::new()
        }
    }

    fn lit(s: &str) -> GrammarNode {
        TerminalLiteral(s.into())
    }
    fn pat(s: &str) -> GrammarNode {
        TerminalPattern(s.into())
    }
    fn nt(s: &str) -> GrammarNode {
        NonTerminal(s.into())
    }

    // ── terminals ─────────────────────────────────────────────────────────────

    #[test]
    fn first_of_literal() {
        let g = grammar(vec![prod("a", lit("'x'"))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_pattern() {
        let g = grammar(vec![prod("a", pat("/[0-9]+/"))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[0-9]+/")])
        );
    }

    // ── non-terminal propagation ───────────────────────────────────────────────

    #[test]
    fn first_of_nonterminal_chain() {
        // b -> 'x' ;  a -> b ;  →  FIRST[a] = FIRST[b] = {'x'}
        let g = grammar(vec![prod("b", lit("'x'")), prod("a", nt("b"))]);
        let f = first_sets(&g);
        assert_eq!(f["a"], HashSet::from([FirstTerminal::Literal("'x'")]));
    }

    #[test]
    fn first_propagates_through_indirect_chain() {
        // c -> 'z' ;  b -> c ;  a -> b ;
        let g = grammar(vec![
            prod("c", lit("'z'")),
            prod("b", nt("c")),
            prod("a", nt("b")),
        ]);
        let f = first_sets(&g);
        assert_eq!(f["b"], HashSet::from([FirstTerminal::Literal("'z'")]));
        assert_eq!(f["a"], HashSet::from([FirstTerminal::Literal("'z'")]));
    }

    #[test]
    fn first_handles_mutual_recursion() {
        // a -> b | 'x' ;  b -> a | 'y' ;  — fixpoint must terminate
        let g = grammar(vec![
            prod("a", Choice(vec![nt("b"), lit("'x'")])),
            prod("b", Choice(vec![nt("a"), lit("'y'")])),
        ]);
        let f = first_sets(&g);
        let expected =
            HashSet::from([FirstTerminal::Literal("'x'"), FirstTerminal::Literal("'y'")]);
        assert_eq!(f["a"], expected);
        assert_eq!(f["b"], expected);
    }

    // ── choice ────────────────────────────────────────────────────────────────

    #[test]
    fn first_of_choice_is_union() {
        let g = grammar(vec![prod("a", Choice(vec![lit("'x'"), lit("'y'")]))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'"), FirstTerminal::Literal("'y'")])
        );
    }

    // ── sequence ──────────────────────────────────────────────────────────────

    #[test]
    fn first_of_sequence_stops_at_non_nullable() {
        // a -> 'x' 'y' ;  →  only 'x' reachable first
        let g = grammar(vec![prod("a", Sequence(vec![lit("'x'"), lit("'y'")]))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_sequence_continues_past_optional() {
        // a -> 'x'? 'y' ;  — the optional may be skipped, so 'y' is also reachable first
        let g = grammar(vec![prod(
            "a",
            Sequence(vec![Optional(Box::new(lit("'x'"))), lit("'y'")]),
        )]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'"), FirstTerminal::Literal("'y'")])
        );
    }

    #[test]
    fn first_of_sequence_continues_past_zero_or_more() {
        // a -> 'x'* 'y' ;
        let g = grammar(vec![prod(
            "a",
            Sequence(vec![ZeroOrMore(Box::new(lit("'x'"))), lit("'y'")]),
        )]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'"), FirstTerminal::Literal("'y'")])
        );
    }

    // ── quantifiers ───────────────────────────────────────────────────────────

    #[test]
    fn first_of_optional_includes_inner() {
        let g = grammar(vec![prod("a", Optional(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_zero_or_more_includes_inner() {
        let g = grammar(vec![prod("a", ZeroOrMore(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_one_or_more_includes_inner() {
        let g = grammar(vec![prod("a", OneOrMore(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    // ── transparent wrappers ──────────────────────────────────────────────────

    #[test]
    fn first_transparent_through_field() {
        let g = grammar(vec![prod("a", Field("f".into(), Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_transparent_through_token() {
        let g = grammar(vec![prod("a", Token(Box::new(pat("/[a-z]/"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[a-z]/")])
        );
    }

    #[test]
    fn first_transparent_through_token_immediate() {
        let g = grammar(vec![prod("a", TokenImmediate(Box::new(pat("/[a-z]/"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[a-z]/")])
        );
    }

    #[test]
    fn first_transparent_through_alias() {
        let g = grammar(vec![prod(
            "a",
            Alias(Box::new(lit("'x'")), Box::new(lit("'y'"))),
        )]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_transparent_through_prec() {
        use crate::dom::PrecKind;
        let g = grammar(vec![prod(
            "a",
            Prec(PrecKind::Left, Some(1), Box::new(lit("'x'"))),
        )]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }
}
