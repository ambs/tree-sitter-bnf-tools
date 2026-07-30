// Derivation of a `Visitor` trait's shape from a `Grammar`: the visible
// node-kind set, per-kind fields and leaf status, and method-name collision
// detection. Target-language emitters (Rust, and later others) consume this
// derived model from sibling modules rather than duplicating the derivation.

use std::collections::HashSet;

use indexmap::{IndexMap, IndexSet};

use super::nodes::GrammarNode;
use super::types::Grammar;

/// Dependency-free markdown-rendering helpers used by [`rust`]'s generated
/// doc comments (table alignment, currently) — isolated so it's the one
/// place to swap in a real markdown crate later, without touching emission
/// logic that has nothing to do with markdown.
mod markdown;
/// The Rust-specific emitter: renders the derivation in this module as a
/// complete `.rs` file containing the generated `Visitor<'tree>` trait.
pub mod rust;
/// Language-agnostic text-shaping helpers (indentation, comment-line
/// prefixing) shared by every target-language emitter, not just [`rust`].
mod text;

/// Returns the ordered set of node-kind names that will actually appear in a
/// parse tree generated from `grammar`.
///
/// A kind is visible when it is either:
/// - an ordinary production name, unless the rule is hidden (leading `_` or
///   `%supertypes` membership, per [`Grammar::is_hidden_rule`]) or listed in
///   `%inline` (per [`Grammar::is_inline_rule`]) — both make the rule's body
///   splice into every call site instead of appearing as its own node; or
/// - an `alias(body, name)` target name, per [`collect_alias_targets`].
///
/// Order follows rule declaration order, with alias targets appended in the
/// order their `alias(…)` occurrences are encountered — this keeps generated
/// output deterministic across runs.
///
/// `pub`, not `pub(crate)`: this is the one piece of the derivation layer
/// worth exposing for introspection on its own — e.g. a dogfood test that
/// checks the kind set against `tree-sitter-bnf/src/node-types.json`
/// without needing to render (or parse back out of) a full `RustVisitor`.
pub fn visible_kinds(grammar: &Grammar) -> IndexSet<String> {
    let mut kinds = IndexSet::new();

    for name in grammar.productions.keys() {
        if grammar.is_hidden_rule(name) || grammar.is_inline_rule(name) {
            continue;
        }
        kinds.insert(name.clone());
    }

    for production in grammar.productions.values() {
        collect_alias_targets(grammar, &production.body, &mut kinds);
    }

    kinds
}

/// Recursively collects visible alias-target names from `node` into `out`.
///
/// Only `Alias(_, NonTerminal(name))` contributes a kind, and only when `name`
/// is not itself hidden (per [`Grammar::is_hidden_rule`]) — the leading-`_`
/// naming convention applies to any symbol name, not just declared rules.
/// `Alias(_, TerminalLiteral(_))` aliases to an anonymous token instead and is
/// skipped, same as any other punctuation.
fn collect_alias_targets(grammar: &Grammar, node: &GrammarNode, out: &mut IndexSet<String>) {
    match node {
        GrammarNode::Alias(body, name) => {
            if let GrammarNode::NonTerminal(n) = name.as_ref()
                && !grammar.is_hidden_rule(n)
            {
                out.insert(n.clone());
            }
            collect_alias_targets(grammar, body, out);
        }
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_alias_targets(grammar, child, out);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => collect_alias_targets(grammar, inner, out),
        GrammarNode::NonTerminal(_)
        | GrammarNode::TerminalLiteral(_)
        | GrammarNode::TerminalPattern(_) => {}
    }
}

/// Returns the `GrammarNode` that determines `kind`'s own visitor shape —
/// leaf status ([`is_leaf_body`]), fields ([`collect_fields`]), and
/// anonymous children ([`collect_anonymous_children`]) are all derived from
/// whatever this function returns for a given kind.
///
/// For an ordinary rule, that's simply the production's own body
/// (`grammar.productions[kind].body`). For a kind that only exists as an
/// `alias(body, name)` target — e.g. `renamed` in
/// `expr -> term => renamed ;`, which has no `renamed` production of its
/// own — it's the aliased `body`, found by scanning every production for
/// the first matching `alias(_, renamed)` occurrence, in declaration order.
/// A kind reused as the alias target at more than one call site (uncommon —
/// an alias target name is usually a one-off rename) takes only the first
/// occurrence's body; this is a scope decision, not a limitation forced by
/// anything structural, and matches [`FieldTargetKinds`]'s "resolve one
/// representative shape per kind" approach rather than unioning across
/// every occurrence.
///
/// Returns `None` only for a kind that isn't in [`visible_kinds`]'s output;
/// every kind [`visible_kinds`] does produce is guaranteed to resolve here,
/// since both use the same alias-target condition
/// (`Alias(_, NonTerminal(n))` with `n` not hidden).
pub(crate) fn body_for_kind<'g>(grammar: &'g Grammar, kind: &str) -> Option<&'g GrammarNode> {
    if let Some(production) = grammar.productions.get(kind) {
        return Some(&production.body);
    }
    grammar
        .productions
        .values()
        .find_map(|production| find_alias_body_for_target(grammar, &production.body, kind))
}

/// Recursive worker for [`body_for_kind`]'s alias-target search.
fn find_alias_body_for_target<'g>(
    grammar: &Grammar,
    node: &'g GrammarNode,
    target: &str,
) -> Option<&'g GrammarNode> {
    match node {
        GrammarNode::Alias(body, name) => {
            if let GrammarNode::NonTerminal(n) = name.as_ref()
                && n == target
                && !grammar.is_hidden_rule(n)
            {
                return Some(body);
            }
            find_alias_body_for_target(grammar, body, target)
        }
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => children
            .iter()
            .find_map(|child| find_alias_body_for_target(grammar, child, target)),
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => find_alias_body_for_target(grammar, inner, target),
        GrammarNode::NonTerminal(_)
        | GrammarNode::TerminalLiteral(_)
        | GrammarNode::TerminalPattern(_) => None,
    }
}

/// Converts a node-kind name (camelCase, mixed, or dot-separated like
/// `prec.dynamic`) to a snake_case Rust method-name fragment.
///
/// A `.` always becomes a word boundary. An uppercase letter starts a new
/// word when the previous character is lowercase or a digit, or when it ends
/// a run of uppercase letters immediately followed by a lowercase one (so
/// `HTMLParser` splits as `html_parser`, not `h_t_m_l_parser`). Already
/// snake_case or single-word lowercase names pass through unchanged.
pub(crate) fn to_snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut result = String::with_capacity(chars.len() + 4);

    for (i, &c) in chars.iter().enumerate() {
        if c == '.' {
            if !result.is_empty() && !result.ends_with('_') {
                result.push('_');
            }
            continue;
        }

        if c.is_uppercase() {
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1);
            let starts_new_word = match prev {
                None => false,
                Some(p) if p.is_lowercase() || p.is_ascii_digit() => true,
                Some(p) => p.is_uppercase() && next.is_some_and(|n| n.is_lowercase()),
            };
            if starts_new_word && !result.ends_with('_') {
                result.push('_');
            }
            result.extend(c.to_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

/// Checks that every kind in `kinds` produces a distinct `visit_*` method
/// name once run through [`to_snake_case`], returning the first collision
/// found as `Err`.
///
/// This is the sole guard against two generated methods silently sharing a
/// name — the emitter never re-checks at codegen time, so a grammar that
/// fails here must be rejected outright rather than producing a Rust file
/// where one kind's `visit_*` method silently shadows another's. There is no
/// equivalent check against the emitter's own fixed helper names
/// (`children_visitor`, `field_visitor`, `error_visitor`, `missing_visitor`,
/// `default_result`, the `visit()` dispatcher): every kind-derived method is
/// exactly `visit_<snake_case(kind)>`, and none of those fixed names start
/// with `visit_`, so a collision with one of them is structurally impossible
/// rather than merely rare — there is nothing to check.
pub(crate) fn check_method_name_collisions(kinds: &IndexSet<String>) -> Result<(), String> {
    let mut seen_by_suffix: IndexMap<String, &str> = IndexMap::new();

    for kind in kinds {
        let suffix = to_snake_case(kind);

        if let Some(&other) = seen_by_suffix.get(&suffix) {
            return Err(format!(
                "kinds '{other}' and '{kind}' both generate method 'visit_{suffix}'; \
                 rename one of the rules to avoid the clash"
            ));
        }

        seen_by_suffix.insert(suffix, kind.as_str());
    }

    Ok(())
}

/// Validates that `grammar` can be safely rendered as a Rust `Visitor`
/// trait — specifically, that no two visible kinds would generate the same
/// `visit_<kind>` method name (see [`check_method_name_collisions`]).
///
/// This is the crate's one `pub` entry point into the collision check:
/// [`visible_kinds`] and [`check_method_name_collisions`] are both
/// `pub(crate)`, internal to how the derivation works, so an external
/// caller — the `visitor` CLI subcommand in `main.rs`, a separate crate —
/// has no other way to ask "is this grammar visitor-safe?" without
/// reaching into derivation internals it has no business depending on.
/// Callers should run this as a pre-flight check before ever constructing
/// a [`rust::RustVisitor`]: rendering an invalid trait and only having it
/// fail later, at `rustc` time in the *user's* own build, would be a much
/// worse experience than failing here with a clear diagnostic.
pub fn check_visitor(grammar: &Grammar) -> Result<(), String> {
    check_method_name_collisions(&visible_kinds(grammar))
}

/// Returns `true` if `body` has no visible children to recurse into: no
/// reference to a visible non-terminal reachable from it.
///
/// A leaf kind's `visit_*` method has nothing structural to walk, so the
/// emitter gives it a `default_result()` body instead of one that calls
/// `children_visitor`.
///
/// References to a hidden ([`Grammar::is_hidden_rule`]) or `%inline`
/// ([`Grammar::is_inline_rule`]) rule are transparent: neither produces a node
/// of its own, so the search continues into the referenced rule's body rather
/// than stopping at the reference. Mutually-recursive hidden/inline rules are
/// cycle-protected: a rule already being resolved contributes nothing further
/// on re-entry, rather than looping forever.
pub(crate) fn is_leaf_body(grammar: &Grammar, body: &GrammarNode) -> bool {
    !references_visible_nonterminal(grammar, body, &mut HashSet::new())
}

/// Recursive worker for [`is_leaf_body`]; `in_progress` holds the hidden/inline
/// rule names currently being resolved, for cycle protection.
fn references_visible_nonterminal(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
) -> bool {
    match node {
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                return true;
            }
            let Some(production) = grammar.productions.get(name) else {
                return false;
            };
            if !in_progress.insert(name.clone()) {
                return false;
            }
            let found = references_visible_nonterminal(grammar, &production.body, in_progress);
            in_progress.remove(name);
            found
        }
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => false,
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => children
            .iter()
            .any(|c| references_visible_nonterminal(grammar, c, in_progress)),
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => {
            references_visible_nonterminal(grammar, inner, in_progress)
        }
        GrammarNode::Alias(body, _) => references_visible_nonterminal(grammar, body, in_progress),
    }
}

/// The set of kinds a field's value may resolve to at runtime.
///
/// Given:
///
/// ```bnf
/// term -> value: (num | str | '(' expr ')') ;
/// ```
///
/// the `value` field's node can be a `num`, a `str`, an `expr` (matched
/// inside the third alternative's parenthesized sequence), or a bare `'('`/
/// `')'` token (matched when the third alternative's own literals are what
/// the field lands on). Resolving `value`'s content node collects:
///
/// - `named = {"num", "str", "expr"}` — every visible kind reachable through
///   the `Choice`'s three arms and the third arm's own `Sequence`; and
/// - `anonymous_token = true` — because `'('` and `')'` are bare literals
///   with no kind name of their own.
///
/// `named` and `anonymous_token` are tracked separately, rather than folding
/// the token case into `named` under some placeholder, because "this field
/// is sometimes a bare token" and "this field is a `foo` node" are different
/// facts a caller needs to tell apart (e.g. to know whether a `match` over
/// named kinds needs a fallback arm).
///
/// `pub`, not `pub(crate)`: exposed alongside [`resolve_field_target_kinds`]
/// for the same introspection reasons [`visible_kinds`] is — e.g. a dogfood
/// test spot-checking a real multi-kind union against
/// `tree-sitter-bnf/src/node-types.json`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FieldTargetKinds {
    /// Visible kind names the field's value may take, in first-encountered order.
    pub named: IndexSet<String>,
    /// `true` when some branch bottoms out at a bare terminal (literal or
    /// pattern) with no kind name of its own.
    pub anonymous_token: bool,
}

/// Resolves the full [`FieldTargetKinds`] a field's content `node` may
/// produce as its value.
///
/// `node` is the field's content — the part after the `label:` in the
/// source, before any wrapping by the field label itself. For:
///
/// ```bnf
/// term -> value: (num | str | '(' expr ')') ;
/// ```
///
/// `node` is the `Choice` node for `(num | str | '(' expr ')')`, and
/// `resolve_field_target_kinds(grammar, node)` returns
/// `FieldTargetKinds { named: {"num", "str", "expr"}, anonymous_token: true }`
/// (see [`FieldTargetKinds`]'s own doc for why each part of that answer is
/// there). This function only sets up the recursion — an empty accumulator
/// and an empty cycle-tracking set — and hands off to
/// [`resolve_field_target_kinds_into`], which does the actual per-variant
/// walk.
pub fn resolve_field_target_kinds(grammar: &Grammar, node: &GrammarNode) -> FieldTargetKinds {
    let mut out = FieldTargetKinds::default();
    resolve_field_target_kinds_into(grammar, node, &mut HashSet::new(), &mut out);
    out
}

/// Recursive worker for [`resolve_field_target_kinds`]; walks `node`,
/// accumulating into `out`. `in_progress` holds the hidden/inline rule names
/// currently being resolved, for cycle protection.
///
/// - [`GrammarNode::NonTerminal`]: an ordinary visible rule reference *is* a
///   produced kind — add its name to `out.named` and stop, without looking at
///   the rule's own body (from the field's point of view, only the kind of
///   the node that comes out matters). A hidden (`_foo`) or `%inline` rule
///   reference produces no node of its own, so resolution steps into *that
///   rule's* body instead and keeps going from there, same as
///   [`is_leaf_body`] — guarded by `in_progress` so a mutually-recursive pair
///   of hidden rules terminates instead of looping forever.
/// - [`GrammarNode::TerminalLiteral`]/[`GrammarNode::TerminalPattern`]: a bare
///   token with no kind name — sets `out.anonymous_token`.
/// - [`GrammarNode::Choice`]/[`GrammarNode::Sequence`]: union in every
///   child's resolution. A field can land on whichever `Choice` alternative
///   matched, or (for a `Sequence`) on any of several children in turn — e.g.
///   in `value: (num | str | '(' expr ')')`, the third alternative is itself
///   a sequence, and `expr` inside it is just as much a possible `value` as
///   `num` or `str` are.
/// - [`GrammarNode::Optional`], [`GrammarNode::ZeroOrMore`],
///   [`GrammarNode::OneOrMore`], [`GrammarNode::Token`],
///   [`GrammarNode::TokenImmediate`], [`GrammarNode::Field`],
///   [`GrammarNode::Prec`], [`GrammarNode::Reserved`]: transparent wrappers
///   that don't change which kind comes out — recurse into the wrapped node.
/// - [`GrammarNode::Alias`]: the alias *fixes* the produced kind, so a named,
///   non-hidden target (`num => literal`) adds that name and never descends
///   into `body` — whatever `num` looks like inside doesn't matter once it's
///   renamed to `literal`. A hidden-named target (`num => _literal`)
///   produces no node either, so resolution falls through to `body` instead,
///   same as a hidden rule reference. A literal target (`num => 'x'`) is an
///   anonymous node — sets `out.anonymous_token`.
fn resolve_field_target_kinds_into(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
    out: &mut FieldTargetKinds,
) {
    match node {
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                out.named.insert(name.clone());
                return;
            }
            if !in_progress.insert(name.clone()) {
                return;
            }
            if let Some(production) = grammar.productions.get(name) {
                resolve_field_target_kinds_into(grammar, &production.body, in_progress, out);
            }
            in_progress.remove(name);
        }
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => {
            out.anonymous_token = true;
        }
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                resolve_field_target_kinds_into(grammar, child, in_progress, out);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => {
            resolve_field_target_kinds_into(grammar, inner, in_progress, out);
        }
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => {
                out.named.insert(n.clone());
            }
            GrammarNode::NonTerminal(_) => {
                resolve_field_target_kinds_into(grammar, body, in_progress, out);
            }
            _ => {
                out.anonymous_token = true;
            }
        },
    }
}

/// Collects the anonymous (unnamed literal/pattern) children that may appear
/// directly under a node built from `body`, for use in that kind's generated
/// doc comment.
///
/// Given:
///
/// ```bnf
/// paren_expr -> '(' expr ')' ;
/// ```
///
/// `paren_expr`'s body is a `Sequence` of three children: the anonymous
/// `'('`, the named `expr` child, and anonymous `')'`. `expr` is its own
/// visible kind with its own `visit_expr` method, so it's excluded here;
/// `collect_anonymous_children(grammar, body)` returns `{"'('", "')'"}`.
///
/// References to a hidden ([`Grammar::is_hidden_rule`]) or `%inline`
/// ([`Grammar::is_inline_rule`]) rule are transparent, same as
/// [`is_leaf_body`] and [`resolve_field_target_kinds`]: neither produces a
/// node of its own, so the walk continues into the referenced rule's body,
/// cycle-protected the same way. A reference to an ordinary *visible* rule,
/// by contrast, stops the walk without contributing anything — that
/// reference is a distinct child node with its own kind (and its own
/// `visit_*` method), not an anonymous child of the node being described
/// here. `Alias` only contributes when its target is a literal (the aliased
/// node becomes that literal's anonymous node) or itself hidden (no node
/// produced, so resolution falls through to the aliased body); an alias to a
/// visible name stops the walk for the same reason a plain visible reference
/// does.
pub(crate) fn collect_anonymous_children(
    grammar: &Grammar,
    body: &GrammarNode,
) -> IndexSet<String> {
    let mut out = IndexSet::new();
    collect_anonymous_children_into(grammar, body, &mut HashSet::new(), &mut out);
    out
}

/// Recursive worker for [`collect_anonymous_children`]; `in_progress` holds
/// the hidden/inline rule names currently being resolved, for cycle protection.
fn collect_anonymous_children_into(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
    out: &mut IndexSet<String>,
) {
    match node {
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                return;
            }
            if !in_progress.insert(name.clone()) {
                return;
            }
            if let Some(production) = grammar.productions.get(name) {
                collect_anonymous_children_into(grammar, &production.body, in_progress, out);
            }
            in_progress.remove(name);
        }
        GrammarNode::TerminalLiteral(text) | GrammarNode::TerminalPattern(text) => {
            out.insert(text.clone());
        }
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_anonymous_children_into(grammar, child, in_progress, out);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => {
            collect_anonymous_children_into(grammar, inner, in_progress, out);
        }
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => {}
            GrammarNode::NonTerminal(_) => {
                collect_anonymous_children_into(grammar, body, in_progress, out);
            }
            GrammarNode::TerminalLiteral(text) | GrammarNode::TerminalPattern(text) => {
                out.insert(text.clone());
            }
            _ => {}
        },
    }
}

/// Collects every field declared directly under a kind's `body`, each
/// mapped to its fully resolved [`FieldTargetKinds`] union.
///
/// Given:
///
/// ```bnf
/// assign -> target: ident '=' value: expr ;
/// ```
///
/// `collect_fields(grammar, body)` on `assign`'s body returns two entries:
/// `"target"` → `{named: {"ident"}, anonymous_token: false}` and `"value"` →
/// `{named: {"expr"}, anonymous_token: false}`.
///
/// The walk is transparent through hidden ([`Grammar::is_hidden_rule`]) and
/// `%inline` ([`Grammar::is_inline_rule`]) rule bodies — same as
/// [`is_leaf_body`] and [`collect_anonymous_children`] — since neither
/// produces a node of its own, so a field declared inside one still belongs
/// to the enclosing kind. A reference to an ordinary *visible* rule stops
/// the walk: any fields inside it belong to *that* rule's own node, not this
/// one. `Alias` is resolved by its name child the same way: a visible-named
/// or literal target is a distinct node (stop); a hidden-named target
/// produces no node, so the walk continues into the aliased body instead.
///
/// A field name repeated at multiple positions within the same body (e.g.
/// `item (',' item)*`, both labeled `field:`) merges into one entry whose
/// target-kind union covers every occurrence — this mirrors how
/// tree-sitter's own `children_by_field_name` enumerates every match under
/// one field name, not just the first.
pub(crate) fn collect_fields(
    grammar: &Grammar,
    body: &GrammarNode,
) -> IndexMap<String, FieldTargetKinds> {
    let mut out = IndexMap::new();
    collect_fields_into(grammar, body, &mut HashSet::new(), &mut out);
    out
}

/// Recursive worker for [`collect_fields`]; `in_progress` holds the
/// hidden/inline rule names currently being resolved, for cycle protection.
fn collect_fields_into(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
    out: &mut IndexMap<String, FieldTargetKinds>,
) {
    match node {
        GrammarNode::Field(name, inner) => {
            let resolved = resolve_field_target_kinds(grammar, inner);
            let entry = out.entry(name.clone()).or_default();
            entry.named.extend(resolved.named);
            entry.anonymous_token |= resolved.anonymous_token;
            // A field's own content could, in principle, contain another
            // field nested inside it; recursing keeps that case correct
            // without needing to special-case it.
            collect_fields_into(grammar, inner, in_progress, out);
        }
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                return;
            }
            if !in_progress.insert(name.clone()) {
                return;
            }
            if let Some(production) = grammar.productions.get(name) {
                collect_fields_into(grammar, &production.body, in_progress, out);
            }
            in_progress.remove(name);
        }
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => {}
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_fields_into(grammar, child, in_progress, out);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Prec(_, _, inner)
        | GrammarNode::Reserved(_, inner) => {
            collect_fields_into(grammar, inner, in_progress, out);
        }
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => {}
            GrammarNode::NonTerminal(_) => {
                collect_fields_into(grammar, body, in_progress, out);
            }
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::{Alias, Field, TerminalLiteral, TerminalPattern};
    use crate::dom::test_utils::{di, nt, p};

    /// An ordinary production with no exclusions is visible.
    #[test]
    fn visible_kinds_includes_ordinary_rule() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'x'".into()))]);
        assert_eq!(visible_kinds(&g), IndexSet::from(["expr".to_string()]));
    }

    /// A leading-underscore rule is hidden and excluded.
    #[test]
    fn visible_kinds_excludes_underscore_prefixed_rule() {
        let g = Grammar::from_rules([
            p("_hidden", TerminalLiteral("'x'".into())),
            p("visible", TerminalLiteral("'y'".into())),
        ]);
        assert_eq!(visible_kinds(&g), IndexSet::from(["visible".to_string()]));
    }

    /// A rule listed in `%supertypes` is hidden and excluded.
    #[test]
    fn visible_kinds_excludes_supertype_rule() {
        let mut g = Grammar::from_rules([
            p("expr", TerminalLiteral("'x'".into())),
            p("other", TerminalLiteral("'y'".into())),
        ]);
        g.supertypes = vec![di("expr", 0)];
        assert_eq!(visible_kinds(&g), IndexSet::from(["other".to_string()]));
    }

    /// A rule listed in `%inline` is excluded even without a leading underscore.
    #[test]
    fn visible_kinds_excludes_inline_rule() {
        let mut g = Grammar::from_rules([
            p("helper", TerminalLiteral("'x'".into())),
            p("main", TerminalLiteral("'y'".into())),
        ]);
        g.inline = vec![di("helper", 0)];
        assert_eq!(visible_kinds(&g), IndexSet::from(["main".to_string()]));
    }

    /// `alias(body, name)` adds the name target as a visible kind.
    #[test]
    fn visible_kinds_includes_alias_name_target() {
        let g = Grammar::from_rules([p(
            "expr",
            Alias(Box::new(nt("term")), Box::new(nt("renamed"))),
        )]);
        assert_eq!(
            visible_kinds(&g),
            IndexSet::from(["expr".to_string(), "renamed".to_string()])
        );
    }

    /// `alias(body, 'literal')` produces an anonymous node, not a visible kind.
    #[test]
    fn visible_kinds_excludes_alias_literal_target() {
        let g = Grammar::from_rules([p(
            "expr",
            Alias(
                Box::new(nt("term")),
                Box::new(TerminalLiteral("'kw'".into())),
            ),
        )]);
        assert_eq!(visible_kinds(&g), IndexSet::from(["expr".to_string()]));
    }

    /// An alias target that is itself hidden (leading `_`) contributes nothing,
    /// matching tree-sitter's naming convention for any symbol, not just rules.
    #[test]
    fn visible_kinds_excludes_alias_target_that_is_itself_hidden() {
        let g = Grammar::from_rules([p(
            "expr",
            Alias(Box::new(nt("term")), Box::new(nt("_hidden_alias"))),
        )]);
        assert_eq!(visible_kinds(&g), IndexSet::from(["expr".to_string()]));
    }

    /// Order follows declaration order, with alias targets appended afterward.
    #[test]
    fn visible_kinds_preserves_declaration_then_alias_order() {
        let g = Grammar::from_rules([
            p("b", TerminalLiteral("'y'".into())),
            p("a", Alias(Box::new(nt("term")), Box::new(nt("aliased")))),
        ]);
        let result = visible_kinds(&g);
        let kinds: Vec<&str> = result.iter().map(String::as_str).collect();
        assert_eq!(kinds, vec!["b", "a", "aliased"]);
    }

    // ── to_snake_case ────────────────────────────────────────────────────

    /// A single lowercase word is unchanged.
    #[test]
    fn to_snake_case_single_word() {
        assert_eq!(to_snake_case("rule"), "rule");
    }

    /// camelCase splits at the lowercase-to-uppercase boundary.
    #[test]
    fn to_snake_case_camel_case() {
        assert_eq!(to_snake_case("nonTerminal"), "non_terminal");
        assert_eq!(to_snake_case("aliasName"), "alias_name");
    }

    /// Already-snake_case names pass through unchanged.
    #[test]
    fn to_snake_case_already_snake_case() {
        assert_eq!(to_snake_case("already_snake"), "already_snake");
    }

    /// A leading underscore (hidden-rule naming) is preserved as-is.
    #[test]
    fn to_snake_case_leading_underscore_preserved() {
        assert_eq!(to_snake_case("_hidden"), "_hidden");
    }

    /// A `.` separator becomes a `_`, as in this grammar's own `prec.dynamic` kind.
    #[test]
    fn to_snake_case_dot_separated() {
        assert_eq!(to_snake_case("prec.dynamic"), "prec_dynamic");
        assert_eq!(to_snake_case("prec.left"), "prec_left");
    }

    /// A run of uppercase letters followed by a lowercase one splits before the
    /// last uppercase letter, treating the run as an acronym.
    #[test]
    fn to_snake_case_acronym_run() {
        assert_eq!(to_snake_case("HTMLParser"), "html_parser");
    }

    /// A digit boundary also starts a new word, same as a lowercase-to-uppercase one.
    #[test]
    fn to_snake_case_digit_boundary() {
        assert_eq!(to_snake_case("rule2Name"), "rule2_name");
    }

    // ── check_method_name_collisions ────────────────────────────────────

    /// Distinct kinds with distinct method names collide with nothing.
    #[test]
    fn collisions_none_for_distinct_kinds() {
        let kinds = IndexSet::from(["expr".to_string(), "nonTerminal".to_string()]);
        assert!(check_method_name_collisions(&kinds).is_ok());
    }

    /// Two kinds whose names snake_case to the same suffix are a hard error
    /// naming both offending kinds.
    #[test]
    fn collisions_detects_kind_kind_clash() {
        let kinds = IndexSet::from(["fooBar".to_string(), "foo_bar".to_string()]);
        let err = check_method_name_collisions(&kinds).unwrap_err();
        assert!(
            err.contains("'fooBar'"),
            "error must name 'fooBar'; got: {err}"
        );
        assert!(
            err.contains("'foo_bar'"),
            "error must name 'foo_bar'; got: {err}"
        );
        assert!(
            err.contains("visit_foo_bar"),
            "error must name the shared method; got: {err}"
        );
    }

    /// A kind whose snake_case name matches a fixed helper's `_visitor`-suffixed
    /// name (e.g. `children_visitor`) is not a collision: kind methods are always
    /// `visit_<kind>`, never bare `<kind>_visitor`.
    #[test]
    fn collisions_kind_matching_helper_suffix_name_is_not_a_clash() {
        let kinds = IndexSet::from(["children_visitor".to_string()]);
        assert!(check_method_name_collisions(&kinds).is_ok());
    }

    // ── check_visitor ────────────────────────────────────────────────────

    /// A grammar with no colliding kind names passes.
    #[test]
    fn check_visitor_ok_for_distinct_kinds() {
        let g = Grammar::from_rules([
            p("fooBar", TerminalLiteral("'x'".into())),
            p("baz", TerminalLiteral("'y'".into())),
        ]);
        assert!(check_visitor(&g).is_ok());
    }

    /// A grammar whose visible kinds would collide fails, end to end
    /// through the one `pub` entry point an external caller (the CLI) has,
    /// not just through the `pub(crate)` pieces it's built from.
    #[test]
    fn check_visitor_detects_kind_kind_clash() {
        let g = Grammar::from_rules([
            p("fooBar", TerminalLiteral("'x'".into())),
            p("foo_bar", TerminalLiteral("'y'".into())),
        ]);
        let err = check_visitor(&g).unwrap_err();
        assert!(err.contains("'fooBar'") && err.contains("'foo_bar'"));
    }

    // ── is_leaf_body ─────────────────────────────────────────────────────

    use crate::dom::GrammarNode::{Choice, Sequence};

    /// A bare terminal has nothing to recurse into: a leaf.
    #[test]
    fn is_leaf_body_true_for_bare_terminal() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        assert!(is_leaf_body(&g, &TerminalLiteral("'x'".into())));
    }

    /// A direct reference to a visible rule is a real child: not a leaf.
    #[test]
    fn is_leaf_body_false_for_visible_nonterminal_reference() {
        let g = Grammar::from_rules([
            p("kind", nt("child")),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        assert!(!is_leaf_body(&g, &nt("child")));
    }

    /// A reference to a hidden (`_`-prefixed) rule whose own body is only
    /// terminals is transparent: still a leaf, since the hidden rule never
    /// produces a node of its own.
    #[test]
    fn is_leaf_body_true_through_hidden_rule_of_only_terminals() {
        let g = Grammar::from_rules([
            p("kind", nt("_digits")),
            p(
                "_digits",
                Choice(vec![
                    TerminalLiteral("'0'".into()),
                    TerminalLiteral("'1'".into()),
                ]),
            ),
        ]);
        assert!(is_leaf_body(&g, &nt("_digits")));
    }

    /// A `%supertypes`-listed rule is transparent the same way a
    /// leading-underscore one is: `is_hidden_rule` treats both the same, so
    /// resolution continues into the supertype rule's own body.
    #[test]
    fn is_leaf_body_true_through_supertype_rule_of_only_terminals() {
        let mut g = Grammar::from_rules([
            p("kind", nt("wrapper")),
            p("wrapper", TerminalLiteral("'x'".into())),
        ]);
        g.supertypes = vec![di("wrapper", 0)];
        assert!(is_leaf_body(&g, &nt("wrapper")));
    }

    /// A reference to a hidden rule that itself references a visible rule is
    /// not a leaf: the hidden wrapper is transparent, so the visible
    /// reference underneath still counts.
    #[test]
    fn is_leaf_body_false_through_hidden_rule_reaching_visible_nonterminal() {
        let g = Grammar::from_rules([
            p("kind", nt("_wrapper")),
            p("_wrapper", Sequence(vec![nt("child")])),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        assert!(!is_leaf_body(&g, &nt("_wrapper")));
    }

    /// `%inline` rules are transparent the same way hidden rules are.
    #[test]
    fn is_leaf_body_transparent_through_inline_rule() {
        let mut g = Grammar::from_rules([
            p("kind", nt("helper")),
            p("helper", nt("child")),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        g.inline = vec![di("helper", 0)];
        assert!(!is_leaf_body(&g, &nt("helper")));
    }

    /// A mutually-recursive pair of hidden rules that never reaches a visible
    /// non-terminal terminates via cycle protection and is a leaf, rather than
    /// looping forever.
    #[test]
    fn is_leaf_body_true_for_hidden_rule_cycle_with_no_visible_reference() {
        let g = Grammar::from_rules([p("kind", nt("_a")), p("_a", nt("_b")), p("_b", nt("_a"))]);
        assert!(is_leaf_body(&g, &nt("_a")));
    }

    /// A mutually-recursive pair of hidden rules where one branch reaches a
    /// visible non-terminal is not a leaf: cycle protection stops the loop
    /// without masking the real reference found before the cycle closes.
    #[test]
    fn is_leaf_body_false_for_hidden_rule_cycle_reaching_visible_nonterminal() {
        let g = Grammar::from_rules([
            p("kind", nt("_a")),
            p("_a", Choice(vec![nt("_b"), nt("child")])),
            p("_b", nt("_a")),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        assert!(!is_leaf_body(&g, &nt("_a")));
    }

    /// An alias node's own name child is a display label, not a rule
    /// reference; leaf status follows the aliased body only.
    #[test]
    fn is_leaf_body_alias_follows_body_not_name() {
        let g = Grammar::from_rules([p(
            "kind",
            Alias(
                Box::new(TerminalLiteral("'x'".into())),
                Box::new(nt("renamed")),
            ),
        )]);
        assert!(is_leaf_body(
            &g,
            &Alias(
                Box::new(TerminalLiteral("'x'".into())),
                Box::new(nt("renamed"))
            )
        ));
    }

    // ── resolve_field_target_kinds ──────────────────────────────────────

    /// A choice of visible rules resolves to the union of their names, with
    /// no anonymous-token branch.
    #[test]
    fn resolve_field_target_kinds_named_only() {
        let g = Grammar::from_rules([
            p("kind", TerminalLiteral("'x'".into())),
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
        ]);
        let result = resolve_field_target_kinds(&g, &Choice(vec![nt("num"), nt("str")]));
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["num".to_string(), "str".to_string()]),
                anonymous_token: false,
            }
        );
    }

    /// A bare terminal contributes no name, only the anonymous-token flag.
    #[test]
    fn resolve_field_target_kinds_bare_terminal_sets_anonymous_token() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let result = resolve_field_target_kinds(&g, &TerminalLiteral("'x'".into()));
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::new(),
                anonymous_token: true,
            }
        );
    }

    /// The doc example: `value: (num | str | '(' expr ')')` resolves to the
    /// three named kinds plus the anonymous-token flag from the parens,
    /// exercising the union across both `Choice` arms and a `Sequence`
    /// nested inside one of them.
    #[test]
    fn resolve_field_target_kinds_mixed_named_and_anonymous() {
        let g = Grammar::from_rules([
            p("term", TerminalLiteral("'x'".into())),
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let content = Choice(vec![
            nt("num"),
            nt("str"),
            Sequence(vec![
                TerminalLiteral("'('".into()),
                nt("expr"),
                TerminalLiteral("')'".into()),
            ]),
        ]);
        let result = resolve_field_target_kinds(&g, &content);
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["num".to_string(), "str".to_string(), "expr".to_string()]),
                anonymous_token: true,
            }
        );
    }

    /// A reference to a hidden rule is transparent: resolution continues
    /// into its body, collecting each visible kind reachable through it —
    /// the small-scale version of this grammar's own 9-way `symbol.content`
    /// case (round 2 finding 3).
    #[test]
    fn resolve_field_target_kinds_transparent_through_hidden_rule() {
        let g = Grammar::from_rules([
            p("kind", nt("_choice")),
            p("_choice", Choice(vec![nt("num"), nt("str"), nt("ident")])),
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
            p("ident", TerminalLiteral("'i'".into())),
        ]);
        let result = resolve_field_target_kinds(&g, &nt("_choice"));
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["num".to_string(), "str".to_string(), "ident".to_string()]),
                anonymous_token: false,
            }
        );
    }

    /// `%inline` rules are transparent the same way hidden rules are.
    #[test]
    fn resolve_field_target_kinds_transparent_through_inline_rule() {
        let mut g = Grammar::from_rules([
            p("kind", nt("helper")),
            p("helper", Choice(vec![nt("num"), nt("str")])),
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
        ]);
        g.inline = vec![di("helper", 0)];
        let result = resolve_field_target_kinds(&g, &nt("helper"));
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["num".to_string(), "str".to_string()]),
                anonymous_token: false,
            }
        );
    }

    /// A mutually-recursive pair of hidden rules that never reaches a
    /// visible non-terminal terminates via cycle protection with an empty
    /// result, rather than looping forever.
    #[test]
    fn resolve_field_target_kinds_cycle_protection_terminates_with_no_result() {
        let g = Grammar::from_rules([p("_a", nt("_b")), p("_b", nt("_a"))]);
        let result = resolve_field_target_kinds(&g, &nt("_a"));
        assert_eq!(result, FieldTargetKinds::default());
    }

    /// A mutually-recursive pair of hidden rules where one branch reaches a
    /// visible kind still finds it: cycle protection stops the loop without
    /// masking a real reference found before the cycle closes.
    #[test]
    fn resolve_field_target_kinds_cycle_protection_still_finds_reachable_kind() {
        let g = Grammar::from_rules([
            p("_a", Choice(vec![nt("_b"), nt("child")])),
            p("_b", nt("_a")),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        let result = resolve_field_target_kinds(&g, &nt("_a"));
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["child".to_string()]),
                anonymous_token: false,
            }
        );
    }

    /// An alias to a visible, non-hidden name fixes the produced kind: only
    /// the alias's name is collected, never the shape of its aliased body.
    #[test]
    fn resolve_field_target_kinds_alias_to_visible_name_uses_name_only() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let node = Alias(
            Box::new(Choice(vec![nt("a"), nt("b")])),
            Box::new(nt("renamed")),
        );
        let result = resolve_field_target_kinds(&g, &node);
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["renamed".to_string()]),
                anonymous_token: false,
            }
        );
    }

    /// An alias to a literal target produces an anonymous node: the
    /// anonymous-token flag is set and the aliased body is not consulted.
    #[test]
    fn resolve_field_target_kinds_alias_to_literal_sets_anonymous_token() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let node = Alias(Box::new(nt("num")), Box::new(TerminalLiteral("'x'".into())));
        let result = resolve_field_target_kinds(&g, &node);
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::new(),
                anonymous_token: true,
            }
        );
    }

    /// An alias whose target name is itself hidden (leading `_`) produces no
    /// node either, so resolution falls through to the aliased body instead
    /// of stopping at the alias.
    #[test]
    fn resolve_field_target_kinds_alias_to_hidden_name_falls_through_to_body() {
        let g = Grammar::from_rules([
            p("kind", TerminalLiteral("'x'".into())),
            p("child", TerminalLiteral("'y'".into())),
        ]);
        let node = Alias(Box::new(nt("child")), Box::new(nt("_hidden_alias")));
        let result = resolve_field_target_kinds(&g, &node);
        assert_eq!(
            result,
            FieldTargetKinds {
                named: IndexSet::from(["child".to_string()]),
                anonymous_token: false,
            }
        );
    }

    // ── collect_anonymous_children ──────────────────────────────────────

    /// The doc example: `paren_expr -> '(' expr ')' ;` collects the two
    /// literal punctuation children and excludes `expr`, which is its own
    /// visible kind with its own `visit_expr` method.
    #[test]
    fn collect_anonymous_children_from_sequence_excludes_visible_child() {
        let g = Grammar::from_rules([
            p("paren_expr", TerminalLiteral("'x'".into())),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let body = Sequence(vec![
            TerminalLiteral("'('".into()),
            nt("expr"),
            TerminalLiteral("')'".into()),
        ]);
        let result = collect_anonymous_children(&g, &body);
        assert_eq!(
            result,
            IndexSet::from(["'('".to_string(), "')'".to_string()])
        );
    }

    /// A choice of bare literals collects every alternative.
    #[test]
    fn collect_anonymous_children_choice_of_literals() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let body = Choice(vec![
            TerminalLiteral("';'".into()),
            TerminalLiteral("'\\n'".into()),
        ]);
        let result = collect_anonymous_children(&g, &body);
        assert_eq!(
            result,
            IndexSet::from(["';'".to_string(), "'\\n'".to_string()])
        );
    }

    /// A regex pattern terminal is collected the same way a literal is.
    #[test]
    fn collect_anonymous_children_pattern_terminal_included() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let body = TerminalPattern("/[0-9]+/".into());
        let result = collect_anonymous_children(&g, &body);
        assert_eq!(result, IndexSet::from(["/[0-9]+/".to_string()]));
    }

    /// A reference to a hidden rule is transparent: the walk continues into
    /// its body and collects the literals reachable through it.
    #[test]
    fn collect_anonymous_children_transparent_through_hidden_rule() {
        let g = Grammar::from_rules([
            p("kind", nt("_terminator")),
            p(
                "_terminator",
                Choice(vec![
                    TerminalLiteral("';'".into()),
                    TerminalLiteral("'\\n'".into()),
                ]),
            ),
        ]);
        let result = collect_anonymous_children(&g, &nt("_terminator"));
        assert_eq!(
            result,
            IndexSet::from(["';'".to_string(), "'\\n'".to_string()])
        );
    }

    /// `%inline` rules are transparent the same way hidden rules are.
    #[test]
    fn collect_anonymous_children_transparent_through_inline_rule() {
        let mut g = Grammar::from_rules([
            p("kind", nt("helper")),
            p("helper", TerminalLiteral("'x'".into())),
        ]);
        g.inline = vec![di("helper", 0)];
        let result = collect_anonymous_children(&g, &nt("helper"));
        assert_eq!(result, IndexSet::from(["'x'".to_string()]));
    }

    /// A reference to an ordinary visible rule stops the walk: that
    /// reference is a distinct child node with its own kind, not an
    /// anonymous child of the node being described.
    #[test]
    fn collect_anonymous_children_stops_at_visible_nonterminal() {
        let g = Grammar::from_rules([
            p("kind", nt("child")),
            p("child", TerminalLiteral("'x'".into())),
        ]);
        let result = collect_anonymous_children(&g, &nt("child"));
        assert!(result.is_empty());
    }

    /// A mutually-recursive pair of hidden rules that never reaches a
    /// literal terminates via cycle protection with an empty result.
    #[test]
    fn collect_anonymous_children_cycle_protection_terminates_with_no_result() {
        let g = Grammar::from_rules([p("_a", nt("_b")), p("_b", nt("_a"))]);
        let result = collect_anonymous_children(&g, &nt("_a"));
        assert!(result.is_empty());
    }

    /// A mutually-recursive pair of hidden rules where one branch reaches a
    /// literal still collects it: cycle protection stops the loop without
    /// masking a real literal found before the cycle closes.
    #[test]
    fn collect_anonymous_children_cycle_protection_still_collects_reachable_literal() {
        let g = Grammar::from_rules([
            p("_a", Choice(vec![nt("_b"), TerminalLiteral("'x'".into())])),
            p("_b", nt("_a")),
        ]);
        let result = collect_anonymous_children(&g, &nt("_a"));
        assert_eq!(result, IndexSet::from(["'x'".to_string()]));
    }

    /// An alias to a visible, non-hidden name contributes nothing: the
    /// aliased node is a distinct named child, not an anonymous one, even
    /// though its own body is full of literals.
    #[test]
    fn collect_anonymous_children_alias_to_visible_name_contributes_nothing() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let node = Alias(
            Box::new(Choice(vec![
                TerminalLiteral("'a'".into()),
                TerminalLiteral("'b'".into()),
            ])),
            Box::new(nt("renamed")),
        );
        let result = collect_anonymous_children(&g, &node);
        assert!(result.is_empty());
    }

    /// An alias to a literal target contributes that literal: the aliased
    /// node becomes that literal's own anonymous node, so the aliased body
    /// is not consulted.
    #[test]
    fn collect_anonymous_children_alias_to_literal_contributes_that_literal() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let node = Alias(Box::new(nt("num")), Box::new(TerminalLiteral("'x'".into())));
        let result = collect_anonymous_children(&g, &node);
        assert_eq!(result, IndexSet::from(["'x'".to_string()]));
    }

    /// An alias whose target name is itself hidden (leading `_`) produces no
    /// node either, so the walk falls through to the aliased body instead of
    /// stopping at the alias.
    #[test]
    fn collect_anonymous_children_alias_to_hidden_name_falls_through_to_body() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let node = Alias(
            Box::new(TerminalLiteral("'y'".into())),
            Box::new(nt("_hidden_alias")),
        );
        let result = collect_anonymous_children(&g, &node);
        assert_eq!(result, IndexSet::from(["'y'".to_string()]));
    }

    // ── collect_fields ───────────────────────────────────────────────────

    /// The doc example: `assign -> target: ident '=' value: expr ;` yields
    /// two fields, each resolved to its single visible target kind.
    #[test]
    fn collect_fields_two_fields_each_a_single_kind() {
        let g = Grammar::from_rules([
            p("ident", TerminalLiteral("'i'".into())),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let body = Sequence(vec![
            Field("target".into(), Box::new(nt("ident"))),
            TerminalLiteral("'='".into()),
            Field("value".into(), Box::new(nt("expr"))),
        ]);
        let result = collect_fields(&g, &body);
        assert_eq!(
            result.get("target").unwrap().named,
            IndexSet::from(["ident".to_string()])
        );
        assert_eq!(
            result.get("value").unwrap().named,
            IndexSet::from(["expr".to_string()])
        );
    }

    /// A field with no counterpart in the body is simply absent from the map.
    #[test]
    fn collect_fields_body_with_no_fields_is_empty() {
        let g = Grammar::from_rules([p("kind", TerminalLiteral("'x'".into()))]);
        let result = collect_fields(&g, &TerminalLiteral("'x'".into()));
        assert!(result.is_empty());
    }

    /// The same field name repeated at two positions in the same body
    /// merges into one entry whose target-kind union covers both.
    #[test]
    fn collect_fields_repeated_field_name_merges_into_one_union() {
        let g = Grammar::from_rules([
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
        ]);
        let body = Sequence(vec![
            Field("item".into(), Box::new(nt("num"))),
            Field("item".into(), Box::new(nt("str"))),
        ]);
        let result = collect_fields(&g, &body);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("item").unwrap().named,
            IndexSet::from(["num".to_string(), "str".to_string()])
        );
    }

    /// A field declared inside a hidden rule's body is transparent: it
    /// still belongs to the enclosing kind, same as a leaf's or anonymous
    /// child's hidden-rule transparency.
    #[test]
    fn collect_fields_transparent_through_hidden_rule() {
        let g = Grammar::from_rules([
            p("kind", nt("_wrapper")),
            p("_wrapper", Field("value".into(), Box::new(nt("expr")))),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let result = collect_fields(&g, &nt("_wrapper"));
        assert_eq!(
            result.get("value").unwrap().named,
            IndexSet::from(["expr".to_string()])
        );
    }

    /// A field declared inside a *visible* referenced rule's body does not
    /// belong to the referencing kind: that reference is a distinct child
    /// node with its own fields, not this one's.
    #[test]
    fn collect_fields_does_not_cross_into_visible_nonterminal_reference() {
        let g = Grammar::from_rules([
            p("outer", nt("inner")),
            p("inner", Field("value".into(), Box::new(nt("expr")))),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let result = collect_fields(&g, &nt("inner"));
        assert!(result.is_empty());
    }

    // ── body_for_kind ────────────────────────────────────────────────────

    /// An ordinary rule's kind resolves to its own production body.
    #[test]
    fn body_for_kind_ordinary_rule_is_its_own_production_body() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'x'".into()))]);
        let body = body_for_kind(&g, "expr").unwrap();
        assert!(matches!(body, TerminalLiteral(s) if s == "'x'"));
    }

    /// The doc example: `expr -> term => renamed ;` has no `renamed`
    /// production, so `renamed`'s body is the alias's own aliased body.
    #[test]
    fn body_for_kind_alias_target_is_the_aliased_body() {
        let g = Grammar::from_rules([p(
            "expr",
            Alias(Box::new(nt("term")), Box::new(nt("renamed"))),
        )]);
        let body = body_for_kind(&g, "renamed").unwrap();
        assert!(matches!(body, crate::dom::GrammarNode::NonTerminal(n) if n == "term"));
    }

    /// A kind that's neither a production name nor any alias target
    /// resolves to nothing.
    #[test]
    fn body_for_kind_unknown_kind_is_none() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'x'".into()))]);
        assert!(body_for_kind(&g, "nope").is_none());
    }

    /// A kind used as an alias target at more than one call site takes only
    /// the first occurrence's body, in declaration order.
    #[test]
    fn body_for_kind_alias_target_reused_takes_first_occurrence() {
        let g = Grammar::from_rules([
            p("a", Alias(Box::new(nt("first")), Box::new(nt("shared")))),
            p("b", Alias(Box::new(nt("second")), Box::new(nt("shared")))),
        ]);
        let body = body_for_kind(&g, "shared").unwrap();
        assert!(matches!(body, crate::dom::GrammarNode::NonTerminal(n) if n == "first"));
    }
}
