/// Pretty-printer that re-emits a [`Grammar`] as canonical BNF text.
///
/// **Known limitation**: comments are not stored in the DOM and will be stripped
/// from the output. See issue #148 for the plan to preserve them.
use super::directive::{ConflictGroup, DirectiveItem};
use super::nodes::{GrammarNode, PrecKind};
use super::production::Production;
use super::types::Grammar;

/// Maximum line length before a multi-alternative rule is split across lines.
const LINE_WIDTH: usize = 80;

/// Formats `grammar` as canonical BNF and returns the result as a `String`.
///
/// Directive order: `%extras`, `%conflicts`, `%inline`, `%supertypes` (all before rules).
/// Rules follow in their original declaration order, each separated by a blank line.
pub fn format_grammar(grammar: &Grammar) -> String {
    let mut out = String::new();

    let directives = collect_directives(grammar);
    for directive in &directives {
        out.push_str(directive);
        out.push('\n');
    }

    let has_directives = !directives.is_empty();
    let mut first_rule = true;

    for production in grammar.productions.values() {
        if !first_rule || has_directives {
            out.push('\n');
        }
        first_rule = false;
        out.push_str(&format_production(production));
        out.push('\n');
    }

    out
}

/// Collects all directives from `grammar` as formatted strings in canonical order.
fn collect_directives(grammar: &Grammar) -> Vec<String> {
    let mut out = Vec::new();
    if !grammar.extras.is_empty() {
        out.push(format_directive("extras", &grammar.extras));
    }
    if !grammar.conflicts.is_empty() {
        out.push(format_conflicts(&grammar.conflicts));
    }
    if !grammar.inline.is_empty() {
        out.push(format_directive("inline", &grammar.inline));
    }
    if !grammar.supertypes.is_empty() {
        out.push(format_directive("supertypes", &grammar.supertypes));
    }
    out
}

/// Formats a simple directive (`%extras`, `%inline`, `%supertypes`) as `%name item, item`.
fn format_directive(name: &str, items: &[DirectiveItem]) -> String {
    let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
    format!("%{} {}", name, names.join(", "))
}

/// Formats a `%conflicts` directive as `%conflicts [a, b], [c, d]`.
fn format_conflicts(groups: &[ConflictGroup]) -> String {
    let groups_str: Vec<String> = groups
        .iter()
        .map(|g| format!("[{}]", g.rules.join(", ")))
        .collect();
    format!("%conflicts {}", groups_str.join(", "))
}

/// Formats a `GrammarNode` in top-level (alternative) context.
///
/// A `Prec` at this level is emitted without surrounding parentheses: `body %kind level`.
/// A `Sequence` at this level has its items space-joined without parentheses.
/// Everything else delegates to `format_node_nested`.
fn format_node_top(node: &GrammarNode) -> String {
    match node {
        GrammarNode::Prec(kind, level, inner) => {
            format!(
                "{} {}",
                format_sequence_items(inner),
                prec_annotation(kind, *level)
            )
        }
        GrammarNode::Sequence(items) => items
            .iter()
            .map(format_node_nested)
            .collect::<Vec<_>>()
            .join(" "),
        other => format_node_nested(other),
    }
}

/// Formats a `GrammarNode` in nested context (inside a quantifier, token, field, etc.).
///
/// Choices gain surrounding parentheses so `|` is not misread as a rule-level separator.
/// A nested `Prec` uses the parenthesised `precGroup` form: `(body %kind level)`.
fn format_node_nested(node: &GrammarNode) -> String {
    match node {
        GrammarNode::NonTerminal(name) => name.clone(),
        GrammarNode::TerminalLiteral(s) | GrammarNode::TerminalPattern(s) => s.clone(),
        GrammarNode::Sequence(items) => items
            .iter()
            .map(format_node_nested)
            .collect::<Vec<_>>()
            .join(" "),
        GrammarNode::Choice(alts) => {
            let inner = alts
                .iter()
                .map(format_node_top)
                .collect::<Vec<_>>()
                .join(" | ");
            format!("({})", inner)
        }
        GrammarNode::Optional(inner) => format!("{}?", quantifier_inner(inner)),
        GrammarNode::ZeroOrMore(inner) => format!("{}*", quantifier_inner(inner)),
        GrammarNode::OneOrMore(inner) => format!("{}+", quantifier_inner(inner)),
        GrammarNode::Token(inner) => format!("<< {} >>", format_node_top(inner)),
        GrammarNode::TokenImmediate(inner) => format!("<<! {} >>", format_node_top(inner)),
        GrammarNode::Field(name, inner) => format!("{}: {}", name, format_node_nested(inner)),
        GrammarNode::Alias(body, name) => {
            format!(
                "({} => {})",
                format_node_top(body),
                format_node_nested(name)
            )
        }
        GrammarNode::Prec(kind, level, inner) => {
            format!(
                "({} {})",
                format_sequence_items(inner),
                prec_annotation(kind, *level)
            )
        }
    }
}

/// Formats the inner node of a quantifier, adding parentheses when the inner node is a
/// `Sequence` or `Choice` so the quantifier applies to the whole group.
fn quantifier_inner(node: &GrammarNode) -> String {
    match node {
        GrammarNode::Sequence(items) => {
            let inner = items
                .iter()
                .map(format_node_nested)
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        GrammarNode::Choice(alts) => {
            let inner = alts
                .iter()
                .map(format_node_top)
                .collect::<Vec<_>>()
                .join(" | ");
            format!("({})", inner)
        }
        other => format_node_nested(other),
    }
}

/// Formats the items of a `Sequence` for use inside a `Prec` annotation without extra parens.
///
/// If `node` is a `Sequence`, its items are space-joined directly. Otherwise delegates to
/// `format_node_nested`.
fn format_sequence_items(node: &GrammarNode) -> String {
    match node {
        GrammarNode::Sequence(items) => items
            .iter()
            .map(format_node_nested)
            .collect::<Vec<_>>()
            .join(" "),
        other => format_node_nested(other),
    }
}

/// Builds the `%prec[.left|.right|.dynamic] [level]` annotation string.
fn prec_annotation(kind: &PrecKind, level: Option<u32>) -> String {
    let kw = match kind {
        PrecKind::Plain => "prec",
        PrecKind::Left => "prec.left",
        PrecKind::Right => "prec.right",
        PrecKind::Dynamic => "prec.dynamic",
    };
    match level {
        Some(n) => format!("%{} {}", kw, n),
        None => format!("%{}", kw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{cg, di, p};
    use crate::dom::GrammarNode::{
        self, Alias, Choice, Field, NonTerminal, OneOrMore, Optional, Prec, Sequence,
        TerminalLiteral, TerminalPattern, Token, TokenImmediate, ZeroOrMore,
    };
    use crate::dom::{Grammar, PrecKind};
    use indoc::indoc;

    fn nt(s: &str) -> GrammarNode {
        NonTerminal(s.into())
    }
    fn lit(s: &str) -> GrammarNode {
        TerminalLiteral(s.into())
    }
    fn pat(s: &str) -> GrammarNode {
        TerminalPattern(s.into())
    }

    // ── format_directive ─────────────────────────────────────────────────────

    #[test]
    fn extras_directive_single_item() {
        assert_eq!(
            format_directive("extras", &[di("/\\s/", 1)]),
            "%extras /\\s/"
        );
    }

    #[test]
    fn extras_directive_multiple_items() {
        assert_eq!(
            format_directive("extras", &[di("/\\s/", 1), di("comment", 1)]),
            "%extras /\\s/, comment"
        );
    }

    #[test]
    fn inline_directive() {
        assert_eq!(
            format_directive("inline", &[di("foo", 1), di("bar", 1)]),
            "%inline foo, bar"
        );
    }

    #[test]
    fn supertypes_directive() {
        assert_eq!(
            format_directive("supertypes", &[di("expr", 1), di("stmt", 1)]),
            "%supertypes expr, stmt"
        );
    }

    // ── format_conflicts ─────────────────────────────────────────────────────

    #[test]
    fn conflicts_single_group() {
        assert_eq!(format_conflicts(&[cg(&["a", "b"], 1)]), "%conflicts [a, b]");
    }

    #[test]
    fn conflicts_multiple_groups() {
        assert_eq!(
            format_conflicts(&[cg(&["a", "b"], 1), cg(&["c", "d", "e"], 1)]),
            "%conflicts [a, b], [c, d, e]"
        );
    }

    // ── format_production ────────────────────────────────────────────────────

    #[test]
    fn single_alternative_short_stays_on_one_line() {
        assert_eq!(format_production(&p("rule", nt("foo"))), "rule -> foo;");
    }

    #[test]
    fn two_short_alternatives_stay_on_one_line() {
        assert_eq!(
            format_production(&p("rule", Choice(vec![nt("foo"), nt("bar")]))),
            "rule -> foo | bar;"
        );
    }

    #[test]
    fn long_rule_splits_alternatives() {
        let prod = p(
            "very_long_rule_name",
            Choice(vec![
                nt("alternative_one_which_is_long"),
                nt("alternative_two_which_is_long"),
            ]),
        );
        let out = format_production(&prod);
        assert_eq!(
            out,
            indoc! {"
                very_long_rule_name -> alternative_one_which_is_long
                                     | alternative_two_which_is_long
                                     ;"}
        );
    }

    #[test]
    fn multiline_semicolon_aligns_with_pipe() {
        let prod = p(
            "x",
            Choice(vec![
                nt("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                nt("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            ]),
        );
        let out = format_production(&prod);
        let lines: Vec<&str> = out.lines().collect();
        let pipe_indent = lines[1].find('|').unwrap();
        let semi_indent = lines[2].find(';').unwrap();
        assert_eq!(pipe_indent, semi_indent);
    }

    // ── format_node_nested ───────────────────────────────────────────────────

    #[test]
    fn non_terminal_no_dollar_sign() {
        assert_eq!(format_node_nested(&nt("foo")), "foo");
    }

    #[test]
    fn terminal_literal_passthrough() {
        assert_eq!(format_node_nested(&lit("'x'")), "'x'");
    }

    #[test]
    fn terminal_pattern_passthrough() {
        assert_eq!(format_node_nested(&pat("/a+/")), "/a+/");
    }

    #[test]
    fn optional_simple_no_parens() {
        assert_eq!(format_node_nested(&Optional(Box::new(nt("a")))), "a?");
    }

    #[test]
    fn optional_sequence_gets_parens() {
        assert_eq!(
            format_node_nested(&Optional(Box::new(Sequence(vec![nt("a"), nt("b")])))),
            "(a b)?"
        );
    }

    #[test]
    fn optional_choice_gets_parens() {
        assert_eq!(
            format_node_nested(&Optional(Box::new(Choice(vec![nt("a"), nt("b")])))),
            "(a | b)?"
        );
    }

    #[test]
    fn zero_or_more_simple() {
        assert_eq!(format_node_nested(&ZeroOrMore(Box::new(nt("a")))), "a*");
    }

    #[test]
    fn one_or_more_simple() {
        assert_eq!(format_node_nested(&OneOrMore(Box::new(nt("a")))), "a+");
    }

    #[test]
    fn token_expr() {
        assert_eq!(
            format_node_nested(&Token(Box::new(lit("'x'")))),
            "<< 'x' >>"
        );
    }

    #[test]
    fn token_immediate_expr() {
        assert_eq!(
            format_node_nested(&TokenImmediate(Box::new(lit("'x'")))),
            "<<! 'x' >>"
        );
    }

    #[test]
    fn field_label() {
        assert_eq!(
            format_node_nested(&Field("lhs".into(), Box::new(nt("expr")))),
            "lhs: expr"
        );
    }

    #[test]
    fn alias_group() {
        assert_eq!(
            format_node_nested(&Alias(Box::new(nt("foo")), Box::new(nt("bar")))),
            "(foo => bar)"
        );
    }

    #[test]
    fn prec_top_level_no_parens() {
        assert_eq!(
            format_node_top(&Prec(PrecKind::Left, Some(1), Box::new(nt("a")))),
            "a %prec.left 1"
        );
    }

    #[test]
    fn prec_nested_gets_parens() {
        assert_eq!(
            format_node_nested(&Prec(PrecKind::Left, Some(1), Box::new(nt("a")))),
            "(a %prec.left 1)"
        );
    }

    #[test]
    fn prec_no_level() {
        assert_eq!(
            format_node_top(&Prec(PrecKind::Right, None, Box::new(nt("a")))),
            "a %prec.right"
        );
    }

    // ── format_grammar ───────────────────────────────────────────────────────

    #[test]
    fn grammar_directives_come_before_rules() {
        let mut g = Grammar::from_rules([p("rule", nt("a"))]);
        g.extras = vec![di("/\\s/", 1)];
        let out = format_grammar(&g);
        assert!(out.find("%extras").unwrap() < out.find("rule ->").unwrap());
    }

    #[test]
    fn grammar_canonical_directive_order() {
        let mut g = Grammar::from_rules([p("r", nt("a"))]);
        g.extras = vec![di("/\\s/", 1)];
        g.conflicts = vec![cg(&["a", "b"], 1)];
        g.inline = vec![di("foo", 1)];
        g.supertypes = vec![di("expr", 1)];
        let out = format_grammar(&g);
        assert!(out.find("%extras").unwrap() < out.find("%conflicts").unwrap());
        assert!(out.find("%conflicts").unwrap() < out.find("%inline").unwrap());
        assert!(out.find("%inline").unwrap() < out.find("%supertypes").unwrap());
    }

    #[test]
    fn grammar_blank_line_between_directives_and_rules() {
        let mut g = Grammar::from_rules([p("rule", nt("a"))]);
        g.extras = vec![di("/\\s/", 1)];
        let out = format_grammar(&g);
        assert_eq!(
            out,
            indoc! {"
                %extras /\\s/

                rule -> a;
            "}
        );
    }

    #[test]
    fn grammar_blank_line_between_consecutive_rules() {
        let g = Grammar::from_rules([p("a", nt("x")), p("b", nt("y"))]);
        let out = format_grammar(&g);
        assert_eq!(
            out,
            indoc! {"
                a -> x;

                b -> y;
            "}
        );
    }
}

/// Returns the alternatives of a `Choice` node, or wraps any other node in a one-element vec.
fn flatten_alternatives(node: &GrammarNode) -> Vec<&GrammarNode> {
    match node {
        GrammarNode::Choice(alts) => alts.iter().collect(),
        other => vec![other],
    }
}

/// Formats a single production as BNF text.
///
/// Uses a single line when `name -> body;` fits within 80 characters.
/// Otherwise puts each alternative on its own line with `|` and `;` aligned to the first alternative.
fn format_production(production: &Production) -> String {
    let prefix = format!("{} -> ", production.name);
    let alternatives = flatten_alternatives(&production.body);

    let single = format!(
        "{}{};",
        prefix,
        alternatives
            .iter()
            .map(|a| format_node_top(a))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    if single.len() <= LINE_WIDTH {
        return single;
    }

    let indent = " ".repeat(prefix.len() - 2);
    let mut lines: Vec<String> = Vec::new();
    for (i, alt) in alternatives.iter().enumerate() {
        let body = format_node_top(alt);
        if i == 0 {
            lines.push(format!("{}{}", prefix, body));
        } else {
            lines.push(format!("{}| {}", indent, body));
        }
    }
    lines.push(format!("{};", indent));
    lines.join("\n")
}
