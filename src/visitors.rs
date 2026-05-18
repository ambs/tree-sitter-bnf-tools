use tree_sitter::Node;
use crate::dom::{Grammar, GrammarNode, Production};
use crate::dom::GrammarNode::*;

fn ensure_node_type(node: &Node, node_type: &str) {
    if node.kind() != node_type {
        panic!("Expected node type {} but got {}", node_type, node.kind());
    }
}

pub fn visit_grammar(node: &Node<'_>, source_code: &str) -> Grammar {
    ensure_node_type(node, "grammar");
    let mut grammar = Grammar { productions: Vec::new() };

    let count = node.child_count();
    for i in 0..count {
        let child = node.child(i).unwrap();
        let production = visit_rule(&child, &source_code);
        grammar.productions.push(production);
    }
    return grammar;
}

fn visit_rule(node: &Node<'_>, source_code: &str) -> Production {
    ensure_node_type(node, "rule");

    let rule_name = node.child(0).unwrap();
    let rule_body = node.child(2).unwrap();

    let NonTerminal(name) = visit(&rule_name, &source_code) else {
        panic!("Non Non-Terminal on LHS of production")
    };
    let body = visit(&rule_body, &source_code);

    return Production { name, body };
}

fn visit_non_terminal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    return NonTerminal(format!("{}", text));
}

fn visit_pattern(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    return TerminalPattern(format!("{}", text));
}

fn visit_literal(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let text = node.utf8_text(source_code.as_bytes()).unwrap();
    return TerminalLiteral(format!("{}", text));
}

fn visit_rule_body(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let count = node.child_count();
    if count == 1 {
        let child = node.child(0).unwrap();
        return visit(&child, &source_code);
    } else {
        let mut choice = Vec::new();
        let mut i = 0;
        while i < count {
            choice.push(visit(&node.child(i).unwrap(), &source_code));
            i += 2;
        }
        return Choice(choice);
    }
}

fn visit_symbol(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let child = node.child(0).unwrap();
    let symbol = visit(&child, &source_code);

    let mut kleene = "";
    if node.child_count() > 1 {
        kleene = node.child(1).unwrap().kind();
    }

    return match kleene {
        "plus" => OneOrMore(Box::new(symbol)),
        "asterisk" => ZeroOrMore(Box::new(symbol)),
        _ => symbol
    }
}

fn visit_symbol_seq(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let count = node.child_count();
    if count == 1 {
        let child = node.child(0).unwrap();
        return visit(&child, &source_code);
    } else {
        let mut seq = Vec::new();
        for i in 0..count {
            seq.push(visit(&node.child(i).unwrap(), &source_code));
        }
        return Sequence(seq);
    }
}

fn visit_symbol_subseq(node: &Node<'_>, source_code: &str) -> GrammarNode {
    return visit(&node.child(1).unwrap(), &source_code);
}

fn visit(node: &Node<'_>, source_code: &str) -> GrammarNode {
    let kind = node.kind();

    match kind {
        "nonTerminal" => visit_non_terminal(&node, &source_code),
        "ruleBody"    => visit_rule_body(&node, &source_code),
        "symbolSeq"   => visit_symbol_seq(&node, &source_code),
        "symbol"      => visit_symbol(&node, &source_code),
        "pattern"     => visit_pattern(&node, &source_code),
        "literal"     => visit_literal(&node, &source_code),
        "subSeq"      => visit_symbol_subseq(&node, &source_code),
        _             => panic!("Unknown node kind: {}", kind)
    }
}
