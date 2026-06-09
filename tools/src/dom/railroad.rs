//! Walker that converts a [`GrammarNode`] tree into railroad-diagram combinators.

use std::collections::HashSet;

use railroad::{Empty, Link, Node, Optional, Repeat};

use super::nodes::GrammarNode;

/// Controls how non-terminal cross-reference hrefs are generated in the SVG output.
pub enum LinkMode {
    /// All rule diagrams share one SVG document; hrefs use `#rule-<name>` fragment anchors.
    SingleFile,
    /// Each rule lives in its own SVG file; hrefs use `<name>.svg` relative paths.
    Split,
}

impl LinkMode {
    /// Returns the href string for a clickable link targeting the named rule.
    fn href_for(&self, name: &str) -> String {
        match self {
            LinkMode::SingleFile => format!("#rule-{name}"),
            LinkMode::Split => format!("{name}.svg"),
        }
    }
}

/// Recursively converts a [`GrammarNode`] into a boxed railroad [`Node`] combinator.
///
/// Any [`GrammarNode::NonTerminal`] whose name is absent from `defined` still produces a
/// clickable non-terminal box; its name is appended to `warnings` as
/// `"warning: rule 'X' referenced but not defined"`.
///
/// Tree-sitter-specific annotations ([`GrammarNode::Token`],
/// [`GrammarNode::TokenImmediate`], [`GrammarNode::Field`], [`GrammarNode::Alias`],
/// [`GrammarNode::Prec`]) are transparent: only the inner body node is rendered.
pub fn node_to_railroad(
    node: &GrammarNode,
    mode: &LinkMode,
    defined: &HashSet<String>,
    warnings: &mut Vec<String>,
) -> Box<dyn Node> {
    match node {
        GrammarNode::TerminalLiteral(s) | GrammarNode::TerminalPattern(s) => {
            Box::new(railroad::Terminal::new(s.clone()))
        }

        GrammarNode::NonTerminal(name) => {
            if !defined.contains(name) {
                warnings.push(format!("warning: rule '{name}' referenced but not defined"));
            }
            Box::new(Link::new(
                railroad::NonTerminal::new(name.clone()),
                mode.href_for(name),
            ))
        }

        GrammarNode::Sequence(children) => Box::new(railroad::Sequence::new(
            children
                .iter()
                .map(|c| node_to_railroad(c, mode, defined, warnings))
                .collect(),
        )),

        GrammarNode::Choice(children) => Box::new(railroad::Choice::new(
            children
                .iter()
                .map(|c| node_to_railroad(c, mode, defined, warnings))
                .collect(),
        )),

        GrammarNode::Optional(inner) => Box::new(Optional::new(node_to_railroad(
            inner, mode, defined, warnings,
        ))),

        // One-or-more: traverse inner at least once; backwards arc loops via an Empty separator.
        GrammarNode::OneOrMore(inner) => Box::new(Repeat::new(
            node_to_railroad(inner, mode, defined, warnings),
            Box::new(Empty) as Box<dyn Node>,
        )),

        // Zero-or-more: same loop as one-or-more, but the whole construct is skippable.
        GrammarNode::ZeroOrMore(inner) => Box::new(Optional::new(Repeat::new(
            node_to_railroad(inner, mode, defined, warnings),
            Box::new(Empty) as Box<dyn Node>,
        ))),

        // Tree-sitter annotations have no visual equivalent; render only the inner expression.
        GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Prec(_, _, inner) => node_to_railroad(inner, mode, defined, warnings),

        GrammarNode::Field(_, inner) | GrammarNode::Alias(inner, _) => {
            node_to_railroad(inner, mode, defined, warnings)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{GrammarNode, PrecKind};

    fn def(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Converts `node` to a railroad combinator and renders it to an SVG string.
    fn to_svg(node: &GrammarNode, mode: &LinkMode, defined: &HashSet<String>) -> (String, Vec<String>) {
        let mut warnings = Vec::new();
        let n = node_to_railroad(node, mode, defined, &mut warnings);
        let svg = railroad::Diagram::new(n).to_string();
        (svg, warnings)
    }

    #[test]
    /// A literal terminal produces a Terminal box whose label appears in the SVG.
    fn terminal_literal_renders() {
        let (svg, w) = to_svg(
            &GrammarNode::TerminalLiteral("NUM".into()),
            &LinkMode::SingleFile,
            &def(&[]),
        );
        assert!(svg.starts_with("<svg"), "output must be an SVG element");
        assert!(svg.contains("NUM"));
        assert!(w.is_empty());
    }

    #[test]
    /// A regex pattern terminal produces a Terminal box whose pattern text appears in the SVG.
    fn terminal_pattern_renders() {
        let (svg, w) = to_svg(
            &GrammarNode::TerminalPattern("DIGITS".into()),
            &LinkMode::SingleFile,
            &def(&[]),
        );
        assert!(svg.contains("DIGITS"));
        assert!(w.is_empty());
    }

    #[test]
    /// A NonTerminal in single-file mode gets an href pointing to `#rule-<name>`.
    fn nonterminal_single_file_href() {
        let (svg, w) = to_svg(
            &GrammarNode::NonTerminal("expr".into()),
            &LinkMode::SingleFile,
            &def(&["expr"]),
        );
        assert!(svg.contains("#rule-expr"));
        assert!(w.is_empty());
    }

    #[test]
    /// A NonTerminal in split mode gets an href pointing to `<name>.svg`.
    fn nonterminal_split_href() {
        let (svg, w) = to_svg(
            &GrammarNode::NonTerminal("expr".into()),
            &LinkMode::Split,
            &def(&["expr"]),
        );
        assert!(svg.contains("expr.svg"));
        assert!(w.is_empty());
    }

    #[test]
    /// A NonTerminal whose name is not in `defined` still renders but appends a warning.
    fn nonterminal_undefined_warns() {
        let mut warnings = Vec::new();
        node_to_railroad(
            &GrammarNode::NonTerminal("ghost".into()),
            &LinkMode::SingleFile,
            &def(&[]),
            &mut warnings,
        );
        assert_eq!(warnings, ["warning: rule 'ghost' referenced but not defined"]);
    }

    #[test]
    /// A Sequence renders all its children; both labels appear in the SVG.
    fn sequence_renders_children() {
        let node = GrammarNode::Sequence(vec![
            GrammarNode::TerminalLiteral("AA".into()),
            GrammarNode::TerminalLiteral("BB".into()),
        ]);
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("AA") && svg.contains("BB"));
    }

    #[test]
    /// A Choice renders all its alternatives; both labels appear in the SVG.
    fn choice_renders_alternatives() {
        let node = GrammarNode::Choice(vec![
            GrammarNode::TerminalLiteral("XX".into()),
            GrammarNode::TerminalLiteral("YY".into()),
        ]);
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("XX") && svg.contains("YY"));
    }

    #[test]
    /// An Optional wraps its inner node; the inner label appears in the SVG.
    fn optional_renders_inner() {
        let node = GrammarNode::Optional(Box::new(GrammarNode::TerminalLiteral("OPT".into())));
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("OPT"));
    }

    #[test]
    /// OneOrMore renders the inner node at least once (Repeat with Empty separator).
    fn one_or_more_renders_inner() {
        let node = GrammarNode::OneOrMore(Box::new(GrammarNode::TerminalLiteral("ITEM".into())));
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("ITEM"));
    }

    #[test]
    /// ZeroOrMore renders the inner node (Optional + Repeat with Empty separator).
    fn zero_or_more_renders_inner() {
        let node = GrammarNode::ZeroOrMore(Box::new(GrammarNode::TerminalLiteral("ELEM".into())));
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("ELEM"));
    }

    #[test]
    /// Token is a tree-sitter annotation; only the inner expression is rendered.
    fn token_wrapper_is_transparent() {
        let node = GrammarNode::Token(Box::new(GrammarNode::TerminalLiteral("TOK".into())));
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("TOK"));
    }

    #[test]
    /// TokenImmediate is a tree-sitter annotation; only the inner expression is rendered.
    fn token_immediate_is_transparent() {
        let node = GrammarNode::TokenImmediate(Box::new(GrammarNode::TerminalLiteral("IMM".into())));
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("IMM"));
    }

    #[test]
    /// Field is a tree-sitter annotation; the inner expression is rendered but the field name is not.
    fn field_wrapper_is_transparent() {
        let node = GrammarNode::Field(
            "myfieldname".into(),
            Box::new(GrammarNode::TerminalLiteral("FLD".into())),
        );
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("FLD"));
        assert!(!svg.contains("myfieldname"));
    }

    #[test]
    /// Alias renders the body expression; the alias name node is discarded and must not appear.
    fn alias_renders_body_not_name() {
        let node = GrammarNode::Alias(
            Box::new(GrammarNode::TerminalLiteral("BODY".into())),
            Box::new(GrammarNode::TerminalLiteral("ALIASNAME".into())),
        );
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("BODY"));
        assert!(!svg.contains("ALIASNAME"));
    }

    #[test]
    /// Prec is a tree-sitter annotation; only the inner expression is rendered.
    fn prec_wrapper_is_transparent() {
        let node = GrammarNode::Prec(
            PrecKind::Left,
            Some(1),
            Box::new(GrammarNode::TerminalLiteral("PRC".into())),
        );
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("PRC"));
    }
}
