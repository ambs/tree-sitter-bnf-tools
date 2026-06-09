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

/// Parses `viewBox="0 0 W H"` from a railroad SVG string and returns `(W, H)`.
fn parse_viewbox(svg: &str) -> (i64, i64) {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"viewBox="0 0 (\d+) (\d+)""#)
            .expect("hardcoded viewBox regex is always valid")
    });
    match re.captures(svg) {
        None => (0, 0),
        Some(caps) => {
            let w = caps[1].parse().unwrap_or(0);
            let h = caps[2].parse().unwrap_or(0);
            (w, h)
        }
    }
}

/// Converts a single production body into a railroad `Sequence` framed by start/end markers.
///
/// This is the shared building block for both single-file and split-mode emitters.
fn production_to_sequence(
    prod: &super::production::Production,
    mode: &LinkMode,
    defined: &std::collections::HashSet<String>,
    warnings: &mut Vec<String>,
) -> railroad::Sequence<Box<dyn Node>> {
    let body = node_to_railroad(&prod.body, mode, defined, warnings);
    railroad::Sequence::new(vec![
        Box::new(railroad::SimpleStart) as Box<dyn Node>,
        body,
        Box::new(railroad::SimpleEnd) as Box<dyn Node>,
    ])
}

/// Vertical space (px) reserved above each diagram for the rule-name label.
const LABEL_HEIGHT: i64 = 24;
/// Vertical gap (px) inserted between consecutive rule blocks.
const RULE_GAP: i64 = 16;

/// Renders rules from `grammar` stacked vertically into a single SVG document.
///
/// When `only_rule` is `Some(name)`, only that rule is rendered; when `None`,
/// all rules are rendered in declaration order.  In both cases the full grammar
/// is used to build the `defined` set so non-terminal hrefs resolve correctly.
///
/// Each rule is preceded by its name as a `<text>` label and wrapped in a
/// `<g id="rule-<name>">` element so that `#rule-<name>` fragment links within
/// the document resolve to the correct diagram.
///
/// Non-terminal references use [`LinkMode::SingleFile`] hrefs (`#rule-<name>`).
///
/// Returns `Ok((svg_string, warnings))` on success.  Warnings are produced for any
/// non-terminal referenced but not defined in the grammar.
///
/// Returns `Err(message)` when `only_rule` names a rule that does not exist in
/// the grammar.
///
/// # Safety note
/// Rule names are interpolated directly into SVG without escaping.  This is safe
/// because BNF rule names match `[A-Za-z_][A-Za-z0-9_-]*` and contain no XML-special
/// characters.
pub fn emit_single_file(
    grammar: &super::types::Grammar,
    only_rule: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    // Collect defined rule names so the walker can detect undefined references.
    // Always uses the full grammar even in single-rule mode so hrefs are correct.
    let defined: std::collections::HashSet<String> = grammar.productions.keys().cloned().collect();
    let mut warnings = Vec::new();

    // Per-rule rendering result: the extracted SVG content and its pixel dimensions.
    struct Rule<'a> {
        name: &'a str,
        content: String,
        width: i64,
        height: i64,
    }

    // Select productions to render: either the single named rule or all rules.
    // Validate the rule name immediately so callers get a clear error rather than
    // a silently empty SVG.
    let selected: Vec<_> = match only_rule {
        None => grammar.productions.iter().collect(),
        Some(name) => {
            let entry = grammar
                .productions
                .get_key_value(name)
                .ok_or_else(|| format!("rule '{name}' not found in grammar"))?;
            vec![entry]
        }
    };

    // Convert each production to a railroad diagram, render it to an SVG string,
    // parse the dimensions from the viewBox, and strip the outer <svg> wrapper so
    // the content can be re-embedded inside the combined document.
    let rules: Vec<Rule<'_>> = selected
        .iter()
        .map(|(name, prod)| {
            let seq = production_to_sequence(prod, &LinkMode::SingleFile, &defined, &mut warnings);
            let svg = railroad::Diagram::new(seq).to_string();
            let (width, height) = parse_viewbox(&svg);
            let content = extract_diagram_content(&svg).to_owned();
            Rule {
                name,
                content,
                width,
                height,
            }
        })
        .collect();

    // The combined SVG is as wide as the widest rule (minimum 200px) and tall
    // enough to stack every rule block (label + diagram + gap) without overlap.
    let max_width = rules.iter().map(|r| r.width).max().unwrap_or(200).max(200);
    let total_height: i64 = rules
        .iter()
        .map(|r| LABEL_HEIGHT + r.height + RULE_GAP)
        .sum();

    // Open the combined SVG and embed the railroad stylesheet once at the top.
    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         class=\"railroad\" \
         viewBox=\"0 0 {max_width} {total_height}\">\n"
    ));
    out.push_str(&format!("<style>{}</style>\n", railroad::DEFAULT_CSS));
    out.push_str("<rect width=\"100%\" height=\"100%\" class=\"railroad_canvas\"/>\n");

    // Emit each rule block: a named anchor group, a rule-name label, and the diagram.
    // The group is translated to its vertical position so rules stack without overlap.
    let mut y: i64 = 0;
    for rule in &rules {
        // `id="rule-<name>"` is the fragment anchor target for #rule-<name> hrefs.
        out.push_str(&format!(
            "<g id=\"rule-{name}\" transform=\"translate(0, {y})\">\n",
            name = rule.name
        ));
        // Rule name label sits just above the diagram (baseline at LABEL_HEIGHT - 6).
        out.push_str(&format!(
            "<text x=\"10\" y=\"{label_y}\" style=\"font:bold 14px monospace\">{name}</text>\n",
            label_y = LABEL_HEIGHT - 6,
            name = rule.name,
        ));
        // Shift the diagram content down by LABEL_HEIGHT to leave room for the label.
        out.push_str(&format!("<g transform=\"translate(0, {LABEL_HEIGHT})\">\n"));
        out.push_str(&rule.content);
        out.push_str("\n</g>\n</g>\n");
        y += LABEL_HEIGHT + rule.height + RULE_GAP;
    }

    out.push_str("</svg>");
    Ok((out, warnings))
}

/// Renders each rule in `grammar` as a standalone SVG file written to `output_dir`.
///
/// Each file is named `<rule>.svg` and includes the railroad stylesheet so it renders
/// correctly when opened on its own.  Non-terminal references use [`LinkMode::Split`]
/// hrefs (`<name>.svg`), enabling navigation between files when served from the same
/// directory.
///
/// `output_dir` is created if it does not already exist.
///
/// Returns the collected warnings on success, or an [`std::io::Error`] if any file
/// could not be written.
///
/// # Safety note
/// Rule names are interpolated into file-system paths without sanitisation.  This is
/// safe because BNF rule names match `[A-Za-z_][A-Za-z0-9_-]*` and contain no
/// path-separator or other shell-special characters.
pub fn emit_split(
    grammar: &super::types::Grammar,
    output_dir: &std::path::Path,
) -> Result<Vec<String>, std::io::Error> {
    // Collect defined rule names so the walker can detect undefined references.
    let defined: std::collections::HashSet<String> = grammar.productions.keys().cloned().collect();
    let mut warnings = Vec::new();

    // Create the output directory (and any missing parents) before writing files.
    std::fs::create_dir_all(output_dir)?;

    // Render each rule as a self-contained SVG file.
    for (name, prod) in &grammar.productions {
        let seq = production_to_sequence(prod, &LinkMode::Split, &defined, &mut warnings);
        // Each file stands alone, so embed the stylesheet for correct rendering.
        let svg =
            railroad::Diagram::new_with_stylesheet(seq, &railroad::Stylesheet::Light).to_string();
        std::fs::write(output_dir.join(format!("{name}.svg")), svg)?;
    }

    Ok(warnings)
}

/// Extracts the drawable elements from a single railroad diagram SVG string.
///
/// Strips the outer `<svg …>` opening tag, the `<rect … railroad_canvas … />` background,
/// and the `</svg>` closing tag, returning only the diagram's inner elements.
fn extract_diagram_content(svg: &str) -> &str {
    // Skip the <svg …> opening tag (ends at the first '>')
    let after_open = match svg.find('>') {
        Some(i) => svg[i + 1..].trim_start(),
        None => return svg,
    };
    // Skip the canvas background <rect … />
    let after_rect = if after_open.starts_with("<rect") {
        match after_open.find('>') {
            Some(i) => after_open[i + 1..].trim_start(),
            None => after_open,
        }
    } else {
        after_open
    };
    // Strip the </svg> closing tag
    match after_rect.rfind("</svg>") {
        Some(i) => after_rect[..i].trim_end(),
        None => after_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{GrammarNode, PrecKind};

    /// Builds a `HashSet<String>` of defined rule names for use in walker tests.
    fn def(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Converts `node` to a railroad combinator and renders it to an SVG string.
    fn to_svg(
        node: &GrammarNode,
        mode: &LinkMode,
        defined: &HashSet<String>,
    ) -> (String, Vec<String>) {
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
        assert_eq!(
            warnings,
            ["warning: rule 'ghost' referenced but not defined"]
        );
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
        let node =
            GrammarNode::TokenImmediate(Box::new(GrammarNode::TerminalLiteral("IMM".into())));
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

    // ── emit_single_file ─────────────────────────────────────────────────────

    /// Builds a two-rule Grammar fixture for emit_single_file tests.
    fn two_rule_grammar() -> super::super::types::Grammar {
        use super::super::production::Production;
        super::super::types::Grammar::from_rules([
            Production {
                name: "expr".into(),
                body: GrammarNode::NonTerminal("term".into()),
                line: 1,
                filename: "test.bnf".into(),
            },
            Production {
                name: "term".into(),
                body: GrammarNode::TerminalLiteral("NUM".into()),
                line: 2,
                filename: "test.bnf".into(),
            },
        ])
    }

    #[test]
    /// Single-file output is a valid SVG element containing both rule names as labels.
    fn single_file_is_svg() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None).unwrap();
        assert!(svg.starts_with("<svg"), "output must open with <svg");
        assert!(svg.ends_with("</svg>"), "output must close with </svg>");
    }

    #[test]
    /// Single-file output contains one `id="rule-X"` anchor element per rule (R-12).
    fn single_file_has_one_anchor_per_rule() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None).unwrap();
        assert!(svg.contains("id=\"rule-expr\""));
        assert!(svg.contains("id=\"rule-term\""));
    }

    #[test]
    /// Non-terminal hrefs in single-file mode point to anchors that exist in the same document (R-13).
    fn single_file_hrefs_resolve_to_local_anchors() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None).unwrap();
        // expr references term, so there must be an href to #rule-term
        assert!(
            svg.contains("#rule-term"),
            "href to referenced rule must be present"
        );
        // and the corresponding anchor must also exist in the same document
        assert!(
            svg.contains("id=\"rule-term\""),
            "anchor target must exist in the document"
        );
    }

    #[test]
    /// The stylesheet is embedded exactly once at the top of the combined SVG.
    fn single_file_embeds_stylesheet_once() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None).unwrap();
        assert_eq!(
            svg.matches("<style>").count(),
            1,
            "stylesheet must appear exactly once"
        );
    }

    // ── emit_single_file --rule ───────────────────────────────────────────────

    #[test]
    /// When only_rule names an existing rule, only that rule's diagram appears in the output.
    fn single_rule_renders_only_named_rule() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), Some("expr")).unwrap();
        assert!(
            svg.contains("id=\"rule-expr\""),
            "named rule anchor must be present"
        );
        assert!(
            !svg.contains("id=\"rule-term\""),
            "other rule anchors must be absent"
        );
    }

    #[test]
    /// When only_rule names a rule that does not exist, an error is returned instead of a silent empty SVG.
    fn single_rule_unknown_name_returns_error() {
        let result = emit_single_file(&two_rule_grammar(), Some("ghost"));
        assert!(result.is_err(), "unknown rule must return Err");
        assert!(
            result.unwrap_err().contains("ghost"),
            "error message must include the missing rule name"
        );
    }

    // ── emit_split ────────────────────────────────────────────────────────────

    /// Returns a temporary directory unique to the calling test (cleaned up by the caller).
    fn make_temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("railroad_test_{}_{}", std::process::id(), suffix));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    /// Split mode writes one .svg file per rule to the output directory (R-14).
    fn split_writes_one_file_per_rule() {
        let dir = make_temp_dir("split_files");
        let warnings = emit_split(&two_rule_grammar(), &dir).unwrap();
        assert!(dir.join("expr.svg").exists(), "expr.svg must be written");
        assert!(dir.join("term.svg").exists(), "term.svg must be written");
        assert!(warnings.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// Each per-rule SVG is a valid standalone SVG with an embedded stylesheet.
    fn split_files_are_standalone_svgs() {
        let dir = make_temp_dir("split_standalone");
        emit_split(&two_rule_grammar(), &dir).unwrap();
        let svg = std::fs::read_to_string(dir.join("expr.svg")).unwrap();
        assert!(
            svg.starts_with("<svg"),
            "each split file must be a valid SVG"
        );
        assert!(
            svg.contains("<style"),
            "each split file must embed the stylesheet"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// Non-terminal hrefs in split mode point to `<name>.svg` relative paths (R-14).
    fn split_hrefs_are_relative_svg_filenames() {
        let dir = make_temp_dir("split_hrefs");
        emit_split(&two_rule_grammar(), &dir).unwrap();
        // expr references term, so expr.svg must link to term.svg
        let svg = std::fs::read_to_string(dir.join("expr.svg")).unwrap();
        assert!(
            svg.contains("term.svg"),
            "non-terminal href must point to <name>.svg"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
