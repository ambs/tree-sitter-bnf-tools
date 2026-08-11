use crate::dom::{
    FieldTargetKinds, Grammar, GrammarNode, resolve_field_target_kinds, visible_kinds,
    visitor::{body_for_kind, is_leaf_body},
};
use std::collections::HashSet;

use indexmap::IndexMap;

/// One kind's derived AST-struct shape.
pub struct AstNodeSpec {
    /// The node kind
    pub kind: String,
    /// the list of fields, and their data
    pub fields: IndexMap<String, AstFieldSpec>,
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

/// Collects every field declared directly under a kind's `body`, each
/// mapped to its resolved [`FieldTargetKinds`] union and whether it can
/// occur more than once.
///
/// Reuses [`resolve_field_target_kinds`] for the target-kind part of the
/// answer — same traversal rules as `Visitor`'s own `collect_fields`
/// (transparent through hidden/`%inline` rules, stops at a visible
/// `NonTerminal` reference). The only new question this asks is
/// multiplicity: a field is `multiple: true` when it's reached while inside
/// a [`GrammarNode::ZeroOrMore`]/[`GrammarNode::OneOrMore`], or when the
/// same field name is declared at more than one position in the body (e.g.
/// `item (',' item)*`, both labeled `item:`).
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

            if repeating || already_seen {
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

/// Derives one [`AstNodeSpec`] per visible kind ([`visible_kinds`]) of
/// `grammar`, in the same order, each built from that kind's own body
/// ([`body_for_kind`]): [`is_leaf_body`] decides `is_leaf`, and
/// [`collect_ast_fields`] decides `fields` — computed unconditionally,
/// regardless of leaf status, since a leaf body can still contain
/// `Field`-wrapped terminals; whether an emitter uses that data for a leaf
/// kind is left to the emitter, not decided here.
// Not yet called from outside tests — phase 3 wires this into `--ast-types`.
#[allow(dead_code)]
fn derive_ast_node_specs(grammar: &Grammar) -> IndexMap<String, AstNodeSpec> {
    let kinds = visible_kinds(grammar);

    let mut out = IndexMap::new();

    for kind in kinds {
        let node = body_for_kind(grammar, kind.as_str())
            .expect("every visible kind has a resolvable body");
        let ast_fields = collect_ast_fields(grammar, node);

        let spec = AstNodeSpec {
            kind: kind.clone(),
            fields: ast_fields,
            is_leaf: is_leaf_body(grammar, node),
        };
        out.insert(kind, spec);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::{
        Alias, Field, OneOrMore, Optional, Sequence, TerminalLiteral, ZeroOrMore,
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
