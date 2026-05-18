use crate::dom::GrammarNode::*;
use crate::dom::{Grammar, GrammarNode, ParseError, Production};
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
    let rule_name = node.child(0).expect("rule has name child");
    let rule_body = node.child(2).expect("rule has body child");
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

fn visit_literal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).expect("valid UTF-8");
    TerminalLiteral(text.to_string())
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
    let symbol = visit(&node.child(0).expect("symbol has child"), source_code)?;
    let kleene = if node.child_count() > 1 {
        node.child(1).expect("child 1 exists").kind()
    } else {
        ""
    };
    Ok(match kleene {
        "plus" => OneOrMore(Box::new(symbol)),
        "asterisk" => ZeroOrMore(Box::new(symbol)),
        _ => symbol,
    })
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
    visit(&node.child(1).expect("subseq has inner child"), source_code)
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
    fn kleene_plus() {
        assert_eq!(parse("a -> 'x'+;"), "a -> repeat1('x')");
    }

    #[test]
    fn grouped_subseq() {
        assert_eq!(parse("a -> ('x' | 'y')*;"), "a -> repeat(choice('x', 'y'))");
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
}
