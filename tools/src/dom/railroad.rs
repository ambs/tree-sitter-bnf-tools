//! Walker that converts a [`GrammarNode`] tree into railroad-diagram combinators.

use std::collections::HashSet;

use railroad::{Comment, Empty, LabeledBox, Link, Node, Optional, Repeat};

use super::nodes::{GrammarNode, PrecKind, PrecLevel};

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

/// Renders the alias-name child of a [`GrammarNode::Alias`] as plain label text.
///
/// The grammar guarantees this node is a [`GrammarNode::NonTerminal`] or a terminal;
/// unlike [`GrammarNode`]'s `Display` impl, no `$.` prefix is added, matching the
/// surface-syntax convention used by `format_node_nested`.
fn alias_label(name: &GrammarNode) -> String {
    match name {
        GrammarNode::NonTerminal(n) => n.clone(),
        GrammarNode::TerminalLiteral(s) | GrammarNode::TerminalPattern(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Renders a `prec[.left|.right|.dynamic](level)` annotation as a tree-sitter-style
/// label, e.g. `"prec.left(2)"` (or a bare `"prec.left"` when there is no level),
/// for use as a [`Comment`] on an annotated node.
fn prec_label(kind: &PrecKind, level: &Option<PrecLevel>) -> String {
    match level {
        Some(n) => format!("{}({n})", kind.as_str()),
        None => kind.as_str().to_string(),
    }
}

/// A CSS override appended after the railroad stylesheet whenever `annotate` is set.
///
/// The crate's own `g.labeledbox > rect` rule paints the box with
/// `fill: rgba(90, 90, 150, .1)`, but Inkscape fails to parse `rgba()` fill values in
/// CSS and falls back to opaque black, turning every annotation box into a black slab
/// that hides its label and contents. This rule re-states the same faint tint in
/// Inkscape-safe syntax (`rgb()` plus `fill-opacity`); appended after
/// [`railroad::DEFAULT_CSS`] it wins the cascade at equal specificity, and browsers
/// render it identically to the crate's original rule.
const ANNOTATION_CSS: &str =
    "svg.railroad g.labeledbox > rect { fill: rgb(90, 90, 150); fill-opacity: .1; }";

/// Wraps `inner` in a [`LabeledBox`] with `text` as a [`Comment`] label above it.
///
/// The railroad stylesheet draws `g.labeledbox > rect` as a grey dashed,
/// near-transparent box ([`ANNOTATION_CSS`] re-states the fill so Inkscape renders it
/// correctly too). `kind` is appended as an `annotation-<kind>` class next to the
/// crate's own `labeledbox` class, giving users a hook to style each annotation kind
/// differently (e.g. colour-code fields vs aliases) without affecting the default
/// rendering.
fn labeled(inner: Box<dyn Node>, text: String, kind: &str) -> Box<dyn Node> {
    let mut boxed = LabeledBox::new(inner, Comment::new(text));
    *boxed.attr("class".to_owned()).or_default() = format!("labeledbox annotation-{kind}");
    Box::new(boxed)
}

/// Recursively converts a [`GrammarNode`] into a boxed railroad [`Node`] combinator.
///
/// Any [`GrammarNode::NonTerminal`] whose name is absent from `defined` still produces a
/// clickable non-terminal box; its name is appended to `warnings` as
/// `"warning: rule 'X' referenced but not defined"`.
///
/// Tree-sitter-specific annotations ([`GrammarNode::Token`],
/// [`GrammarNode::TokenImmediate`], [`GrammarNode::Field`], [`GrammarNode::Alias`],
/// [`GrammarNode::Prec`]) are transparent by default: only the inner body node is
/// rendered. When `annotate` is `true`, each of these is instead wrapped in a
/// [`LabeledBox`] describing the annotation in the dialect's surface syntax: `name:`
/// for a field, `"token"`/`"token.immediate"`, `=> name` for an alias, or a
/// `"prec.left(2)"`-style precedence label.
/// [`GrammarNode::Reserved`] is always transparent — it has no candidate rendering
/// in the issue this flag was added for (#182).
pub fn node_to_railroad(
    node: &GrammarNode,
    mode: &LinkMode,
    defined: &HashSet<String>,
    warnings: &mut Vec<String>,
    annotate: bool,
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
                .map(|c| node_to_railroad(c, mode, defined, warnings, annotate))
                .collect(),
        )),

        GrammarNode::Choice(children) => Box::new(railroad::Choice::new(
            children
                .iter()
                .map(|c| node_to_railroad(c, mode, defined, warnings, annotate))
                .collect(),
        )),

        GrammarNode::Optional(inner) => Box::new(Optional::new(node_to_railroad(
            inner, mode, defined, warnings, annotate,
        ))),

        // One-or-more: traverse inner at least once; backwards arc loops via an Empty separator.
        GrammarNode::OneOrMore(inner) => Box::new(Repeat::new(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            Box::new(Empty) as Box<dyn Node>,
        )),

        // Zero-or-more: same loop as one-or-more, but the whole construct is skippable.
        GrammarNode::ZeroOrMore(inner) => Box::new(Optional::new(Repeat::new(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            Box::new(Empty) as Box<dyn Node>,
        ))),

        // Tree-sitter annotations are transparent unless `annotate` is set.
        GrammarNode::Token(inner)
        | GrammarNode::TokenImmediate(inner)
        | GrammarNode::Field(_, inner)
        | GrammarNode::Alias(inner, _)
        | GrammarNode::Prec(_, _, inner)
            if !annotate =>
        {
            node_to_railroad(inner, mode, defined, warnings, annotate)
        }

        GrammarNode::Token(inner) => labeled(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            "token".to_string(),
            "token",
        ),

        GrammarNode::TokenImmediate(inner) => labeled(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            "token.immediate".to_string(),
            "token-immediate",
        ),

        // Labeled with the dialect's field syntax (`name:`) so a field box is
        // distinguishable from an alias box.
        GrammarNode::Field(name, inner) => labeled(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            format!("{name}:"),
            "field",
        ),

        // Labeled with the dialect's alias syntax (`=> name`); see Field above.
        GrammarNode::Alias(body, name) => labeled(
            node_to_railroad(body, mode, defined, warnings, annotate),
            format!("=> {}", alias_label(name)),
            "alias",
        ),

        GrammarNode::Prec(kind, level, inner) => labeled(
            node_to_railroad(inner, mode, defined, warnings, annotate),
            prec_label(kind, level),
            "prec",
        ),

        // No candidate rendering in issue #182; always transparent.
        GrammarNode::Reserved(_, inner) => {
            node_to_railroad(inner, mode, defined, warnings, annotate)
        }
    }
}

/// Parses `viewBox="0 0 W H"` from a railroad SVG string and returns `(W, H)`.
fn parse_viewbox(svg: &str) -> (i64, i64) {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"viewBox="0 0 ([\d.]+) ([\d.]+)""#)
            .expect("hardcoded viewBox regex is always valid")
    });
    match re.captures(svg) {
        None => (0, 0),
        Some(caps) => {
            let w = caps[1].parse::<f64>().unwrap_or(0.0) as i64;
            let h = caps[2].parse::<f64>().unwrap_or(0.0) as i64;
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
    annotate: bool,
) -> railroad::Sequence<Box<dyn Node>> {
    let body = node_to_railroad(&prod.body, mode, defined, warnings, annotate);
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
/// When `annotate` is `true`, tree-sitter annotations (`Field`, `Token`,
/// `TokenImmediate`, `Alias`, `Prec`) are drawn as labeled boxes; see
/// [`node_to_railroad`].
///
/// # Safety note
/// Rule names are interpolated directly into SVG without escaping.  This is safe
/// because BNF rule names match `[A-Za-z_][A-Za-z0-9_-]*` and contain no XML-special
/// characters.
pub fn emit_single_file(
    grammar: &super::types::Grammar,
    only_rule: Option<&str>,
    annotate: bool,
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
            let seq = production_to_sequence(
                prod,
                &LinkMode::SingleFile,
                &defined,
                &mut warnings,
                annotate,
            );
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
    out.push_str("<style>");
    out.push_str(railroad::DEFAULT_CSS);
    if annotate {
        out.push_str(ANNOTATION_CSS);
    }
    out.push_str("</style>\n");
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
            "<text x=\"10\" y=\"{label_y}\" style=\"font:bold 14px monospace;text-anchor:start\">{name}</text>\n",
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
    warnings.sort_unstable();
    warnings.dedup();
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
/// When `annotate` is `true`, tree-sitter annotations (`Field`, `Token`,
/// `TokenImmediate`, `Alias`, `Prec`) are drawn as labeled boxes; see
/// [`node_to_railroad`].
///
/// # Safety note
/// Rule names are interpolated into file-system paths without sanitisation.  This is
/// safe because BNF rule names match `[A-Za-z_][A-Za-z0-9_-]*` and contain no
/// path-separator or other shell-special characters.
pub fn emit_split(
    grammar: &super::types::Grammar,
    output_dir: &std::path::Path,
    annotate: bool,
) -> Result<Vec<String>, std::io::Error> {
    // Collect defined rule names so the walker can detect undefined references.
    let defined: std::collections::HashSet<String> = grammar.productions.keys().cloned().collect();
    let mut warnings = Vec::new();

    // Create the output directory (and any missing parents) before writing files.
    std::fs::create_dir_all(output_dir)?;

    // Render each rule as a self-contained SVG file.
    for (name, prod) in &grammar.productions {
        let seq = production_to_sequence(prod, &LinkMode::Split, &defined, &mut warnings, annotate);
        // Each file stands alone, so embed the stylesheet for correct rendering.
        let mut diagram = railroad::Diagram::new_with_stylesheet(seq, &railroad::Stylesheet::Light);
        if annotate {
            diagram.add_css(ANNOTATION_CSS);
        }
        std::fs::write(output_dir.join(format!("{name}.svg")), diagram.to_string())?;
    }

    warnings.sort_unstable();
    warnings.dedup();
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
    use crate::dom::{GrammarNode, PrecKind, PrecLevel};

    /// Builds a `HashSet<String>` of defined rule names for use in walker tests.
    fn def(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// Converts `node` to a railroad combinator (annotations off) and renders it to an SVG string.
    fn to_svg(
        node: &GrammarNode,
        mode: &LinkMode,
        defined: &HashSet<String>,
    ) -> (String, Vec<String>) {
        to_svg_annotated(node, mode, defined, false)
    }

    /// Converts `node` to a railroad combinator and renders it to an SVG string,
    /// with `annotate` controlling whether tree-sitter annotations are drawn.
    fn to_svg_annotated(
        node: &GrammarNode,
        mode: &LinkMode,
        defined: &HashSet<String>,
        annotate: bool,
    ) -> (String, Vec<String>) {
        let mut warnings = Vec::new();
        let n = node_to_railroad(node, mode, defined, &mut warnings, annotate);
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
            false,
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
    /// With `--annotate`, Token wraps the inner expression in a LabeledBox with a
    /// "token" comment and a per-kind class for CSS styling hooks.
    fn token_wrapper_is_labeled_when_annotated() {
        let node = GrammarNode::Token(Box::new(GrammarNode::TerminalLiteral("TOK".into())));
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("TOK"));
        assert!(svg.contains("token"));
        assert!(
            svg.contains("labeledbox annotation-token"),
            "annotation box must carry the stylesheet's labeledbox class plus a per-kind class"
        );
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
    /// With `--annotate`, TokenImmediate wraps the inner expression in a LabeledBox
    /// with a "token.immediate" comment.
    fn token_immediate_is_labeled_when_annotated() {
        let node =
            GrammarNode::TokenImmediate(Box::new(GrammarNode::TerminalLiteral("IMM".into())));
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("IMM"));
        assert!(svg.contains("token.immediate"));
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
    /// With `--annotate`, Field wraps the inner expression in a LabeledBox labeled with
    /// the dialect's field syntax (`name:`), so it cannot be mistaken for an alias.
    fn field_wrapper_is_labeled_when_annotated() {
        let node = GrammarNode::Field(
            "myfieldname".into(),
            Box::new(GrammarNode::TerminalLiteral("FLD".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("FLD"));
        assert!(svg.contains("myfieldname:"));
        assert!(svg.contains("labeledbox annotation-field"));
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
    /// With `--annotate`, Alias wraps the body in a LabeledBox labeled with the
    /// dialect's alias syntax (`=> name`; `>` is XML-escaped in the raw SVG), so it
    /// cannot be mistaken for a field.
    fn alias_is_labeled_with_name_when_annotated() {
        let node = GrammarNode::Alias(
            Box::new(GrammarNode::TerminalLiteral("BODY".into())),
            Box::new(GrammarNode::TerminalLiteral("ALIASNAME".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("BODY"));
        assert!(svg.contains("=&gt; ALIASNAME"));
        assert!(svg.contains("labeledbox annotation-alias"));
    }

    #[test]
    /// With `--annotate`, an Alias whose name is a bare rule reference (not a string
    /// literal) is labeled with the plain rule name, not a `$.`-prefixed reference.
    fn alias_nonterminal_name_label_has_no_dollar_prefix() {
        let node = GrammarNode::Alias(
            Box::new(GrammarNode::TerminalLiteral("BODY".into())),
            Box::new(GrammarNode::NonTerminal("renamed".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("renamed"));
        assert!(!svg.contains("$.renamed"));
    }

    #[test]
    /// Prec is a tree-sitter annotation; only the inner expression is rendered.
    fn prec_wrapper_is_transparent() {
        let node = GrammarNode::Prec(
            PrecKind::Left,
            Some(PrecLevel::Integer(1)),
            Box::new(GrammarNode::TerminalLiteral("PRC".into())),
        );
        let (svg, _) = to_svg(&node, &LinkMode::SingleFile, &def(&[]));
        assert!(svg.contains("PRC"));
    }

    #[test]
    /// With `--annotate`, Prec wraps the inner expression in a LabeledBox with a
    /// tree-sitter-style `prec.left(1)` comment.
    fn prec_wrapper_is_labeled_when_annotated() {
        let node = GrammarNode::Prec(
            PrecKind::Left,
            Some(PrecLevel::Integer(1)),
            Box::new(GrammarNode::TerminalLiteral("PRC".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("PRC"));
        assert!(svg.contains("prec.left(1)"));
    }

    #[test]
    /// With `--annotate`, a Prec without a level is labeled with the bare kind name
    /// (`prec`), not an empty call (`prec()`).
    fn prec_without_level_is_labeled_without_parens() {
        let node = GrammarNode::Prec(
            PrecKind::Plain,
            None,
            Box::new(GrammarNode::TerminalLiteral("PRC".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("prec"));
        assert!(!svg.contains("prec()"));
    }

    #[test]
    /// Reserved has no candidate rendering in issue #182 and stays transparent
    /// even with `--annotate`.
    fn reserved_stays_transparent_when_annotated() {
        let node = GrammarNode::Reserved(
            "keywords".into(),
            Box::new(GrammarNode::TerminalLiteral("RSV".into())),
        );
        let (svg, _) = to_svg_annotated(&node, &LinkMode::SingleFile, &def(&[]), true);
        assert!(svg.contains("RSV"));
        assert!(!svg.contains("keywords"));
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
        let (svg, _) = emit_single_file(&two_rule_grammar(), None, false).unwrap();
        assert!(svg.starts_with("<svg"), "output must open with <svg");
        assert!(svg.ends_with("</svg>"), "output must close with </svg>");
    }

    #[test]
    /// Single-file output contains one `id="rule-X"` anchor element per rule (R-12).
    fn single_file_has_one_anchor_per_rule() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None, false).unwrap();
        assert!(svg.contains("id=\"rule-expr\""));
        assert!(svg.contains("id=\"rule-term\""));
    }

    #[test]
    /// Non-terminal hrefs in single-file mode point to anchors that exist in the same document (R-13).
    fn single_file_hrefs_resolve_to_local_anchors() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), None, false).unwrap();
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
        let (svg, _) = emit_single_file(&two_rule_grammar(), None, false).unwrap();
        assert_eq!(
            svg.matches("<style>").count(),
            1,
            "stylesheet must appear exactly once"
        );
    }

    /// Builds a single-rule Grammar fixture whose body is a `field(...)` annotation,
    /// for exercising `--annotate` through the emitters.
    fn field_rule_grammar() -> super::super::types::Grammar {
        use super::super::production::Production;
        super::super::types::Grammar::from_rules([Production {
            name: "expr".into(),
            body: GrammarNode::Field(
                "operand".into(),
                Box::new(GrammarNode::TerminalLiteral("NUM".into())),
            ),
            line: 1,
            filename: "test.bnf".into(),
        }])
    }

    #[test]
    /// `emit_single_file` with `annotate: false` (the default) keeps field annotations transparent.
    fn single_file_annotate_false_omits_field_label() {
        let (svg, _) = emit_single_file(&field_rule_grammar(), None, false).unwrap();
        assert!(svg.contains("NUM"));
        assert!(!svg.contains("operand"));
    }

    #[test]
    /// `emit_single_file` with `annotate: true` draws the field name as a LabeledBox comment.
    fn single_file_annotate_true_shows_field_label() {
        let (svg, _) = emit_single_file(&field_rule_grammar(), None, true).unwrap();
        assert!(svg.contains("NUM"));
        assert!(svg.contains("operand:"));
    }

    #[test]
    /// `emit_single_file` with `annotate: true` embeds the Inkscape-safe fill override:
    /// Inkscape renders the crate stylesheet's `fill: rgba(...)` as opaque black,
    /// hiding the annotation label and contents.
    fn single_file_annotate_true_embeds_inkscape_safe_fill() {
        let (svg, _) = emit_single_file(&field_rule_grammar(), None, true).unwrap();
        assert!(svg.contains(ANNOTATION_CSS));
    }

    #[test]
    /// Without `annotate`, the override CSS is not embedded and output matches the
    /// pre-`--annotate` stylesheet exactly.
    fn single_file_annotate_false_omits_override_css() {
        let (svg, _) = emit_single_file(&field_rule_grammar(), None, false).unwrap();
        assert!(!svg.contains("fill-opacity"));
    }

    // ── emit_single_file --rule ───────────────────────────────────────────────

    #[test]
    /// When only_rule names an existing rule, only that rule's diagram appears in the output.
    fn single_rule_renders_only_named_rule() {
        let (svg, _) = emit_single_file(&two_rule_grammar(), Some("expr"), false).unwrap();
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
        let result = emit_single_file(&two_rule_grammar(), Some("ghost"), false);
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
        let warnings = emit_split(&two_rule_grammar(), &dir, false).unwrap();
        assert!(dir.join("expr.svg").exists(), "expr.svg must be written");
        assert!(dir.join("term.svg").exists(), "term.svg must be written");
        assert!(warnings.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// Each per-rule SVG is a valid standalone SVG with an embedded stylesheet.
    fn split_files_are_standalone_svgs() {
        let dir = make_temp_dir("split_standalone");
        emit_split(&two_rule_grammar(), &dir, false).unwrap();
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
        emit_split(&two_rule_grammar(), &dir, false).unwrap();
        // expr references term, so expr.svg must link to term.svg
        let svg = std::fs::read_to_string(dir.join("expr.svg")).unwrap();
        assert!(
            svg.contains("term.svg"),
            "non-terminal href must point to <name>.svg"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// `emit_split` with `annotate: true` draws field annotations as LabeledBox comments.
    fn split_annotate_true_shows_field_label() {
        let dir = make_temp_dir("split_annotate");
        emit_split(&field_rule_grammar(), &dir, true).unwrap();
        let svg = std::fs::read_to_string(dir.join("expr.svg")).unwrap();
        assert!(svg.contains("NUM"));
        assert!(svg.contains("operand:"));
        assert!(
            svg.contains(ANNOTATION_CSS),
            "split output must embed the Inkscape-safe fill override"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
