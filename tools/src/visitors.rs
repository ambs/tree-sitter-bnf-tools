use crate::dom::GrammarNode::*;
use crate::dom::{Grammar, GrammarNode, Production};
use tree_sitter::Node;

fn ensure_node_type(node: &Node, node_type: &str) {
    if node.kind() != node_type {
        panic!("Expected node type {} but got {}", node_type, node.kind());
    }
}

pub fn visit_grammar(node: &Node<'_>, source_code: &str) -> Grammar {
    ensure_node_type(node, "grammar");
    let mut grammar = Grammar {
        productions: Vec::new(),
    };
    let count = node.child_count();
    for i in 0..count {
        let child = node.child(i).unwrap();
        let production = visit_rule(&child, source_code);
        grammar.productions.push(production);
    }
    grammar
}

fn visit_rule(node: &Node<'_>, source_code: &str) -> Production {
    ensure_node_type(node, "rule");
    let rule_name = node.child(0).unwrap();
    let rule_body = node.child(2).unwrap();
    let NonTerminal(name) = visit(&rule_name, source_code) else {
        panic!("Non Non-Terminal on LHS of production")
    };
    let body = visit(&rule_body, source_code);
    Production { name, body }
}

fn visit_non_terminal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    NonTerminal(text.to_string())
}

fn visit_pattern(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    TerminalPattern(text.to_string())
}

fn visit_literal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    TerminalLiteral(text.to_string())
}

fn visit_rule_body(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let count = node.child_count();
    if count == 1 {
        visit(&node.child(0).unwrap(), source_code)
    } else {
        let mut choice = Vec::new();
        let mut i = 0;
        while i < count {
            choice.push(visit(&node.child(i).unwrap(), source_code));
            i += 2;
        }
        Choice(choice)
    }
}

fn visit_symbol(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let symbol = visit(&node.child(0).unwrap(), source_code);
    let kleene = if node.child_count() > 1 {
        node.child(1).unwrap().kind()
    } else {
        ""
    };
    match kleene {
        "plus" => OneOrMore(Box::new(symbol)),
        "asterisk" => ZeroOrMore(Box::new(symbol)),
        _ => symbol,
    }
}

fn visit_symbol_seq(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let count = node.child_count();
    if count == 1 {
        visit(&node.child(0).unwrap(), source_code)
    } else {
        let seq = (0..count)
            .map(|i| visit(&node.child(i).unwrap(), source_code))
            .collect();
        Sequence(seq)
    }
}

fn visit_symbol_subseq(node: &Node<'_>, source_code: &str) -> GrammarNode {
    visit(&node.child(1).unwrap(), source_code)
}

fn visit(node: &Node<'_>, source_code: &str) -> GrammarNode {
    match node.kind() {
        "nonTerminal" => visit_non_terminal(node, source_code),
        "ruleBody" => visit_rule_body(node, source_code),
        "symbolSeq" => visit_symbol_seq(node, source_code),
        "symbol" => visit_symbol(node, source_code),
        "pattern" => visit_pattern(node, source_code),
        "literal" => visit_literal(node, source_code),
        "subSeq" => visit_symbol_subseq(node, source_code),
        kind => panic!("Unknown node kind: {}", kind),
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
