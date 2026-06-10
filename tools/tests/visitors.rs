//! Integration tests for the BNF → grammar.js visitor pipeline.

use ts_bnf_tool::dom::test_utils::{cg, di};
use ts_bnf_tool::dom::{DirectiveItem, Grammar, Severity};
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
fn kleene_operators() {
    for (src, expected) in [
        ("a -> 'x'*;", "a -> repeat('x')"),
        ("a -> 'x'?;", "a -> optional('x')"),
        ("a -> 'x'+;", "a -> repeat1('x')"),
        ("a -> ('x' | 'y')*;", "a -> repeat(choice('x', 'y'))"),
        ("a -> ('x' | 'y')+;", "a -> repeat1(choice('x', 'y'))"),
        ("a -> ('x' | 'y')?;", "a -> optional(choice('x', 'y'))"),
    ] {
        assert_eq!(parse(src), expected, "source: {src}");
    }
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
fn token_expressions() {
    for (src, expected) in [
        ("a -> << /[0-9]+/ >>;", "a -> token(/[0-9]+/)"),
        ("a -> <<! /[0-9]+/ >>;", "a -> token.immediate(/[0-9]+/)"),
        (
            "a -> <<! /[A-Za-z_]/ /[A-Za-z0-9_]*/ >>;",
            "a -> token.immediate(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))",
        ),
        (
            "negative -> '-' <<! /[0-9]+/ >>;",
            "negative -> seq('-', token.immediate(/[0-9]+/))",
        ),
        (
            "a -> << /[A-Za-z_]/ /[A-Za-z0-9_]*/ >>;",
            "a -> token(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))",
        ),
        ("a -> << '+' | '-' >>;", "a -> token(choice('+', '-'))"),
        ("a -> << /[0-9]/ >>+;", "a -> repeat1(token(/[0-9]/))"),
    ] {
        assert_eq!(parse(src), expected, "source: {src}");
    }
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
fn conflicts_directive() {
    // single group
    let g = parse_grammar("%conflicts [a, b]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "b"], 1)]);

    // three rules in one group
    let g = parse_grammar("%conflicts [a, b, c]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "b", "c"], 1)]);

    // several groups on one line
    let g = parse_grammar("%conflicts [a, b], [c, d]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "b"], 1), cg(&["c", "d"], 1)]);

    // multiple directives are additive
    let g = parse_grammar("%conflicts [a, b]\n%conflicts [c, d]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "b"], 1), cg(&["c", "d"], 2)]);

    // interleaved with rules
    let g = parse_grammar("a -> 'x' ;\n%conflicts [a, b]\nb -> 'y' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "b"], 2)]);
    assert_eq!(g.productions.len(), 2);

    // undefined rule still parses (check warns later; parsing must not fail)
    let g = parse_grammar("%conflicts [a, ghost]\na -> 'x' ;");
    assert_eq!(g.conflicts, vec![cg(&["a", "ghost"], 1)]);
}

/// %inline, %supertypes and %extras share their name-list semantics: items
/// accumulate with their source line, directives are additive and may
/// interleave with rules, and naming an undefined rule does not abort parsing.
#[test]
fn name_list_directives() {
    /// Accessor for the grammar field a name-list directive populates.
    type Field = fn(&Grammar) -> &Vec<DirectiveItem>;
    let directives: [(&str, Field); 3] = [
        ("inline", |g| &g.inline),
        ("supertypes", |g| &g.supertypes),
        ("extras", |g| &g.extras),
    ];
    for (dir, field) in directives {
        let g = parse_grammar(&format!("%{dir} foo\na -> foo ;\nfoo -> 'x' ;"));
        assert_eq!(field(&g), &vec![di("foo", 1)], "%{dir}: single rule");

        let g = parse_grammar(&format!("%{dir} foo, bar\na -> 'x' ;"));
        assert_eq!(
            field(&g),
            &vec![di("foo", 1), di("bar", 1)],
            "%{dir}: two rules in one directive"
        );

        let g = parse_grammar(&format!("%{dir} foo\n%{dir} bar\na -> 'x' ;"));
        assert_eq!(
            field(&g),
            &vec![di("foo", 1), di("bar", 2)],
            "%{dir}: directives are additive"
        );

        let g = parse_grammar(&format!("a -> 'x' ;\n%{dir} foo\nb -> 'y' ;"));
        assert_eq!(field(&g), &vec![di("foo", 2)], "%{dir}: interleaved");
        assert_eq!(g.productions.len(), 2, "%{dir}: interleaved keeps rules");

        let g = parse_grammar(&format!("%{dir} ghost\na -> 'x' ;"));
        assert_eq!(
            field(&g),
            &vec![di("ghost", 1)],
            "%{dir}: undefined rule still parses"
        );
    }
}

#[test]
fn extras_single_pattern() {
    let g = parse_grammar("%extras /\\s/\na -> 'x' ;");
    assert_eq!(g.extras, vec![di("/\\s/", 1)]);
}

#[test]
fn extras_pattern_and_rule() {
    let g = parse_grammar("%extras /\\s/, comment\na -> 'x' ;\ncomment -> '#' ;");
    assert_eq!(g.extras, vec![di("/\\s/", 1), di("comment", 1)]);
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

#[test]
fn duplicate_rule_emits_warning() {
    let (_, diagnostics) = parse_source("a -> 'x'; a -> 'y';").unwrap();
    assert!(diagnostics.iter().any(|d| d.severity == Severity::Warning
        && d.message.contains("'a'")
        && d.message.contains("more than once")));
}

#[test]
fn duplicate_rule_second_definition_wins() {
    let (grammar, _) = parse_source("a -> 'x'; a -> 'y';").unwrap();
    assert_eq!(grammar.to_string().trim(), "a -> 'y'");
}

#[test]
fn axiom_directive_sets_axiom_field() {
    let g = parse_grammar("%axiom root\nroot -> 'x' ;\n");
    assert_eq!(g.axiom.as_ref().map(|a| a.name.as_str()), Some("root"));
}

#[test]
fn axiom_directive_line_is_recorded() {
    let g = parse_grammar("%axiom root\nroot -> 'x' ;\n");
    assert_eq!(g.axiom.as_ref().map(|a| a.line), Some(1));
}

#[test]
fn duplicate_axiom_emits_error() {
    let (_, diags) = parse_source("%axiom foo\n%axiom bar\nfoo -> 'x' ;\nbar -> 'y' ;\n").unwrap();
    assert!(diags.iter().any(|d| {
        d.severity == Severity::Error && d.message.contains("%axiom declared more than once")
    }));
}

#[test]
fn axiom_undefined_rule_emits_error() {
    let (_, diags) = parse_source("%axiom ghost\nroot -> 'x' ;\n").unwrap();
    assert!(diags
        .iter()
        .any(|d| { d.severity == Severity::Error && d.message.contains("'ghost'") }));
}
