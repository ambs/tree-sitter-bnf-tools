use crate::dom::NameOrLiteral;

use super::nodes::GrammarNode;
use super::types::Grammar;

/// Renames rule `old` to `new` throughout `grammar`: the definition key and name field,
/// all RHS `NonTerminal` references in every rule body, every directive list that can
/// reference rule names, and the `rhs_nonterminals` cache.
///
/// Returns `Err` if `old` is not defined or `new` is already defined.
pub fn rename_grammar(grammar: &mut Grammar, old: &str, new: &str) -> Result<(), String> {
    if !grammar.productions.contains_key(old) {
        return Err(format!("rule '{old}' is not defined"));
    }
    if grammar.productions.contains_key(new) {
        return Err(format!("rule '{new}' is already defined"));
    }

    rename_production_key(grammar, old, new);

    for production in grammar.productions.values_mut() {
        rename_node(&mut production.body, old, new);
    }

    rename_directives(grammar, old, new);

    if grammar.rhs_nonterminals.remove(old) {
        grammar.rhs_nonterminals.insert(new.to_owned());
    }

    Ok(())
}

/// Renames `old` to `new` in every directive list that holds plain rule names
/// (`%axiom`, `%inline`, `%supertypes`, `%extras`, `%conflicts`).
fn rename_directives(grammar: &mut Grammar, old: &str, new: &str) {
    if let Some(item) = grammar.axiom_directive_mut()
        && item.name == old
    {
        item.name = new.to_owned();
    }
    if let Some(word) = &mut grammar.word
        && word.name == old
    {
        word.name = new.to_owned();
    }
    for item in grammar
        .inline
        .iter_mut()
        .chain(grammar.supertypes.iter_mut())
        .chain(grammar.extras.iter_mut())
    {
        if item.name == old {
            item.name = new.to_owned();
        }
    }
    for group in grammar.conflicts.iter_mut() {
        for rule in group.rules.iter_mut() {
            if rule == old {
                *rule = new.to_owned();
            }
        }
    }
    for group in grammar.precedences.iter_mut() {
        for item in group.items.iter_mut() {
            if let NameOrLiteral::Name(name) = item
                && name == old
            {
                *name = new.to_owned();
            }
        }
    }
    for entry in grammar.reserved_sets.iter_mut() {
        for item in entry.rule_names.iter_mut() {
            if let NameOrLiteral::Name(name) = item
                && name == old
            {
                *name = new.to_owned();
            }
        }
    }
}

/// Rebuilds `grammar.productions`, replacing the key and `Production::name` for the renamed rule.
///
/// Draining and re-collecting preserves insertion order.
fn rename_production_key(grammar: &mut Grammar, old: &str, new: &str) {
    let updated = grammar
        .productions
        .drain(..)
        .map(|(k, mut p)| {
            if k == old {
                p.name = new.to_owned();
                (new.to_owned(), p)
            } else {
                (k, p)
            }
        })
        .collect();
    grammar.productions = updated;
}

/// Recursively renames every `NonTerminal(old)` to `NonTerminal(new)` in `node`.
fn rename_node(node: &mut GrammarNode, old: &str, new: &str) {
    match node {
        GrammarNode::NonTerminal(name) if name == old => *name = new.to_owned(),
        GrammarNode::Sequence(items) | GrammarNode::Choice(items) => {
            for item in items {
                rename_node(item, old, new);
            }
        }
        GrammarNode::Optional(inner)
        | GrammarNode::ZeroOrMore(inner)
        | GrammarNode::OneOrMore(inner)
        | GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner) => rename_node(inner, old, new),
        GrammarNode::Alias(body, name_node) => {
            rename_node(body, old, new);
            rename_node(name_node, old, new);
        }
        GrammarNode::Prec(_, _, inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Reserved(_, inner) => rename_node(inner, old, new),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Grammar;
    use crate::dom::test_utils::{nt, p};

    /// Renaming a rule updates both the `productions` map key and `Production::name`.
    #[test]
    fn renames_production_key_and_name() {
        let mut g = Grammar::from_rules([p("expr", nt("term"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert!(g.productions.contains_key("expression"));
        assert!(!g.productions.contains_key("expr"));
        assert_eq!(g.productions["expression"].name, "expression");
        assert!(
            matches!(&g.productions["expression"].body, GrammarNode::NonTerminal(n) if n == "term"),
            "body must be preserved unchanged"
        );
    }

    /// A `NonTerminal` in the body (first argument) of an `Alias` node is updated.
    #[test]
    fn renames_nonterminal_in_alias_body() {
        use crate::dom::GrammarNode::Alias;
        let body = Alias(Box::new(nt("expr")), Box::new(nt("other")));
        let mut g = Grammar::from_rules([p("root", body), p("expr", nt("x")), p("other", nt("y"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        if let Alias(inner_body, _) = &g.productions["root"].body {
            assert!(
                matches!(inner_body.as_ref(), GrammarNode::NonTerminal(n) if n == "expression")
            );
        } else {
            panic!("expected Alias");
        }
    }

    /// A `NonTerminal` in the alias name (second argument) of an `Alias` node is updated.
    #[test]
    fn renames_nonterminal_in_alias_name() {
        use crate::dom::GrammarNode::Alias;
        let body = Alias(Box::new(nt("other")), Box::new(nt("expr")));
        let mut g = Grammar::from_rules([p("root", body), p("expr", nt("x")), p("other", nt("y"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        if let Alias(_, alias_name) = &g.productions["root"].body {
            assert!(
                matches!(alias_name.as_ref(), GrammarNode::NonTerminal(n) if n == "expression")
            );
        } else {
            panic!("expected Alias");
        }
    }

    /// Declaration order of all rules is preserved after a rename.
    #[test]
    fn preserves_declaration_order_after_rename() {
        let mut g = Grammar::from_rules([p("a", nt("x")), p("b", nt("y")), p("c", nt("z"))]);
        rename_grammar(&mut g, "b", "beta").unwrap();
        let keys: Vec<&str> = g.productions.keys().map(String::as_str).collect();
        assert_eq!(keys, ["a", "beta", "c"]);
    }

    /// An error is returned when the target rule name is already defined in the grammar.
    #[test]
    fn error_when_new_already_defined() {
        let mut g = Grammar::from_rules([p("expr", nt("x")), p("term", nt("y"))]);
        let err = rename_grammar(&mut g, "expr", "term").unwrap_err();
        assert!(
            err.contains("'term'"),
            "error must name the conflicting rule; got: {err}"
        );
    }

    /// An error is returned when the source rule name is not defined in the grammar.
    #[test]
    fn error_when_old_not_defined() {
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        let err = rename_grammar(&mut g, "unknown", "foo").unwrap_err();
        assert!(
            err.contains("'unknown'"),
            "error must name the missing rule; got: {err}"
        );
    }

    /// The `rhs_nonterminals` cache is updated to reflect the rename.
    #[test]
    fn updates_rhs_nonterminals_cache() {
        let mut g = Grammar::from_rules([p("root", nt("expr")), p("expr", nt("x"))]);
        g.rhs_nonterminals.insert("expr".to_owned());
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert!(
            g.rhs_nonterminals.contains("expression"),
            "new name must be in cache"
        );
        assert!(
            !g.rhs_nonterminals.contains("expr"),
            "old name must be removed from cache"
        );
    }

    /// Entries in a `%conflicts` group are updated when they reference the renamed rule.
    #[test]
    fn renames_conflicts_directive() {
        use crate::dom::test_utils::cg;
        let mut g = Grammar::from_rules([p("expr", nt("x")), p("stmt", nt("y"))]);
        g.conflicts = vec![cg(&["expr", "stmt"], 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(g.conflicts[0].rules, vec!["expression", "stmt"]);
    }

    /// The `%extras` directive is updated when it references the renamed rule.
    #[test]
    fn renames_extras_directive() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("comment", nt("x"))]);
        g.extras = vec![di("comment", 1)];
        rename_grammar(&mut g, "comment", "line_comment").unwrap();
        assert_eq!(g.extras[0].name, "line_comment");
    }

    /// The `%supertypes` directive is updated when it references the renamed rule.
    #[test]
    fn renames_supertypes_directive() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.supertypes = vec![di("expr", 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(g.supertypes[0].name, "expression");
    }

    /// The `%inline` directive is updated when it references the renamed rule.
    #[test]
    fn renames_inline_directive() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.inline = vec![di("expr", 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(g.inline[0].name, "expression");
    }

    /// The `%axiom` directive is updated when it references the renamed rule.
    #[test]
    fn renames_axiom_directive() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.declare_axiom(di("expr", 1));
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(g.axiom_directive().unwrap().name, "expression");
    }

    /// A `NonTerminal` reference to the renamed rule inside a `Sequence` body is updated.
    #[test]
    fn renames_nonterminal_inside_sequence() {
        use crate::dom::GrammarNode::Sequence;
        let body = Sequence(vec![nt("a"), nt("expr")]);
        let mut g = Grammar::from_rules([p("root", body), p("a", nt("x")), p("expr", nt("y"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        if let Sequence(items) = &g.productions["root"].body {
            assert!(
                matches!(&items[0], GrammarNode::NonTerminal(n) if n == "a"),
                "first item must be unchanged"
            );
            assert!(matches!(&items[1], GrammarNode::NonTerminal(n) if n == "expression"));
        } else {
            panic!("expected Sequence");
        }
    }

    /// A `NonTerminal` reference to the renamed rule inside a `Choice` body is updated.
    #[test]
    fn renames_nonterminal_inside_choice() {
        use crate::dom::GrammarNode::Choice;
        let body = Choice(vec![nt("expr"), nt("term")]);
        let mut g = Grammar::from_rules([p("root", body), p("expr", nt("x")), p("term", nt("y"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        if let Choice(alts) = &g.productions["root"].body {
            assert!(matches!(&alts[0], GrammarNode::NonTerminal(n) if n == "expression"));
            assert!(
                matches!(&alts[1], GrammarNode::NonTerminal(n) if n == "term"),
                "other branch must be unchanged"
            );
        } else {
            panic!("expected Choice");
        }
    }

    /// The `%word` directive is updated when it references the renamed rule.
    #[test]
    fn renames_word_directive() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("ident", nt("x"))]);
        g.declare_word(di("ident", 1));
        rename_grammar(&mut g, "ident", "identifier").unwrap();
        assert_eq!(g.word.as_ref().unwrap().name, "identifier");
    }

    /// The `%word` directive is not changed when it names a different rule.
    #[test]
    fn word_directive_unchanged_when_other_rule_renamed() {
        use crate::dom::test_utils::di;
        let mut g = Grammar::from_rules([p("ident", nt("x")), p("other", nt("x"))]);
        g.declare_word(di("ident", 1));
        rename_grammar(&mut g, "other", "another").unwrap();
        assert_eq!(g.word.as_ref().unwrap().name, "ident");
    }

    /// A `NonTerminal` reference to the renamed rule in another rule's body is updated.
    #[test]
    fn renames_rhs_reference_in_other_rule() {
        let mut g = Grammar::from_rules([p("root", nt("expr")), p("expr", nt("x"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert!(
            g.productions.contains_key("expression"),
            "renamed rule must exist"
        );
        assert!(
            matches!(&g.productions["root"].body, GrammarNode::NonTerminal(n) if n == "expression"),
            "root's body must reference the new name"
        );
    }

    /// A `Name` item in a `%precedences` group is updated when the referenced rule is renamed.
    #[test]
    fn renames_precedences_name_item() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("expr", nt("x")), p("term", nt("y"))]);
        g.precedences = vec![pg(
            &[
                NameOrLiteral::Name("expr".into()),
                NameOrLiteral::Name("term".into()),
            ],
            1,
        )];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(
            g.precedences[0].items[0],
            NameOrLiteral::Name("expression".into())
        );
        assert_eq!(
            g.precedences[0].items[1],
            NameOrLiteral::Name("term".into())
        );
    }

    /// A `Literal` item in a `%precedences` group is never modified by a rename.
    #[test]
    fn renames_precedences_literal_item_untouched() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::pg;
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.precedences = vec![pg(&[NameOrLiteral::Literal("'expr'".into())], 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(
            g.precedences[0].items[0],
            NameOrLiteral::Literal("'expr'".into())
        );
    }

    /// Renaming a rule referenced in a `%reserved` entry's `rule_names` (as a `Name`)
    /// updates that entry in place.
    #[test]
    fn renames_reserved_set_name_item() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("expr", nt("x")), p("term", nt("y"))]);
        g.reserved_sets = vec![re(
            "kw",
            &[
                NameOrLiteral::Name("expr".into()),
                NameOrLiteral::Name("term".into()),
            ],
            1,
        )];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(
            g.reserved_sets[0].rule_names[0],
            NameOrLiteral::Name("expression".into())
        );
        assert_eq!(
            g.reserved_sets[0].rule_names[1],
            NameOrLiteral::Name("term".into())
        );
    }

    /// A `Literal` item in a `%reserved` entry's `rule_names` is never modified by a rename.
    #[test]
    fn renames_reserved_literal_item_untouched() {
        use crate::dom::NameOrLiteral;
        use crate::dom::test_utils::re;
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.reserved_sets = vec![re("kw", &[NameOrLiteral::Literal("'expr'".into())], 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(
            g.reserved_sets[0].rule_names[0],
            NameOrLiteral::Literal("'expr'".into())
        );
    }

    /// Renaming never touches a `%reserved` entry's `set_name`, nor `reserved_set_refs`
    /// (both hold set names, not rule names).
    #[test]
    fn rename_does_not_alter_reserved_set_name_or_refs() {
        use crate::dom::test_utils::{di, re};
        let mut g = Grammar::from_rules([p("expr", nt("x"))]);
        g.reserved_sets = vec![re("expr", &[], 1)];
        g.reserved_set_refs = vec![di("expr", 1)];
        rename_grammar(&mut g, "expr", "expression").unwrap();
        assert_eq!(g.reserved_sets[0].set_name, "expr");
        assert_eq!(g.reserved_set_refs[0].name, "expr");
    }

    /// `rename_node`'s `Reserved` arm has no compiler safety net (wildcard fallback) —
    /// this is the only test guarding that the body is actually traversed.
    #[test]
    fn renames_nonterminal_in_reserved_body() {
        use crate::dom::GrammarNode::Reserved;
        let body = Reserved("kw".into(), Box::new(nt("expr")));
        let mut g = Grammar::from_rules([p("root", body), p("expr", nt("x"))]);
        rename_grammar(&mut g, "expr", "expression").unwrap();
        if let Reserved(set_name, inner) = &g.productions["root"].body {
            assert_eq!(set_name, "kw");
            assert!(matches!(inner.as_ref(), GrammarNode::NonTerminal(n) if n == "expression"));
        } else {
            panic!("expected Reserved");
        }
    }
}
