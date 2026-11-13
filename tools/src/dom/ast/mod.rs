use crate::dom::{
    FieldTargetKinds, Grammar, GrammarNode, resolve_field_target_kinds, visible_kinds,
    visitor::{body_for_kind, is_leaf_body},
};
use std::collections::HashSet;

use indexmap::IndexMap;

/// The Rust-specific emitter: renders the derivation in this module as a
/// complete `.rs` file containing the generated AST types.
pub mod rust;

/// `--merge-config` parsing and validation: collapsing several grammar
/// kinds into a shared Rust `enum`, renaming a kind, or explicitly leaving
/// a kind as the default baseline struct.
pub mod merge;

/// One kind's derived AST-struct shape.
pub struct AstNodeSpec {
    /// The node kind
    pub kind: String,
    /// the list of fields, and their data
    pub fields: IndexMap<String, AstFieldSpec>,
    /// What this node can have as *unlabelled* children — the ones no field
    /// label captures — or `None` when it can have none (always `None` for a
    /// leaf kind, whose `_text` already covers all of its own source). The
    /// target is the visible kinds that can occur, plus `anonymous_token` when
    /// a string literal can (a keyword or punctuation, which tree-sitter keeps
    /// as an anonymous child node). Always `multiple`. Derived for every kind,
    /// but only the partial-trees emitter ([`BuildMode::Partial`]) uses it:
    /// strict mode keeps capturing labelled fields only, exactly as before.
    pub children: Option<AstFieldSpec>,
    /// true is node is leaf
    pub is_leaf: bool,
}

/// One field's derived shape within an [`AstNodeSpec`].
#[derive(Default)]
pub struct AstFieldSpec {
    /// Reused as-is from the `Visitor` derivation layer — same meaning.
    pub target: FieldTargetKinds,
    /// `true` if this field's value can occur more than once (`Vec<T>`
    /// in the emitted struct), `false` for exactly one (`T`).
    pub multiple: bool,
}

/// How a generated AST treats a parse tree that contains tree-sitter
/// recovery nodes (`ERROR`/`MISSING`).
///
/// This changes the *shape* of the generated types, not just their runtime
/// behaviour (`Partial` makes fields optional), so it's chosen once at
/// generation time — the `--partial-trees` flag — rather than at the call
/// site. Language-agnostic: any target-language emitter has the same choice
/// to make, so it lives here beside the derivation, not in `rust.rs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildMode {
    /// The first problem aborts the whole build with an error. Today's
    /// behaviour, and the default.
    #[default]
    Strict,
    /// A tree with syntax errors still yields whatever could be built, plus
    /// a list of diagnostics describing what couldn't.
    Partial,
}

impl From<bool> for BuildMode {
    /// Maps the `--partial-trees` flag (`true` ⇒ [`BuildMode::Partial`]).
    fn from(partial_trees: bool) -> Self {
        if partial_trees {
            BuildMode::Partial
        } else {
            BuildMode::Strict
        }
    }
}

/// Collects every field declared directly under a kind's `body`, each
/// mapped to its resolved [`FieldTargetKinds`] union and whether it can
/// occur more than once.
///
/// Reuses [`resolve_field_target_kinds`] for the target-kind part of the
/// answer — same traversal rules as `Visitor`'s own `collect_fields`
/// (transparent through hidden/`%inline` rules, stops at a visible
/// `NonTerminal` reference). The only new question this asks is
/// multiplicity: a field is `multiple: true` when it's reached while inside
/// a [`GrammarNode::ZeroOrMore`]/[`GrammarNode::OneOrMore`] ancestor (e.g.
/// `item (',' item)*`, both labeled `item:`), or when the field's own value
/// contains one (`items: decl*`, i.e. `field('items', repeat(...))` at the
/// tree-sitter level — the far more common way to write a list field, and
/// the opposite nesting from the ancestor case; see
/// [`field_value_is_multiple`]).
fn collect_ast_fields(grammar: &Grammar, node: &GrammarNode) -> IndexMap<String, AstFieldSpec> {
    let mut out = IndexMap::new();
    collect_ast_fields_into(grammar, node, false, &mut HashSet::new(), &mut out);
    out
}

/// Recursive worker for [`collect_ast_fields`]; `repeating` is `true` once
/// the walk has entered a [`GrammarNode::ZeroOrMore`]/[`GrammarNode::OneOrMore`]
/// ancestor, and `in_progress` holds the hidden/inline rule names currently
/// being resolved, for cycle protection.
fn collect_ast_fields_into(
    grammar: &Grammar,
    node: &GrammarNode,
    repeating: bool,
    in_progress: &mut HashSet<String>,
    out: &mut IndexMap<String, AstFieldSpec>,
) {
    match node {
        GrammarNode::Field(name, inner) => {
            let resolved = resolve_field_target_kinds(grammar, inner);

            let already_seen = out.contains_key(name);
            let entry = out.entry(name.clone()).or_default();
            entry.target.named.extend(resolved.named);
            entry.target.anonymous_token |= resolved.anonymous_token;

            if repeating || already_seen || field_value_is_multiple(grammar, inner) {
                entry.multiple = true;
            }

            collect_ast_fields_into(grammar, inner, repeating, in_progress, out);
        }
        GrammarNode::ZeroOrMore(inner) | GrammarNode::OneOrMore(inner) => {
            collect_ast_fields_into(grammar, inner, true, in_progress, out);
        }
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                return;
            }
            if !in_progress.insert(name.clone()) {
                return;
            }
            if let Some(production) = grammar.productions.get(name) {
                collect_ast_fields_into(grammar, &production.body, repeating, in_progress, out);
            }
            in_progress.remove(name);
        }
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => {}
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_ast_fields_into(grammar, child, repeating, in_progress, out);
            }
        }
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => {}
            GrammarNode::NonTerminal(_) => {
                collect_ast_fields_into(grammar, body, repeating, in_progress, out);
            }
            _ => {}
        },
        _ => {
            if let Some(inner) = node.transparent_inner() {
                collect_ast_fields_into(grammar, inner, repeating, in_progress, out);
            }
        }
    }
}

/// Returns `true` if `node` — a field's own value expression — can produce
/// more than one child per match: a [`GrammarNode::ZeroOrMore`]/
/// [`GrammarNode::OneOrMore`] sits directly along the path to its target
/// kind, reached through the same transparent wrappers
/// [`resolve_field_target_kinds`] already treats as "doesn't change what
/// kind comes out" (hidden/`%inline` rule bodies, `Choice`/`Sequence`
/// children, a hidden-named `Alias` target). Catches `items: decl*` —
/// `field('items', repeat(...))` at the tree-sitter level — which
/// [`collect_ast_fields_into`]'s own `repeating` ancestor flag misses,
/// since that only catches the opposite nesting, `(items: decl)*`.
fn field_value_is_multiple(grammar: &Grammar, node: &GrammarNode) -> bool {
    field_value_is_multiple_into(grammar, node, &mut HashSet::new())
}

/// Recursive worker for [`field_value_is_multiple`]; `in_progress` holds the
/// hidden/inline rule names currently being resolved, for cycle protection
/// — a fresh set per top-level call, independent of
/// [`collect_ast_fields_into`]'s own `in_progress`, since this answers a
/// separate question about a single field's value, not the outer field walk.
fn field_value_is_multiple_into(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
) -> bool {
    match node {
        GrammarNode::ZeroOrMore(_) | GrammarNode::OneOrMore(_) => true,
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                return false;
            }
            if !in_progress.insert(name.clone()) {
                return false;
            }
            let result = grammar.productions.get(name).is_some_and(|production| {
                field_value_is_multiple_into(grammar, &production.body, in_progress)
            });
            in_progress.remove(name);
            result
        }
        GrammarNode::TerminalLiteral(_) | GrammarNode::TerminalPattern(_) => false,
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => children
            .iter()
            .any(|child| field_value_is_multiple_into(grammar, child, in_progress)),
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => false,
            GrammarNode::NonTerminal(_) => field_value_is_multiple_into(grammar, body, in_progress),
            _ => false,
        },
        _ => node
            .transparent_inner()
            .is_some_and(|inner| field_value_is_multiple_into(grammar, inner, in_progress)),
    }
}

/// Collects what `node` can have as unlabelled children: the visible kinds
/// reachable without passing through a [`GrammarNode::Field`], in
/// first-encountered order, with `anonymous_token` set when a string literal
/// is reachable the same way.
///
/// The mirror image of [`collect_ast_fields`]: that walk keeps what a field
/// label captures, this one keeps what is left over. Same traversal rules —
/// transparent through hidden/`%inline` rules (cycle-protected) and
/// transparent wrappers, stops at a visible `NonTerminal` (its children are
/// its own), an `alias(body, name)` contributes `name` when it is a visible
/// kind, is transparent through `body` when it is a hidden one, and counts as
/// a literal when it renames to one. Anything under a `Field` is labelled, so
/// it is skipped entirely.
///
/// Only a [`GrammarNode::TerminalLiteral`] counts as a token: tree-sitter
/// keeps a string literal as an anonymous child node, but an inline pattern
/// (`/[0-9]+/`) becomes a hidden auxiliary token that never appears in the
/// tree (checked with `tree-sitter parse --cst`), so there is nothing to
/// capture. A rule that is a single token is a leaf, handled by the caller.
fn collect_unlabelled_children(grammar: &Grammar, node: &GrammarNode) -> FieldTargetKinds {
    let mut out = FieldTargetKinds::default();
    collect_unlabelled_children_into(grammar, node, &mut HashSet::new(), &mut out);
    out
}

/// Recursive worker for [`collect_unlabelled_children`]; `in_progress` holds
/// the hidden/inline rule names currently being resolved, for cycle
/// protection.
fn collect_unlabelled_children_into(
    grammar: &Grammar,
    node: &GrammarNode,
    in_progress: &mut HashSet<String>,
    out: &mut FieldTargetKinds,
) {
    match node {
        GrammarNode::Field(_, _) => {}
        GrammarNode::NonTerminal(name) => {
            if !grammar.is_hidden_rule(name) && !grammar.is_inline_rule(name) {
                out.named.insert(name.clone());
                return;
            }
            if !in_progress.insert(name.clone()) {
                return;
            }
            if let Some(production) = grammar.productions.get(name) {
                collect_unlabelled_children_into(grammar, &production.body, in_progress, out);
            }
            in_progress.remove(name);
        }
        GrammarNode::TerminalLiteral(_) => out.anonymous_token = true,
        GrammarNode::TerminalPattern(_) => {}
        GrammarNode::Sequence(children) | GrammarNode::Choice(children) => {
            for child in children {
                collect_unlabelled_children_into(grammar, child, in_progress, out);
            }
        }
        GrammarNode::Alias(body, name) => match name.as_ref() {
            GrammarNode::NonTerminal(n) if !grammar.is_hidden_rule(n) => {
                out.named.insert(n.clone());
            }
            GrammarNode::NonTerminal(_) => {
                collect_unlabelled_children_into(grammar, body, in_progress, out);
            }
            GrammarNode::TerminalLiteral(_) => out.anonymous_token = true,
            _ => {}
        },
        _ => {
            if let Some(inner) = node.transparent_inner() {
                collect_unlabelled_children_into(grammar, inner, in_progress, out);
            }
        }
    }
}

/// Derives one [`AstNodeSpec`] per visible kind ([`visible_kinds`]) of
/// `grammar`, in the same order, each built from that kind's own body
/// ([`body_for_kind`]): [`is_leaf_body`] decides `is_leaf`, and
/// [`collect_ast_fields`] decides `fields` — computed unconditionally,
/// regardless of leaf status, since a leaf body can still contain
/// `Field`-wrapped terminals; whether an emitter uses that data for a leaf
/// kind is left to the emitter, not decided here.
fn derive_ast_node_specs(grammar: &Grammar) -> IndexMap<String, AstNodeSpec> {
    let kinds = visible_kinds(grammar);

    let mut out = IndexMap::new();

    for kind in kinds {
        let node = body_for_kind(grammar, kind.as_str())
            .expect("every visible kind has a resolvable body");
        let ast_fields = collect_ast_fields(grammar, node);

        // A leaf kind has no children to capture: its `_text` is the whole
        // node, literals included.
        let is_leaf = is_leaf_body(grammar, node);
        let children = if is_leaf {
            None
        } else {
            let target = collect_unlabelled_children(grammar, node);
            if target.named.is_empty() && !target.anonymous_token {
                None
            } else {
                Some(AstFieldSpec {
                    target,
                    multiple: true,
                })
            }
        };

        let spec = AstNodeSpec {
            kind: kind.clone(),
            fields: ast_fields,
            children,
            is_leaf,
        };
        out.insert(kind, spec);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── collect_unlabelled_child_kinds / AstNodeSpec::children ───────────

    /// Collects the unlabelled-child *kinds* of `body` in `g` as a plain
    /// `Vec`, for readable assertions.
    fn unlabelled(g: &Grammar, body: &GrammarNode) -> Vec<String> {
        collect_unlabelled_children(g, body)
            .named
            .into_iter()
            .collect()
    }

    /// Whether a string literal can be an unlabelled child of `body` in `g`.
    fn unlabelled_has_token(g: &Grammar, body: &GrammarNode) -> bool {
        collect_unlabelled_children(g, body).anonymous_token
    }

    /// A repeated reference to a visible kind with no label is an unlabelled
    /// child — the `program -> decl* ;` shape that gives an empty struct today.
    #[test]
    fn unlabelled_children_plain_visible_reference() {
        let g = Grammar::from_rules([p("decl", TerminalLiteral("'d'".into()))]);
        let body = ZeroOrMore(Box::new(nt("decl")));
        assert_eq!(unlabelled(&g, &body), vec!["decl"]);
    }

    /// Everything under a `Field` is labelled, so it is excluded — including
    /// when the field sits under a repetition, and when its value is a hidden
    /// rule holding visible kinds.
    #[test]
    fn unlabelled_children_skip_everything_under_a_field() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'a'".into())),
            p("b", TerminalLiteral("'b'".into())),
            p("_hidden", Choice(vec![nt("a"), nt("b")])),
        ]);
        let body = Sequence(vec![
            Field("x".into(), Box::new(nt("a"))),
            ZeroOrMore(Box::new(Field("y".into(), Box::new(nt("_hidden"))))),
        ]);
        assert!(unlabelled(&g, &body).is_empty());
    }

    /// A kind reached both under a label and without one is still an
    /// unlabelled child: the label only captures its own occurrence.
    #[test]
    fn unlabelled_children_keep_a_kind_also_used_under_a_label() {
        let g = Grammar::from_rules([p("a", TerminalLiteral("'a'".into()))]);
        let body = Sequence(vec![Field("l".into(), Box::new(nt("a"))), nt("a")]);
        assert_eq!(unlabelled(&g, &body), vec!["a"]);
    }

    /// Hidden rules are transparent: their visible kinds are children of the
    /// referencing node, in first-encountered order, without duplicates.
    #[test]
    fn unlabelled_children_transparent_through_hidden_rule() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'a'".into())),
            p("b", TerminalLiteral("'b'".into())),
            p("_either", Choice(vec![nt("b"), nt("a")])),
        ]);
        let body = Sequence(vec![nt("_either"), nt("a")]);
        assert_eq!(unlabelled(&g, &body), vec!["b", "a"]);
    }

    /// A visible kind's own children are its own: the walk stops there.
    #[test]
    fn unlabelled_children_stop_at_a_visible_reference() {
        let g = Grammar::from_rules([
            p("outer", nt("inner")),
            p("inner", nt("deep")),
            p("deep", TerminalLiteral("'d'".into())),
        ]);
        assert_eq!(unlabelled(&g, &nt("inner")), vec!["inner"]);
    }

    /// An alias to a visible kind contributes the alias name (that is the
    /// kind tree-sitter reports), not the aliased rule.
    #[test]
    fn unlabelled_children_alias_contributes_its_target_name() {
        let g = Grammar::from_rules([p("term", TerminalLiteral("'t'".into()))]);
        let body = Alias(Box::new(nt("term")), Box::new(nt("renamed")));
        assert_eq!(unlabelled(&g, &body), vec!["renamed"]);
    }

    /// An alias to a hidden name is transparent: the body's kinds show through.
    #[test]
    fn unlabelled_children_alias_to_hidden_name_is_transparent() {
        let g = Grammar::from_rules([p("term", TerminalLiteral("'t'".into()))]);
        let body = Alias(Box::new(nt("term")), Box::new(nt("_hidden")));
        assert_eq!(unlabelled(&g, &body), vec!["term"]);
    }

    /// A string literal is an anonymous child node, so it counts as a token
    /// child alongside the named kinds.
    #[test]
    fn unlabelled_children_literal_is_a_token_child() {
        let g = Grammar::from_rules([p("a", TerminalLiteral("'a'".into()))]);
        let body = Sequence(vec![TerminalLiteral("'+'".into()), nt("a")]);
        assert_eq!(unlabelled(&g, &body), vec!["a"]);
        assert!(unlabelled_has_token(&g, &body));
    }

    /// An inline pattern is a hidden auxiliary token that never appears in
    /// the tree (checked with `tree-sitter parse --cst`), so there is nothing
    /// to capture and it is not a token child.
    #[test]
    fn unlabelled_children_inline_pattern_is_not_a_child() {
        let g = Grammar::from_rules([]);
        let body = Sequence(vec![TerminalPattern("/[a-z]+/".into())]);
        assert!(unlabelled(&g, &body).is_empty());
        assert!(!unlabelled_has_token(&g, &body));
    }

    /// A literal under a field is labelled, so it is not an unlabelled child.
    #[test]
    fn unlabelled_children_literal_under_a_field_is_labelled() {
        let g = Grammar::from_rules([]);
        let body = Field("op".into(), Box::new(TerminalLiteral("'+'".into())));
        assert!(!unlabelled_has_token(&g, &body));
    }

    /// Hidden rules are transparent for literals too.
    #[test]
    fn unlabelled_children_literal_through_hidden_rule() {
        let g = Grammar::from_rules([p("_semi", TerminalLiteral("';'".into()))]);
        assert!(unlabelled_has_token(&g, &nt("_semi")));
    }

    /// An alias that renames to a literal produces an anonymous node, so it
    /// is a token child — and not a named one.
    #[test]
    fn unlabelled_children_alias_to_a_literal_is_a_token_child() {
        let g = Grammar::from_rules([p("term", TerminalLiteral("'t'".into()))]);
        let body = Alias(
            Box::new(nt("term")),
            Box::new(TerminalLiteral("'x'".into())),
        );
        assert!(unlabelled(&g, &body).is_empty());
        assert!(unlabelled_has_token(&g, &body));
    }

    /// An alias whose target is neither a name nor a literal — not something
    /// the parser produces, but a tree can be built by hand — names no node
    /// tree-sitter would report, so it contributes no child of either kind.
    #[test]
    fn unlabelled_children_alias_to_neither_name_nor_literal_contributes_nothing() {
        let g = Grammar::from_rules([p("term", TerminalLiteral("'t'".into()))]);
        let body = Alias(
            Box::new(nt("term")),
            Box::new(TerminalPattern("/x/".into())),
        );
        assert!(unlabelled(&g, &body).is_empty());
        assert!(!unlabelled_has_token(&g, &body));
    }

    /// Mutually recursive hidden rules terminate rather than looping.
    #[test]
    fn unlabelled_children_cycle_protection_terminates() {
        let g = Grammar::from_rules([p("_a", nt("_b")), p("_b", nt("_a"))]);
        assert!(unlabelled(&g, &nt("_a")).is_empty());
    }

    /// The derived spec carries unlabelled children as a `multiple` target —
    /// and `None` for a leaf or a fully labelled kind.
    #[test]
    fn derived_spec_children_is_multiple_named_only_and_absent_when_empty() {
        let g = Grammar::from_rules([
            p("program", ZeroOrMore(Box::new(nt("decl")))),
            p("decl", Field("name".into(), Box::new(nt("ident")))),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let specs = derive_ast_node_specs(&g);

        let children = specs["program"]
            .children
            .as_ref()
            .expect("program has children");
        assert!(children.multiple);
        assert!(!children.target.anonymous_token, "no literal in this body");
        assert_eq!(
            children.target.named.iter().cloned().collect::<Vec<_>>(),
            vec!["decl"]
        );

        assert!(specs["decl"].children.is_none(), "fully labelled");
        assert!(specs["ident"].children.is_none(), "leaf");
    }

    /// A non-leaf kind's literals are part of its children: alongside named
    /// kinds, or on their own when a label captures everything named.
    #[test]
    fn derived_spec_children_include_literals_of_a_non_leaf_kind() {
        let g = Grammar::from_rules([
            p(
                "decl",
                Sequence(vec![TerminalLiteral("'var'".into()), nt("ident")]),
            ),
            p(
                "call",
                Sequence(vec![
                    Field("name".into(), Box::new(nt("ident"))),
                    TerminalLiteral("'('".into()),
                    TerminalLiteral("')'".into()),
                ]),
            ),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let specs = derive_ast_node_specs(&g);

        let decl = specs["decl"].children.as_ref().expect("decl has children");
        assert_eq!(
            decl.target.named.iter().cloned().collect::<Vec<_>>(),
            vec!["ident"]
        );
        assert!(decl.target.anonymous_token);

        let call = specs["call"].children.as_ref().expect("call has literals");
        assert!(call.target.named.is_empty());
        assert!(call.target.anonymous_token);
    }

    /// A leaf kind has no children even when its body is literals: `_text` is
    /// the whole node already (`op -> '+' | '-' ;` is a node whose child is
    /// the anonymous `+`, and its text is that `+`).
    #[test]
    fn derived_spec_leaf_kind_has_no_children_even_with_literals() {
        let g = Grammar::from_rules([p(
            "op",
            Choice(vec![
                TerminalLiteral("'+'".into()),
                TerminalLiteral("'-'".into()),
            ]),
        )]);
        let specs = derive_ast_node_specs(&g);
        assert!(specs["op"].is_leaf);
        assert!(specs["op"].children.is_none());
    }

    // ── BuildMode ───────────────────────────────────────────────────────

    /// The `--partial-trees` flag maps `true` to partial and `false` to
    /// strict, and strict is the default.
    #[test]
    fn build_mode_from_flag_and_default() {
        assert_eq!(BuildMode::from(true), BuildMode::Partial);
        assert_eq!(BuildMode::from(false), BuildMode::Strict);
        assert_eq!(BuildMode::default(), BuildMode::Strict);
    }
    use crate::dom::GrammarNode::{
        Alias, Choice, Field, OneOrMore, Optional, Sequence, TerminalLiteral, TerminalPattern,
        ZeroOrMore,
    };
    use crate::dom::test_utils::{nt, p};
    use crate::dom::visitor::collect_fields;
    use indexmap::IndexSet;

    // ── collect_ast_fields ──────────────────────────────────────────────

    /// A field wrapped in `*` (zero or more) is multiple.
    #[test]
    fn collect_ast_fields_field_under_zero_or_more_is_multiple() {
        let g = Grammar::from_rules([p("stmt", TerminalLiteral("'s'".into()))]);
        let body = ZeroOrMore(Box::new(Field("items".into(), Box::new(nt("stmt")))));
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// A field wrapped in `+` (one or more) is multiple, same as `*`.
    #[test]
    fn collect_ast_fields_field_under_one_or_more_is_multiple() {
        let g = Grammar::from_rules([p("stmt", TerminalLiteral("'s'".into()))]);
        let body = OneOrMore(Box::new(Field("items".into(), Box::new(nt("stmt")))));
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// The same field name declared at two positions in one body is
    /// multiple, even with no `*`/`+` involved at all.
    #[test]
    fn collect_ast_fields_repeated_field_name_without_repetition_is_multiple() {
        let g = Grammar::from_rules([
            p("num", TerminalLiteral("'0'".into())),
            p("str", TerminalLiteral("'\"\"'".into())),
        ]);
        let body = Sequence(vec![
            Field("item".into(), Box::new(nt("num"))),
            Field("item".into(), Box::new(nt("str"))),
        ]);
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("item").unwrap().multiple);
    }

    /// A field declared exactly once, outside any repetition, is not multiple.
    #[test]
    fn collect_ast_fields_single_occurrence_field_is_not_multiple() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'e'".into()))]);
        let body = Field("value".into(), Box::new(nt("expr")));
        let result = collect_ast_fields(&g, &body);
        assert!(!result.get("value").unwrap().multiple);
    }

    /// A field *wrapping* `*` (`items: decl*`, i.e. `field('items',
    /// repeat(...))` at the tree-sitter level) is multiple — the opposite
    /// nesting from `collect_ast_fields_field_under_zero_or_more_is_multiple`
    /// above, and the one the `repeating` ancestor flag alone misses (see
    /// [`field_value_is_multiple`]).
    #[test]
    fn collect_ast_fields_field_wrapping_zero_or_more_is_multiple() {
        let g = Grammar::from_rules([p("stmt", TerminalLiteral("'s'".into()))]);
        let body = Field("items".into(), Box::new(ZeroOrMore(Box::new(nt("stmt")))));
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// Same as above for `+`.
    #[test]
    fn collect_ast_fields_field_wrapping_one_or_more_is_multiple() {
        let g = Grammar::from_rules([p("stmt", TerminalLiteral("'s'".into()))]);
        let body = Field("items".into(), Box::new(OneOrMore(Box::new(nt("stmt")))));
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// A field wrapping a choice where only one alternative repeats is still
    /// multiple: `field_value_is_multiple` unions across `Choice`/`Sequence`
    /// children rather than requiring every alternative to repeat.
    #[test]
    fn collect_ast_fields_field_wrapping_choice_with_one_repeating_alternative_is_multiple() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'a'".into())),
            p("b", TerminalLiteral("'b'".into())),
        ]);
        let body = Field(
            "items".into(),
            Box::new(crate::dom::GrammarNode::Choice(vec![
                ZeroOrMore(Box::new(nt("a"))),
                nt("b"),
            ])),
        );
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// A field wrapping a reference to a hidden rule whose own body repeats
    /// is multiple: the hidden-rule indirection is transparent, same as
    /// target-kind resolution treats it.
    #[test]
    fn collect_ast_fields_field_wrapping_hidden_rule_that_repeats_is_multiple() {
        let g = Grammar::from_rules([
            p("_group", ZeroOrMore(Box::new(nt("stmt")))),
            p("stmt", TerminalLiteral("'s'".into())),
        ]);
        let body = Field("items".into(), Box::new(nt("_group")));
        let result = collect_ast_fields(&g, &body);
        assert!(result.get("items").unwrap().multiple);
    }

    /// A mutually-recursive pair of hidden rules that never reaches a repeat
    /// terminates `field_value_is_multiple` via cycle protection, returning
    /// `false` (not multiple) rather than looping forever.
    #[test]
    fn collect_ast_fields_field_multiplicity_cycle_protection_terminates() {
        let g = Grammar::from_rules([p("_a", nt("_b")), p("_b", nt("_a"))]);
        let body = Field("items".into(), Box::new(nt("_a")));
        let result = collect_ast_fields(&g, &body);
        assert!(!result.get("items").unwrap().multiple);
    }

    /// A field declared inside a hidden rule's body is transparent: it
    /// still belongs to the enclosing kind, exercising the `NonTerminal`
    /// arm's hidden-rule traversal (not just its early-return guard).
    #[test]
    fn collect_ast_fields_transparent_through_hidden_rule() {
        let g = Grammar::from_rules([
            p("kind", nt("_wrapper")),
            p("_wrapper", Field("value".into(), Box::new(nt("expr")))),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let result = collect_ast_fields(&g, &nt("_wrapper"));
        assert_eq!(
            result.get("value").unwrap().target.named,
            IndexSet::from(["expr".to_string()])
        );
    }

    /// A mutually-recursive pair of hidden rules that never reaches a field
    /// terminates via cycle protection with an empty result, rather than
    /// looping forever.
    #[test]
    fn collect_ast_fields_cycle_protection_terminates_with_no_result() {
        let g = Grammar::from_rules([p("_a", nt("_b")), p("_b", nt("_a"))]);
        let result = collect_ast_fields(&g, &nt("_a"));
        assert!(result.is_empty());
    }

    /// `optional(…)` is a transparent wrapper, same as every other
    /// annotation: a field nested inside it still belongs to the enclosing
    /// kind, via the generic `transparent_inner` fallback arm.
    #[test]
    fn collect_ast_fields_transparent_through_optional() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'e'".into()))]);
        let node = Optional(Box::new(Field("value".into(), Box::new(nt("expr")))));
        let result = collect_ast_fields(&g, &node);
        assert_eq!(
            result.get("value").unwrap().target.named,
            IndexSet::from(["expr".to_string()])
        );
    }

    /// An `Alias` whose name target is a *visible* rule is a distinct node:
    /// the aliased body's fields are not collected.
    #[test]
    fn collect_ast_fields_alias_to_visible_name_contributes_no_field() {
        let g = Grammar::from_rules([
            p("expr", TerminalLiteral("'e'".into())),
            p("renamed", TerminalLiteral("'r'".into())),
        ]);
        let node = Alias(
            Box::new(Field("value".into(), Box::new(nt("expr")))),
            Box::new(nt("renamed")),
        );
        let result = collect_ast_fields(&g, &node);
        assert!(result.is_empty());
    }

    /// An `Alias` whose name target is itself hidden produces no node of its
    /// own, so field collection falls through to the aliased body instead.
    #[test]
    fn collect_ast_fields_alias_to_hidden_name_falls_through_to_body() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'e'".into()))]);
        let node = Alias(
            Box::new(Field("value".into(), Box::new(nt("expr")))),
            Box::new(nt("_hidden_alias")),
        );
        let result = collect_ast_fields(&g, &node);
        assert_eq!(
            result.get("value").unwrap().target.named,
            IndexSet::from(["expr".to_string()])
        );
    }

    /// An `Alias` to a literal target produces an anonymous node with no
    /// fields of its own: the aliased body's fields are not collected.
    #[test]
    fn collect_ast_fields_alias_to_literal_contributes_no_field() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'e'".into()))]);
        let node = Alias(
            Box::new(Field("value".into(), Box::new(nt("expr")))),
            Box::new(TerminalLiteral("'x'".into())),
        );
        let result = collect_ast_fields(&g, &node);
        assert!(result.is_empty());
    }

    // ── derive_ast_node_specs ────────────────────────────────────────────

    /// A non-leaf kind's derived fields resolve the same target kinds as
    /// `Visitor`'s own `collect_fields` — this only checks that
    /// `collect_ast_fields` reuses [`resolve_field_target_kinds`] correctly,
    /// not `collect_fields`'s own correctness (covered by its own tests).
    #[test]
    fn derive_ast_node_specs_non_leaf_kind_fields_match_collect_fields() {
        let g = Grammar::from_rules([
            p(
                "assign",
                Sequence(vec![
                    Field("target".into(), Box::new(nt("ident"))),
                    TerminalLiteral("'='".into()),
                    Field("value".into(), Box::new(nt("expr"))),
                ]),
            ),
            p("ident", TerminalLiteral("'i'".into())),
            p("expr", TerminalLiteral("'e'".into())),
        ]);
        let specs = derive_ast_node_specs(&g);
        let assign = &specs.get("assign").unwrap().fields;
        let expected = collect_fields(&g, &g.productions.get("assign").unwrap().body);
        assert_eq!(
            assign.get("target").unwrap().target.named,
            expected.get("target").unwrap().named
        );
        assert_eq!(
            assign.get("value").unwrap().target.named,
            expected.get("value").unwrap().named
        );
    }

    /// A leaf kind's body has no `Field` nodes to find, so it derives to an
    /// empty field map.
    #[test]
    fn derive_ast_node_specs_leaf_kind_has_no_fields() {
        let g = Grammar::from_rules([p("num", TerminalLiteral("'0'".into()))]);
        let specs = derive_ast_node_specs(&g);
        let num = specs.get("num").unwrap();
        assert!(num.is_leaf);
        assert!(num.fields.is_empty());
    }

    /// A hidden rule is not in `visible_kinds`, so it never appears as a key
    /// in the derived map.
    #[test]
    fn derive_ast_node_specs_excludes_hidden_kind() {
        let g = Grammar::from_rules([
            p("_hidden", TerminalLiteral("'x'".into())),
            p("visible", TerminalLiteral("'y'".into())),
        ]);
        let specs = derive_ast_node_specs(&g);
        assert!(!specs.contains_key("_hidden"));
        assert!(specs.contains_key("visible"));
    }
}
