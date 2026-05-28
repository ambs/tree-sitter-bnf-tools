//! Integration tests for the BNF → grammar.js visitor pipeline.

use ts_bnf_tool::dom::Grammar;
use ts_bnf_tool::visitors::parse_source;

fn parse(src: &str) -> String {
    parse_source(src).unwrap().0.to_string().trim().to_string()
}

fn parse_grammar(src: &str) -> Grammar {
    parse_source(src).unwrap().0
}

#[test]
fn literal_terminal() {
    assert_eq!(parse("a -> 'x';"), "a -> 'x'");
}

#[test]
fn double_quoted_literal_normalised_to_single() {
    assert_eq!(parse(r#"a -> "x";"#), "a -> 'x'");
}

#[test]
fn double_quoted_literal_with_embedded_single_quote() {
    assert_eq!(parse(r#"a -> "it's";"#), r"a -> 'it\'s'");
}

#[test]
fn double_quoted_literal_with_escaped_double_quote() {
    assert_eq!(parse(r#"a -> "say \"hi\"";"#), r#"a -> 'say "hi"'"#);
}

#[test]
fn pattern_terminal() {
    assert_eq!(parse("a -> /x+/;"), "a -> /x+/");
}

#[test]
fn nonterminal_ref() {
    assert_eq!(parse("a -> b;"), "a -> $.b");
}

#[test]
fn alternatives() {
    assert_eq!(parse("a -> 'x' | 'y';"), "a -> choice('x', 'y')");
}

#[test]
fn sequence() {
    assert_eq!(parse("a -> 'x' 'y';"), "a -> seq('x', 'y')");
}

#[test]
fn kleene_star() {
    assert_eq!(parse("a -> 'x'*;"), "a -> repeat('x')");
}

#[test]
fn kleene_question_mark() {
    assert_eq!(parse("a -> 'x'?;"), "a -> optional('x')");
}

#[test]
fn kleene_plus() {
    assert_eq!(parse("a -> 'x'+;"), "a -> repeat1('x')");
}

#[test]
fn grouped_subseq_asterisk() {
    assert_eq!(parse("a -> ('x' | 'y')*;"), "a -> repeat(choice('x', 'y'))");
}

#[test]
fn grouped_subseq_plus() {
    assert_eq!(
        parse("a -> ('x' | 'y')+;"),
        "a -> repeat1(choice('x', 'y'))"
    );
}

#[test]
fn grouped_subseq_optional() {
    assert_eq!(
        parse("a -> ('x' | 'y')?;"),
        "a -> optional(choice('x', 'y'))"
    );
}

#[test]
fn sequence_with_alternative() {
    assert_eq!(
        parse("a -> 'x' 'y' | 'z';"),
        "a -> choice(seq('x', 'y'), 'z')"
    );
}

#[test]
fn multi_rule() {
    assert_eq!(parse("a -> 'x';\nb -> a;"), "a -> 'x'\nb -> $.a");
}

#[test]
fn token_expr_single_pattern() {
    assert_eq!(parse("a -> << /[0-9]+/ >>;"), "a -> token(/[0-9]+/)");
}

#[test]
fn token_immediate_expr_single_pattern() {
    assert_eq!(
        parse("a -> <<! /[0-9]+/ >>;"),
        "a -> token.immediate(/[0-9]+/)"
    );
}

#[test]
fn token_immediate_expr_sequence() {
    assert_eq!(
        parse("a -> <<! /[A-Za-z_]/ /[A-Za-z0-9_]*/ >>;"),
        "a -> token.immediate(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))"
    );
}

#[test]
fn token_immediate_expr_literal() {
    assert_eq!(
        parse("negative -> '-' <<! /[0-9]+/ >>;"),
        "negative -> seq('-', token.immediate(/[0-9]+/))"
    );
}

#[test]
fn token_expr_sequence() {
    assert_eq!(
        parse("a -> << /[A-Za-z_]/ /[A-Za-z0-9_]*/ >>;"),
        "a -> token(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))"
    );
}

#[test]
fn token_expr_alternatives() {
    assert_eq!(
        parse("a -> << '+' | '-' >>;"),
        "a -> token(choice('+', '-'))"
    );
}

#[test]
fn token_expr_with_kleene_plus() {
    assert_eq!(
        parse("a -> << /[0-9]/ >>+;"),
        "a -> repeat1(token(/[0-9]/))"
    );
}

#[test]
fn field_label_on_nonterminal() {
    assert_eq!(parse("rule -> lhs: expr ;"), "rule -> field('lhs', $.expr)");
}

#[test]
fn field_label_on_literal() {
    assert_eq!(parse("rule -> key: 'foo' ;"), "rule -> field('key', 'foo')");
}

#[test]
fn multiple_field_labels() {
    assert_eq!(
        parse("rule -> lhs: expr '+' rhs: expr ;"),
        "rule -> seq(field('lhs', $.expr), '+', field('rhs', $.expr))"
    );
}

#[test]
fn field_label_with_kleene() {
    assert_eq!(
        parse("rule -> items: expr* ;"),
        "rule -> field('items', repeat($.expr))"
    );
}

#[test]
fn alias_group_nonterminal_name() {
    assert_eq!(
        parse("rule -> (a b => pair) ;"),
        "rule -> alias(seq($.a, $.b), $.pair)"
    );
}

#[test]
fn alias_group_literal_name() {
    assert_eq!(
        parse("rule -> (a b => 'pair') ;"),
        "rule -> alias(seq($.a, $.b), 'pair')"
    );
}

#[test]
fn alias_group_single_symbol() {
    assert_eq!(
        parse("rule -> (foo => bar) ;"),
        "rule -> alias($.foo, $.bar)"
    );
}

#[test]
fn alias_group_with_kleene() {
    assert_eq!(
        parse("rule -> (a b => pair)* ;"),
        "rule -> repeat(alias(seq($.a, $.b), $.pair))"
    );
}

#[test]
fn prec_plain_on_alternative() {
    assert_eq!(parse("a -> b c %prec 2 ;"), "a -> prec(2, seq($.b, $.c))");
}

#[test]
fn prec_left_on_alternative() {
    assert_eq!(
        parse("expr -> expr '+' expr %prec.left 1 ;"),
        "expr -> prec.left(1, seq($.expr, '+', $.expr))"
    );
}

#[test]
fn prec_right_on_alternative() {
    assert_eq!(
        parse("expr -> expr '^' expr %prec.right 3 ;"),
        "expr -> prec.right(3, seq($.expr, '^', $.expr))"
    );
}

#[test]
fn prec_dynamic_on_alternative() {
    assert_eq!(
        parse("a -> b c %prec.dynamic 5 ;"),
        "a -> prec.dynamic(5, seq($.b, $.c))"
    );
}

#[test]
fn prec_left_no_level() {
    assert_eq!(
        parse("a -> b c %prec.left ;"),
        "a -> prec.left(seq($.b, $.c))"
    );
}

#[test]
fn prec_on_single_symbol_alternative() {
    assert_eq!(
        parse("rule -> if_stmt %prec 1 | other ;"),
        "rule -> choice(prec(1, $.if_stmt), $.other)"
    );
}

#[test]
fn prec_group_wraps_choice() {
    assert_eq!(
        parse("rule -> (a | b %prec 1) c ;"),
        "rule -> seq(prec(1, choice($.a, $.b)), $.c)"
    );
}

#[test]
fn prec_group_single_symbol() {
    assert_eq!(
        parse("rule -> (a %prec 1) b c ;"),
        "rule -> seq(prec(1, $.a), $.b, $.c)"
    );
}

#[test]
fn prec_group_with_kleene() {
    assert_eq!(
        parse("rule -> (a %prec 1)* ;"),
        "rule -> repeat(prec(1, $.a))"
    );
}

#[test]
fn conflicts_single_group() {
    let g = parse_grammar("%conflicts [a, b]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "b"]]);
}

#[test]
fn conflicts_three_rules_in_group() {
    let g = parse_grammar("%conflicts [a, b, c]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "b", "c"]]);
}

#[test]
fn conflicts_multiple_groups_one_line() {
    let g = parse_grammar("%conflicts [a, b], [c, d]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "b"], vec!["c", "d"]]);
}

#[test]
fn conflicts_multiple_directives_are_additive() {
    let g = parse_grammar("%conflicts [a, b]\n%conflicts [c, d]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "b"], vec!["c", "d"]]);
}

#[test]
fn conflicts_interleaved_with_rules() {
    let g = parse_grammar("a -> 'x' ;\n%conflicts [a, b]\nb -> 'y' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "b"]]);
    assert_eq!(g.productions.len(), 2);
}

#[test]
fn conflicts_undefined_rule_still_parses() {
    let g = parse_grammar("%conflicts [a, ghost]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![vec!["a", "ghost"]]);
}

#[test]
fn inline_single_rule() {
    let g = parse_grammar("%inline _helper\na -> _helper ;");
    assert_eq!(g.inline, vec!["_helper"]);
}

#[test]
fn inline_multiple_rules_one_directive() {
    let g = parse_grammar("%inline _a, _b\na -> _a ;");
    assert_eq!(g.inline, vec!["_a", "_b"]);
}

#[test]
fn inline_multiple_directives_are_additive() {
    let g = parse_grammar("%inline _a\n%inline _b\na -> _a ;");
    assert_eq!(g.inline, vec!["_a", "_b"]);
}

#[test]
fn inline_interleaved_with_rules() {
    let g = parse_grammar("a -> _h ;\n%inline _h\n_h -> 'x' ;");
    assert_eq!(g.inline, vec!["_h"]);
    assert_eq!(g.productions.len(), 2);
}

#[test]
fn inline_undefined_rule_still_parses() {
    let g = parse_grammar("%inline ghost\na -> 'x' ;");
    assert_eq!(g.inline, vec!["ghost"]);
}

#[test]
fn supertypes_single_rule() {
    let g = parse_grammar("%supertypes expression\nexpression -> 'x' ;");
    assert_eq!(g.supertypes, vec!["expression"]);
}

#[test]
fn supertypes_multiple_rules_one_directive() {
    let g = parse_grammar("%supertypes expression, statement\nexpression -> 'x' ;");
    assert_eq!(g.supertypes, vec!["expression", "statement"]);
}

#[test]
fn supertypes_multiple_directives_are_additive() {
    let g = parse_grammar("%supertypes expression\n%supertypes statement\nexpression -> 'x' ;");
    assert_eq!(g.supertypes, vec!["expression", "statement"]);
}

#[test]
fn supertypes_interleaved_with_rules() {
    let g = parse_grammar("expression -> 'x' ;\n%supertypes expression\nstatement -> 'y' ;");
    assert_eq!(g.supertypes, vec!["expression"]);
    assert_eq!(g.productions.len(), 2);
}

#[test]
fn supertypes_undefined_rule_still_parses() {
    let g = parse_grammar("%supertypes ghost\na -> 'x' ;");
    assert_eq!(g.supertypes, vec!["ghost"]);
}

#[test]
fn extras_single_pattern() {
    let g = parse_grammar("%extras /\\s/\na -> 'x' ;");
    assert_eq!(g.extras, vec!["/\\s/"]);
}

#[test]
fn extras_pattern_and_rule() {
    let g = parse_grammar("%extras /\\s/, comment\na -> 'x' ;\ncomment -> '#' ;");
    assert_eq!(g.extras, vec!["/\\s/", "comment"]);
}

#[test]
fn extras_multiple_directives_are_additive() {
    let g = parse_grammar("%extras /\\s/\n%extras comment\na -> 'x' ;\ncomment -> '#' ;");
    assert_eq!(g.extras, vec!["/\\s/", "comment"]);
}

#[test]
fn extras_interleaved_with_rules() {
    let g = parse_grammar("a -> 'x' ;\n%extras /\\s/\nb -> 'y' ;");
    assert_eq!(g.extras, vec!["/\\s/"]);
    assert_eq!(g.productions.len(), 2);
}

#[test]
fn extras_undefined_rule_still_parses() {
    let g = parse_grammar("%extras ghost\na -> 'x' ;");
    assert_eq!(g.extras, vec!["ghost"]);
}

#[test]
fn prec_combined_with_alias() {
    assert_eq!(
        parse("rule -> (a b %prec.left 1 => op) ;"),
        "rule -> alias(prec.left(1, seq($.a, $.b)), $.op)"
    );
}

#[test]
fn prec_nested_groups() {
    assert_eq!(
        parse("rule -> (a | (b %prec 1)) ;"),
        "rule -> choice($.a, prec(1, $.b))"
    );
}
