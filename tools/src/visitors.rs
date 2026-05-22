use crate::dom::GrammarNode::{self, *};
use crate::dom::{Grammar, ParseError, PrecKind, Production};
use tree_sitter::Node;

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

pub fn visit_grammar(node: &Node<'_>, source_code: &str) -> Result<Grammar, ParseError> {
    ensure_node_type(node, "grammar")?;
    let mut grammar = Grammar {
        productions: Vec::new(),
    };
    let count = node.child_count() as u32;
    for i in 0..count {
        let child = node.child(i).expect("child index in bounds");
        let production = visit_rule(&child, source_code)?;
        grammar.productions.push(production);
    }
    Ok(grammar)
}

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

fn visit_non_terminal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    NonTerminal(text.to_string())
}

fn visit_pattern(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    TerminalPattern(text.to_string())
}

fn normalize_literal(text: &str) -> String {
    if text.starts_with('"') {
        let inner = &text[1..text.len() - 1];
        let content = inner.replace("\\\"", "\"").replace('\'', "\\'");
        format!("'{content}'")
    } else {
        text.to_string()
    }
}

fn visit_literal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    TerminalLiteral(normalize_literal(text))
}

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

fn visit_symbol_subseq(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    visit(
        &node
            .child_by_field_name("body")
            .expect("subSeq has body field"),
        source_code,
    )
}

fn visit_token_expr(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let inner = visit(
        &node
            .child_by_field_name("body")
            .expect("tokenExpr has body field"),
        source_code,
    )?;
    Ok(Token(Box::new(inner)))
}

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
