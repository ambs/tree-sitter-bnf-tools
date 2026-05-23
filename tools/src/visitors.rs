use crate::dom::GrammarNode::{self, *};
use crate::dom::{Grammar, ParseError, PrecKind, Production};
use tree_sitter::Node;

/// Returns `Ok(())` if `node.kind() == node_type`, otherwise an [`ParseError::UnexpectedNodeType`] error.
fn ensure_node_type(node: &Node, node_type: &str) -> Result<(), ParseError> {
    if node.kind() != node_type {
        Err(ParseError::UnexpectedNodeType {
            expected: node_type.to_string(),
            got: node.kind().to_string(),
        })
    } else {
        Ok(())
    }
}

/// Converts the root `grammar` tree-sitter node into a [`Grammar`] DOM.
pub fn visit_grammar(node: &Node<'_>, source_code: &str) -> Result<Grammar, ParseError> {
    ensure_node_type(node, "grammar")?;
    let mut grammar = Grammar::new();
    let count = node.child_count() as u32;
    for i in 0..count {
        let child = node.child(i).expect("child index in bounds");
        match child.kind() {
            "rule" => {
                grammar.productions.push(visit_rule(&child, source_code)?);
            }
            "conflictsDirective" => {
                grammar
                    .conflicts
                    .extend(visit_conflicts_directive(&child, source_code)?);
            }
            "inlineDirective" => {
                grammar
                    .inline
                    .extend(visit_inline_directive(&child, source_code));
            }
            "supertypesDirective" => {
                grammar
                    .supertypes
                    .extend(visit_supertypes_directive(&child, source_code));
            }
            "extrasDirective" => {
                grammar
                    .extras
                    .extend(visit_extras_directive(&child, source_code));
            }
            _ => {}
        }
    }
    grammar.check();
    Ok(grammar)
}

/// Converts an `inlineDirective` node into a flat list of rule names.
fn visit_inline_directive(node: &Node<'_>, source_code: &str) -> Vec<String> {
    (0..node.named_child_count() as u32)
        .map(|i| {
            node.named_child(i)
                .expect("named child index in bounds")
                .utf8_text(source_code.as_bytes())
                .expect("valid UTF-8")
                .to_string()
        })
        .collect()
}

/// Converts an `extrasDirective` node into a flat list of pattern strings and rule names.
fn visit_extras_directive(node: &Node<'_>, source_code: &str) -> Vec<String> {
    (0..node.named_child_count() as u32)
        .map(|i| {
            node.named_child(i)
                .expect("named child index in bounds")
                .utf8_text(source_code.as_bytes())
                .expect("valid UTF-8")
                .to_string()
        })
        .collect()
}

/// Converts a `supertypesDirective` node into a flat list of rule names.
fn visit_supertypes_directive(node: &Node<'_>, source_code: &str) -> Vec<String> {
    (0..node.named_child_count() as u32)
        .map(|i| {
            node.named_child(i)
                .expect("named child index in bounds")
                .utf8_text(source_code.as_bytes())
                .expect("valid UTF-8")
                .to_string()
        })
        .collect()
}

/// Converts a `conflictsDirective` node into a list of conflict groups (lists of rule names).
fn visit_conflicts_directive(
    node: &Node<'_>,
    source_code: &str,
) -> Result<Vec<Vec<String>>, ParseError> {
    let mut groups = Vec::new();
    for i in 0..node.named_child_count() as u32 {
        let child = node.named_child(i).expect("named child index in bounds");
        if child.kind() == "conflictGroup" {
            let names = (0..child.named_child_count() as u32)
                .map(|j| {
                    child
                        .named_child(j)
                        .expect("named child index in bounds")
                        .utf8_text(source_code.as_bytes())
                        .expect("valid UTF-8")
                        .to_string()
                })
                .collect();
            groups.push(names);
        }
    }
    Ok(groups)
}

/// Converts a `rule` node into a [`Production`].
fn visit_rule(node: &Node<'_>, source_code: &str) -> Result<Production, ParseError> {
    ensure_node_type(node, "rule")?;
    let rule_name = node
        .child_by_field_name("name")
        .expect("rule has name field");
    let rule_body = node
        .child_by_field_name("body")
        .expect("rule has body field");
    let NonTerminal(name) = visit(&rule_name, source_code)? else {
        return Err(ParseError::MalformedProduction);
    };
    let body = visit(&rule_body, source_code)?;
    Ok(Production { name, body })
}

/// Converts a `nonTerminal` node into a [`GrammarNode::NonTerminal`].
fn visit_non_terminal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    NonTerminal(text.to_string())
}

/// Converts a `pattern` node into a [`GrammarNode::TerminalPattern`].
fn visit_pattern(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    TerminalPattern(text.to_string())
}

/// Normalises a BNF literal to tree-sitter single-quote form, converting double-quoted strings.
fn normalize_literal(text: &str) -> String {
    if text.starts_with('"') {
        let inner = &text[1..text.len() - 1];
        let content = inner.replace("\\\"", "\"").replace('\'', "\\'");
        format!("'{content}'")
    } else {
        text.to_string()
    }
}

/// Converts a `literal` node into a [`GrammarNode::TerminalLiteral`], normalising quotes.
fn visit_literal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    TerminalLiteral(normalize_literal(text))
}

/// Converts a `ruleBody` or `ruleBodyInner` node into a [`GrammarNode`], wrapping multiple alternatives in [`GrammarNode::Choice`].
fn visit_rule_body(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let count = node.child_count() as u32;
    if count == 1 {
        visit(&node.child(0).expect("child 0 exists"), source_code)
    } else {
        let mut choice = Vec::new();
        let mut i: u32 = 0;
        while i < count {
            choice.push(visit(
                &node.child(i).expect("child index in bounds"),
                source_code,
            )?);
            i += 2;
        }
        Ok(Choice(choice))
    }
}

/// Converts a `symbol` node, applying any Kleene quantifier and optional field label.
fn visit_symbol(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let symbol = visit(
        &node
            .child_by_field_name("content")
            .expect("symbol has content field"),
        source_code,
    )?;
    let kleene = node
        .child_by_field_name("kleene")
        .as_ref()
        .map(|n| n.kind())
        .unwrap_or("");
    let with_kleene = match kleene {
        "plus" => OneOrMore(Box::new(symbol)),
        "asterisk" => ZeroOrMore(Box::new(symbol)),
        "questionMark" => Optional(Box::new(symbol)),
        _ => symbol,
    };
    Ok(
        if let Some(label_node) = node.child_by_field_name("label") {
            let text = label_node
                .utf8_text(source_code.as_bytes())
                .expect("valid UTF-8");
            let name = text.trim_end_matches(':').to_string();
            Field(name, Box::new(with_kleene))
        } else {
            with_kleene
        },
    )
}

/// Extracts the precedence kind and optional numeric level from a `precAnnotation` node.
fn parse_prec_annotation(
    node: &Node<'_>,
    source_code: &str,
) -> Result<(PrecKind, Option<u32>), ParseError> {
    let kind_node = node
        .child_by_field_name("kind")
        .expect("precAnnotation has kind field");
    let kind_text = kind_node
        .utf8_text(source_code.as_bytes())
        .expect("valid UTF-8");
    let kind = match kind_text {
        "prec" => PrecKind::Plain,
        "prec.left" => PrecKind::Left,
        "prec.right" => PrecKind::Right,
        "prec.dynamic" => PrecKind::Dynamic,
        other => return Err(ParseError::UnknownNodeKind(other.to_string())),
    };
    let level = if let Some(n) = node.child_by_field_name("level") {
        let text = n.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
        Some(
            text.parse::<u32>()
                .map_err(|_| ParseError::MalformedProduction)?,
        )
    } else {
        None
    };
    Ok((kind, level))
}

/// Converts a `symbolSeq` or `symbolSeqInner` node into a sequence (or single node), wrapping with precedence if annotated.
fn visit_symbol_seq(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let prec_annotation = if let Some(n) = node.child_by_field_name("prec") {
        Some(parse_prec_annotation(&n, source_code)?)
    } else {
        None
    };

    let count = node.child_count() as u32;
    let symbol_count = if prec_annotation.is_some() {
        count - 1
    } else {
        count
    };

    let body = if symbol_count == 1 {
        visit(&node.child(0).expect("child 0 exists"), source_code)?
    } else {
        let seq = (0..symbol_count)
            .map(|i| visit(&node.child(i).expect("child index in bounds"), source_code))
            .collect::<Result<Vec<_>, _>>()?;
        Sequence(seq)
    };

    Ok(match prec_annotation {
        Some((kind, level)) => Prec(kind, level, Box::new(body)),
        None => body,
    })
}

/// Converts a `subSeq` node by delegating to its `body` field.
fn visit_symbol_subseq(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    visit(
        &node
            .child_by_field_name("body")
            .expect("subSeq has body field"),
        source_code,
    )
}

/// Converts a `tokenExpr` node into a [`GrammarNode::Token`] wrapping its body.
fn visit_token_expr(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let inner = visit(
        &node
            .child_by_field_name("body")
            .expect("tokenExpr has body field"),
        source_code,
    )?;
    Ok(Token(Box::new(inner)))
}

/// Converts a `tokenImmediateExpr` node into a [`GrammarNode::TokenImmediate`] wrapping its body.
fn visit_token_immediate_expr(
    node: &Node<'_>,
    source_code: &str,
) -> Result<GrammarNode, ParseError> {
    let inner = visit(
        &node
            .child_by_field_name("body")
            .expect("tokenImmediateExpr has body field"),
        source_code,
    )?;
    Ok(TokenImmediate(Box::new(inner)))
}

/// Converts a `precGroup` node into a [`GrammarNode::Prec`] wrapping its body.
fn visit_prec_group(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let body = visit(
        &node
            .child_by_field_name("body")
            .expect("precGroup has body field"),
        source_code,
    )?;
    let annotation = node
        .child_by_field_name("annotation")
        .expect("precGroup has annotation field");
    let (kind, level) = parse_prec_annotation(&annotation, source_code)?;
    Ok(Prec(kind, level, Box::new(body)))
}

/// Converts an `aliasGroup` node into a [`GrammarNode::Alias`].
fn visit_alias_group(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let body = visit(
        &node
            .child_by_field_name("body")
            .expect("aliasGroup has body field"),
        source_code,
    )?;
    let alias_node = node
        .child_by_field_name("alias")
        .expect("aliasGroup has alias field");
    let name_child = alias_node.child(0).expect("aliasName has a child");
    let name = visit(&name_child, source_code)?;
    Ok(Alias(Box::new(body), Box::new(name)))
}

/// Dispatches a tree-sitter node to the appropriate typed visitor by node kind.
fn visit(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    match node.kind() {
        "nonTerminal" => Ok(visit_non_terminal(node, source_code)),
        "ruleBody" => visit_rule_body(node, source_code),
        "symbolSeq" => visit_symbol_seq(node, source_code),
        "symbol" => visit_symbol(node, source_code),
        "pattern" => Ok(visit_pattern(node, source_code)),
        "literal" => Ok(visit_literal(node, source_code)),
        "subSeq" => visit_symbol_subseq(node, source_code),
        "aliasGroup" => visit_alias_group(node, source_code),
        "tokenExpr" => visit_token_expr(node, source_code),
        "tokenImmediateExpr" => visit_token_immediate_expr(node, source_code),
        "precGroup" => visit_prec_group(node, source_code),
        "ruleBodyInner" => visit_rule_body(node, source_code),
        "symbolSeqInner" => visit_symbol_seq(node, source_code),
        kind => Err(ParseError::UnknownNodeKind(kind.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> String {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bnf::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        visit_grammar(&tree.root_node(), src)
            .unwrap()
            .to_string()
            .trim()
            .to_string()
    }

    fn parse_grammar(src: &str) -> crate::dom::Grammar {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bnf::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        visit_grammar(&tree.root_node(), src).unwrap()
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
}
