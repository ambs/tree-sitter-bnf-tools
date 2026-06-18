//! Semantic analyses over a [`Grammar`]: FIRST sets, left-recursion detection, and more to come.
//!
//! All functions are pure: they take a `&Grammar` and return computed results
//! without modifying the grammar.
//!
//! # Available analyses
//!
//! | Function | Returns | Purpose |
//! |---|---|---|
//! | [`first_sets`] | Leading terminals per rule | LL(1) feasibility checks |
//! | [`left_recursive_rules`] | Left-recursive rule names | Surface a structural grammar property |
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
//! let g = Grammar::from_rules([
//!     Production { name: "sign".into(), body: GrammarNode::Choice(vec![
//!         GrammarNode::TerminalLiteral("'+'".into()),
//!         GrammarNode::TerminalLiteral("'-'".into()),
//!     ]), line: 1, filename: "test.bnf".into() },
//! ]);
//! let f = first_sets(&g);
//! assert!(f["sign"].contains(&FirstTerminal::Literal("'+'")));
//! assert!(f["sign"].contains(&FirstTerminal::Literal("'-'")));
//! ```

use std::collections::{HashMap, HashSet};

use super::nodes::GrammarNode;
use super::types::Grammar;

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
        GrammarNode::Alias(body, _) => is_nullable(body, nullable),
        GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => is_nullable(inner, nullable),
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
        GrammarNode::Alias(body, _) => collect_first(body, first, nullable, result),
        GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => collect_first(inner, first, nullable, result),
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

/// Collects the non-terminals that can appear as the leftmost symbol of `node`.
///
/// Similar to [`collect_first`] but tracks non-terminal names instead of terminal
/// symbols.  Returns `true` if `node` can produce the empty string.
fn collect_leading_nts<'g>(
    node: &'g GrammarNode,
    nullable: &HashSet<&str>,
    result: &mut HashSet<&'g str>,
) -> bool {
    match node {
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => false,
        GrammarNode::NonTerminal(name) => {
            result.insert(name.as_str());
            nullable.contains(name.as_str())
        }
        GrammarNode::Sequence(children) => {
            for child in children {
                if !collect_leading_nts(child, nullable, result) {
                    return false;
                }
            }
            true
        }
        GrammarNode::Choice(children) => {
            let mut any_nullable = false;
            for child in children {
                if collect_leading_nts(child, nullable, result) {
                    any_nullable = true;
                }
            }
            any_nullable
        }
        GrammarNode::Optional(inner) | GrammarNode::ZeroOrMore(inner) => {
            collect_leading_nts(inner, nullable, result);
            true
        }
        GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner) => collect_leading_nts(inner, nullable, result),
        GrammarNode::Alias(body, _) => collect_leading_nts(body, nullable, result),
        GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => collect_leading_nts(inner, nullable, result),
    }
}

/// Returns the left-recursive rules in the grammar, sorted alphabetically.
///
/// Each element is `(rule_name, is_direct)`:
/// - `is_direct = true`: the rule's body can immediately start with the rule
///   itself (e.g. `expr ::= expr '+' term | term`).
/// - `is_direct = false`: the rule is mutually left-recursive — it starts with
///   another rule that transitively starts with it (e.g. `A ::= B …` and
///   `B ::= A …`).
///
/// Left recursion is a grammar *property*, not a defect: tree-sitter is GLR
/// and supports both forms — left recursion is the documented idiomatic style
/// for binary/postfix expression rules. What actually breaks
/// `tree-sitter generate` is *unresolved ambiguity*, whose ahead-of-time
/// detection is tracked separately (issue #31, A-01). The counts derived from
/// this analysis are surfaced informationally in `check --summary`.
///
/// # Examples
///
/// Direct left-recursion:
///
/// ```
/// use ts_bnf_tool::dom::{Grammar, GrammarNode, Production};
/// use ts_bnf_tool::dom::analysis::left_recursive_rules;
///
/// // expr ::= expr '+' 'n' | 'n'
/// let g = Grammar::from_rules([
///     Production { name: "expr".into(), body: GrammarNode::Choice(vec![
///         GrammarNode::Sequence(vec![
///             GrammarNode::NonTerminal("expr".into()),
///             GrammarNode::TerminalLiteral("'+'".into()),
///             GrammarNode::TerminalLiteral("'n'".into()),
///         ]),
///         GrammarNode::TerminalLiteral("'n'".into()),
///     ]), line: 1, filename: "test.bnf".into() },
/// ]);
/// let lr = left_recursive_rules(&g);
/// assert_eq!(lr, vec![("expr", true)]);
/// ```
///
/// Mutual left-recursion:
///
/// ```
/// use ts_bnf_tool::dom::{Grammar, GrammarNode, Production};
/// use ts_bnf_tool::dom::analysis::left_recursive_rules;
///
/// // a ::= b 'x' | 'a'   b ::= a 'y' | 'b'
/// let g = Grammar::from_rules([
///     Production { name: "a".into(), body: GrammarNode::Choice(vec![
///         GrammarNode::Sequence(vec![
///             GrammarNode::NonTerminal("b".into()),
///             GrammarNode::TerminalLiteral("'x'".into()),
///         ]),
///         GrammarNode::TerminalLiteral("'a'".into()),
///     ]), line: 1, filename: "test.bnf".into() },
///     Production { name: "b".into(), body: GrammarNode::Choice(vec![
///         GrammarNode::Sequence(vec![
///             GrammarNode::NonTerminal("a".into()),
///             GrammarNode::TerminalLiteral("'y'".into()),
///         ]),
///         GrammarNode::TerminalLiteral("'b'".into()),
///     ]), line: 1, filename: "test.bnf".into() },
/// ]);
/// let lr = left_recursive_rules(&g);
/// assert_eq!(lr, vec![("a", false), ("b", false)]);
/// ```
pub fn left_recursive_rules(grammar: &Grammar) -> Vec<(&str, bool)> {
    let nullable = nullable_rules(grammar);

    // One-step: direct leading non-terminals of each rule's body.
    let one_step: HashMap<&str, HashSet<&str>> = grammar
        .productions
        .values()
        .map(|p| {
            let mut result = HashSet::new();
            collect_leading_nts(&p.body, &nullable, &mut result);
            (p.name.as_str(), result)
        })
        .collect();

    // Transitive closure via fixpoint.
    let mut transitive = one_step.clone();
    loop {
        let snapshot = transitive.clone();
        let mut changed = false;
        for prod in grammar.productions.values() {
            let rule = prod.name.as_str();
            let extra: HashSet<&str> = snapshot[rule]
                .iter()
                .flat_map(|nt| snapshot.get(nt).into_iter().flatten().copied())
                .collect();
            let entry = transitive.get_mut(rule).unwrap();
            for nt in extra {
                if entry.insert(nt) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut result: Vec<(&str, bool)> = transitive
        .iter()
        .filter(|(rule, nts)| nts.contains(*rule))
        .map(|(rule, _)| {
            let is_direct = one_step[*rule].contains(*rule);
            (*rule, is_direct)
        })
        .collect();
    result.sort_unstable_by_key(|(name, _)| *name);
    result
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
/// let g = Grammar::from_rules([
///     Production { name: "word".into(), body: GrammarNode::TerminalPattern("/[a-z]+/".into()), line: 1, filename: "test.bnf".into() },
///     Production { name: "kw".into(),   body: GrammarNode::TerminalLiteral("'if'".into()), line: 1, filename: "test.bnf".into() },
/// ]);
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
/// let g = Grammar::from_rules([
///     Production { name: "digit".into(), body: GrammarNode::TerminalPattern("/[0-9]/".into()), line: 1, filename: "test.bnf".into() },
///     Production { name: "num".into(),   body: GrammarNode::NonTerminal("digit".into()), line: 1, filename: "test.bnf".into() },
/// ]);
/// let f = first_sets(&g);
/// assert_eq!(f["num"], f["digit"]);
/// ```
pub fn first_sets(grammar: &Grammar) -> HashMap<&str, HashSet<FirstTerminal<'_>>> {
    let nullable = nullable_rules(grammar);

    // Seed every rule with an empty set.
    let mut first: HashMap<&str, HashSet<FirstTerminal<'_>>> = grammar
        .productions
        .values()
        .map(|p| (p.name.as_str(), HashSet::new()))
        .collect();

    // Iterate until no new terminals are added to any set.  Each pass may
    // propagate terminals through non-terminal references; the fixpoint is
    // reached when a full pass adds nothing new.  We snapshot `first` before
    // each pass so that reads and writes don't alias.
    loop {
        let snapshot = first.clone();
        let mut changed = false;
        for prod in grammar.productions.values() {
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

/// Returns the number of productions whose body contains no [`GrammarNode::NonTerminal`]
/// references — i.e. rules that are defined entirely in terms of terminals.
pub fn count_leaf_rules(grammar: &Grammar) -> usize {
    grammar
        .productions
        .values()
        .filter(|p| !p.body.contains_nonterminal())
        .count()
}

/// Walks `node` recursively, inserting every [`GrammarNode::TerminalLiteral`]
/// into `literals` and every [`GrammarNode::TerminalPattern`] into `patterns`.
fn collect_terminals<'g>(
    node: &'g GrammarNode,
    literals: &mut HashSet<&'g str>,
    patterns: &mut HashSet<&'g str>,
) {
    match node {
        GrammarNode::TerminalLiteral(s) => {
            literals.insert(s.as_str());
        }
        GrammarNode::TerminalPattern(s) => {
            patterns.insert(s.as_str());
        }
        GrammarNode::NonTerminal(_) => {}
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_terminals(child, literals, patterns);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner) => collect_terminals(inner, literals, patterns),
        GrammarNode::Alias(body, _) => collect_terminals(body, literals, patterns),
        GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => collect_terminals(inner, literals, patterns),
    }
}

/// Returns the number of unique terminal literals and unique terminal patterns
/// across all production bodies in the grammar.
///
/// The returned tuple is `(unique_literals, unique_patterns)`. Uniqueness is
/// determined by the raw source string (e.g. `'x'` and `"x"` count as two
/// distinct literals even if they match the same character).
pub fn count_unique_terminals(grammar: &Grammar) -> (usize, usize) {
    let mut literals = HashSet::new();
    let mut patterns = HashSet::new();
    for production in grammar.productions.values() {
        collect_terminals(&production.body, &mut literals, &mut patterns);
    }
    (literals.len(), patterns.len())
}

/// Returns the number of directly and mutually left-recursive rules as
/// `(direct, mutual)`.
///
/// Delegates to [`left_recursive_rules`], which computes the full transitive
/// closure, and partitions the result on the `is_direct` flag.
pub fn count_left_recursive(grammar: &Grammar) -> (usize, usize) {
    let (direct, mutual): (Vec<_>, Vec<_>) = left_recursive_rules(grammar)
        .into_iter()
        .partition(|(_, is_direct)| *is_direct);
    (direct.len(), mutual.len())
}

/// Computes min, max, and average FIRST-set size across all productions.
///
/// Returns [`None`] when the grammar has no productions (no sizes to aggregate).
/// This function runs the full [`first_sets`] fixpoint — it is not free and
/// should only be called when `--summary` is requested.
pub fn first_set_stats(grammar: &Grammar) -> Option<super::summary::FirstSetStats> {
    let sets = first_sets(grammar);
    if sets.is_empty() {
        return None;
    }
    let sizes: Vec<usize> = sets.values().map(|s| s.len()).collect();
    let min = *sizes.iter().min().unwrap();
    let max = *sizes.iter().max().unwrap();
    let avg = {
        let sum: usize = sizes.iter().sum();
        let raw = sum as f64 / sizes.len() as f64;
        // Round to one decimal place: shift up, round to integer, shift back.
        // e.g. 3.25 → 32.5 → 33.0 → 3.3
        (raw * 10.0).round() / 10.0
    };
    Some(super::summary::FirstSetStats { min, max, avg })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::*;
    use crate::dom::test_utils::p;
    use crate::dom::{Grammar, Production};

    fn grammar(prods: Vec<Production>) -> Grammar {
        Grammar::from_rules(prods)
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

    // ── is_nullable ───────────────────────────────────────────────────────────

    #[test]
    /// `Reserved` is a transparent structural annotation for nullability, like `Field`/`Prec`.
    fn is_nullable_propagates_through_reserved() {
        let nullable = HashSet::from(["a"]);
        let node = Reserved("kw".into(), Box::new(nt("a")));
        assert!(is_nullable(&node, &nullable));
    }

    // ── terminals ─────────────────────────────────────────────────────────────

    #[test]
    fn first_of_literal() {
        let g = grammar(vec![p("a", lit("'x'"))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_pattern() {
        let g = grammar(vec![p("a", pat("/[0-9]+/"))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[0-9]+/")])
        );
    }

    // ── non-terminal propagation ───────────────────────────────────────────────

    #[test]
    fn first_of_nonterminal_chain() {
        // b -> 'x' ;  a -> b ;  →  FIRST[a] = FIRST[b] = {'x'}
        let g = grammar(vec![p("b", lit("'x'")), p("a", nt("b"))]);
        let f = first_sets(&g);
        assert_eq!(f["a"], HashSet::from([FirstTerminal::Literal("'x'")]));
    }

    #[test]
    fn first_propagates_through_indirect_chain() {
        // c -> 'z' ;  b -> c ;  a -> b ;
        let g = grammar(vec![p("c", lit("'z'")), p("b", nt("c")), p("a", nt("b"))]);
        let f = first_sets(&g);
        assert_eq!(f["b"], HashSet::from([FirstTerminal::Literal("'z'")]));
        assert_eq!(f["a"], HashSet::from([FirstTerminal::Literal("'z'")]));
    }

    #[test]
    fn first_handles_mutual_recursion() {
        // a -> b | 'x' ;  b -> a | 'y' ;  — fixpoint must terminate
        let g = grammar(vec![
            p("a", Choice(vec![nt("b"), lit("'x'")])),
            p("b", Choice(vec![nt("a"), lit("'y'")])),
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
        let g = grammar(vec![p("a", Choice(vec![lit("'x'"), lit("'y'")]))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'"), FirstTerminal::Literal("'y'")])
        );
    }

    // ── sequence ──────────────────────────────────────────────────────────────

    #[test]
    fn first_of_sequence_stops_at_non_nullable() {
        // a -> 'x' 'y' ;  →  only 'x' reachable first
        let g = grammar(vec![p("a", Sequence(vec![lit("'x'"), lit("'y'")]))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_sequence_continues_past_optional() {
        // a -> 'x'? 'y' ;  — the optional may be skipped, so 'y' is also reachable first
        let g = grammar(vec![p(
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
        let g = grammar(vec![p(
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
        let g = grammar(vec![p("a", Optional(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_zero_or_more_includes_inner() {
        let g = grammar(vec![p("a", ZeroOrMore(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_of_one_or_more_includes_inner() {
        let g = grammar(vec![p("a", OneOrMore(Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    // ── transparent wrappers ──────────────────────────────────────────────────

    #[test]
    fn first_transparent_through_field() {
        let g = grammar(vec![p("a", Field("f".into(), Box::new(lit("'x'"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    #[test]
    fn first_transparent_through_token() {
        let g = grammar(vec![p("a", Token(Box::new(pat("/[a-z]/"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[a-z]/")])
        );
    }

    #[test]
    fn first_transparent_through_token_immediate() {
        let g = grammar(vec![p("a", TokenImmediate(Box::new(pat("/[a-z]/"))))]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Pattern("/[a-z]/")])
        );
    }

    #[test]
    fn first_transparent_through_alias() {
        let g = grammar(vec![p(
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
        let g = grammar(vec![p(
            "a",
            Prec(PrecKind::Left, Some(1), Box::new(lit("'x'"))),
        )]);
        assert_eq!(
            first_sets(&g)["a"],
            HashSet::from([FirstTerminal::Literal("'x'")])
        );
    }

    // ── count_leaf_rules ──────────────────────────────────────────────────────

    #[test]
    /// An empty grammar has no leaf rules.
    fn leaf_rules_empty_grammar() {
        assert_eq!(count_leaf_rules(&grammar(vec![])), 0);
    }

    #[test]
    /// A rule whose body is a single terminal is a leaf.
    fn leaf_rules_single_terminal() {
        let g = grammar(vec![p("tok", lit("'x'"))]);
        assert_eq!(count_leaf_rules(&g), 1);
    }

    #[test]
    /// A rule that references another rule is not a leaf.
    fn leaf_rules_nonterminal_body_not_leaf() {
        let g = grammar(vec![p("tok", lit("'x'")), p("expr", nt("tok"))]);
        assert_eq!(count_leaf_rules(&g), 1);
    }

    #[test]
    /// All rules in a purely terminal grammar are counted as leaves.
    fn leaf_rules_all_terminal() {
        let g = grammar(vec![
            p("a", lit("'x'")),
            p("b", TerminalPattern("/[0-9]+/".into())),
            p("c", Choice(vec![lit("'+'"), lit("'-'")])),
        ]);
        assert_eq!(count_leaf_rules(&g), 3);
    }

    #[test]
    /// A token(…) wrapping a terminal is still a leaf — token contains no NonTerminal.
    fn leaf_rules_token_wrapping_terminal_is_leaf() {
        let g = grammar(vec![p(
            "tok",
            Token(Box::new(TerminalPattern("/[a-z]+/".into()))),
        )]);
        assert_eq!(count_leaf_rules(&g), 1);
    }

    // ── count_unique_terminals ────────────────────────────────────────────────

    #[test]
    /// An empty grammar has no terminals.
    fn unique_terminals_empty_grammar() {
        assert_eq!(count_unique_terminals(&grammar(vec![])), (0, 0));
    }

    #[test]
    /// A single literal produces one unique literal and zero patterns.
    fn unique_terminals_single_literal() {
        let g = grammar(vec![p("a", lit("'x'"))]);
        assert_eq!(count_unique_terminals(&g), (1, 0));
    }

    #[test]
    /// A single pattern produces zero literals and one unique pattern.
    fn unique_terminals_single_pattern() {
        let g = grammar(vec![p("a", pat("/[0-9]+/"))]);
        assert_eq!(count_unique_terminals(&g), (0, 1));
    }

    #[test]
    /// The same literal appearing in two rules is counted only once.
    fn unique_terminals_deduplicates_across_rules() {
        let g = grammar(vec![p("a", lit("'x'")), p("b", lit("'x'"))]);
        assert_eq!(count_unique_terminals(&g), (1, 0));
    }

    #[test]
    /// The same literal appearing twice inside one rule body is counted once.
    fn unique_terminals_deduplicates_within_rule() {
        let g = grammar(vec![p("a", Choice(vec![lit("'x'"), lit("'x'")]))]);
        assert_eq!(count_unique_terminals(&g), (1, 0));
    }

    #[test]
    /// Literals and patterns are counted in separate buckets.
    fn unique_terminals_separates_literals_and_patterns() {
        let g = grammar(vec![p(
            "a",
            Sequence(vec![lit("'+'"), pat("/[0-9]+/"), lit("'-'")]),
        )]);
        assert_eq!(count_unique_terminals(&g), (2, 1));
    }

    #[test]
    /// Terminals nested inside token(…) are still collected.
    fn unique_terminals_inside_token() {
        let g = grammar(vec![p("a", Token(Box::new(lit("'x'"))))]);
        assert_eq!(count_unique_terminals(&g), (1, 0));
    }

    #[test]
    /// The alias name node is not a terminal source — only the body is walked.
    fn unique_terminals_alias_name_not_collected() {
        // alias(body='x', name=some_rule) — name is a NonTerminal display label.
        let g = grammar(vec![p(
            "a",
            Alias(Box::new(lit("'x'")), Box::new(nt("label"))),
        )]);
        assert_eq!(count_unique_terminals(&g), (1, 0));
    }

    // ── count_left_recursive ──────────────────────────────────────────────────

    #[test]
    /// A grammar with no recursion returns (0, 0).
    fn left_recursive_none() {
        let g = grammar(vec![p("a", lit("'x'"))]);
        assert_eq!(count_left_recursive(&g), (0, 0));
    }

    #[test]
    /// A directly left-recursive rule (`a → a …`) is counted in the direct bucket.
    fn left_recursive_direct_only() {
        let g = grammar(vec![p(
            "a",
            Choice(vec![Sequence(vec![nt("a"), lit("'x'")]), lit("'y'")]),
        )]);
        assert_eq!(count_left_recursive(&g), (1, 0));
    }

    #[test]
    /// Two rules that are mutually left-recursive appear in the mutual bucket.
    fn left_recursive_mutual_only() {
        let g = grammar(vec![
            p(
                "a",
                Choice(vec![Sequence(vec![nt("b"), lit("'x'")]), lit("'a'")]),
            ),
            p(
                "b",
                Choice(vec![Sequence(vec![nt("a"), lit("'y'")]), lit("'b'")]),
            ),
        ]);
        let (direct, mutual) = count_left_recursive(&g);
        assert_eq!(direct, 0);
        assert_eq!(mutual, 2);
    }

    // ── first_set_stats ───────────────────────────────────────────────────────

    #[test]
    /// An empty grammar has no FIRST sets to aggregate.
    fn first_set_stats_empty_grammar_returns_none() {
        assert_eq!(first_set_stats(&grammar(vec![])), None);
    }

    #[test]
    /// A single rule with one terminal: min = max = avg = 1.
    fn first_set_stats_single_rule_single_terminal() {
        let g = grammar(vec![p("a", lit("'x'"))]);
        let stats = first_set_stats(&g).unwrap();
        assert_eq!(stats.min, 1);
        assert_eq!(stats.max, 1);
        assert_eq!(stats.avg, 1.0);
    }

    #[test]
    /// Two rules with FIRST sets of size 1 and 3: min=1, max=3, avg=2.0.
    fn first_set_stats_min_max_avg() {
        let g = grammar(vec![
            p("a", lit("'x'")),
            p("b", Choice(vec![lit("'x'"), lit("'y'"), lit("'z'")])),
        ]);
        let stats = first_set_stats(&g).unwrap();
        assert_eq!(stats.min, 1);
        assert_eq!(stats.max, 3);
        assert_eq!(stats.avg, 2.0);
    }

    #[test]
    /// Average is rounded to one decimal place (not truncated).
    fn first_set_stats_avg_rounds_to_one_decimal() {
        // Three rules with FIRST sets of size 1, 1, 2 → raw avg = 4/3 ≈ 1.333… → rounded 1.3
        let g = grammar(vec![
            p("a", lit("'x'")),
            p("b", lit("'y'")),
            p("c", Choice(vec![lit("'p'"), lit("'q'")])),
        ]);
        let stats = first_set_stats(&g).unwrap();
        assert_eq!(stats.avg, 1.3);
    }
}
