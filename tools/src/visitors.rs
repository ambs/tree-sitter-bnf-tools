use crate::dom::GrammarNode::{self, *};
use crate::dom::{Grammar, ParseError, Production};
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

fn visit_symbol_seq(node: &Node<'_>, source_code: &str) -> Result<GrammarNode, ParseError> {
    let count = node.child_count() as u32;
    if count == 1 {
        visit(&node.child(0).expect("child 0 exists"), source_code)
    } else {
        let seq = (0..count)
            .map(|i| visit(&node.child(i).expect("child index in bounds"), source_code))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Sequence(seq))
    }
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
}
