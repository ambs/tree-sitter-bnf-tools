//! Rule-dependency graph builder and DOT/Mermaid emitters for the `graph` subcommand.

use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

use super::types::Grammar;

/// Computes the undefined references and their warning messages for the given edge list.
fn compute_undefined(
    edges: &[(String, String)],
    defined: &HashSet<String>,
) -> (HashSet<String>, Vec<String>) {
    let all_rhs: HashSet<String> = edges.iter().map(|(_, rhs)| rhs.clone()).collect();
    let undefined: HashSet<String> = all_rhs.difference(defined).cloned().collect();
    let mut warnings: Vec<String> = undefined
        .iter()
        .map(|name| format!("warning: rule '{name}' referenced but not defined"))
        .collect();
    warnings.sort_unstable();
    (undefined, warnings)
}

/// Restricts `edges` to the subgraph reachable from `start` (BFS), and returns
/// the filtered edge list together with the subset of `defined` that is reachable.
fn filter_reachable(
    edges: Vec<(String, String)>,
    defined: &HashSet<String>,
    start: &str,
) -> (Vec<(String, String)>, HashSet<String>) {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (lhs, rhs) in &edges {
        adj.entry(lhs.as_str()).or_default().push(rhs.as_str());
    }

    let mut reachable: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    reachable.insert(start.to_string());
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        if let Some(targets) = adj.get(current) {
            for &target in targets {
                if reachable.insert(target.to_string()) {
                    queue.push_back(target);
                }
            }
        }
    }

    let filtered_edges = edges
        .into_iter()
        .filter(|(lhs, _)| reachable.contains(lhs.as_str()))
        .collect();

    let filtered_defined = defined.intersection(&reachable).cloned().collect();

    (filtered_edges, filtered_defined)
}

/// Builds a [`GraphData`] from the grammar, optionally restricting to rules
/// reachable from `start_rule`.
///
/// Returns an error string if `start_rule` is given but does not exist in the grammar.
/// Returns the graph data together with a list of undefined-reference warnings.
pub fn build_graph(
    grammar: &Grammar,
    start_rule: Option<&str>,
) -> Result<(GraphData, Vec<String>), String> {
    let mut defined: HashSet<String> = grammar.productions.keys().cloned().collect();

    let start: String = match start_rule {
        Some(sr) if !defined.contains(sr) => {
            return Err(format!("error: rule '{sr}' not found in grammar"))
        }
        Some(sr) => sr.to_string(),
        None => grammar
            .productions
            .keys()
            .next()
            .map(String::as_str)
            .unwrap_or("")
            .to_string(),
    };

    let mut edges = build_raw_edges(grammar);

    if start_rule.is_some() && !start.is_empty() {
        (edges, defined) = filter_reachable(edges, &defined, &start);
    }

    let (undefined, warnings) = compute_undefined(&edges, &defined);

    Ok((
        GraphData {
            edges,
            start,
            defined,
            undefined,
        },
        warnings,
    ))
}

/// Returns defined rule names that do not appear in any edge and are not the start symbol,
/// sorted for deterministic output.
fn isolated_nodes(data: &GraphData) -> Vec<&str> {
    let in_edges: HashSet<&str> = data
        .edges
        .iter()
        .flat_map(|(a, b)| [a.as_str(), b.as_str()])
        .collect();
    let mut nodes: Vec<&str> = data
        .defined
        .iter()
        .filter(|r| r.as_str() != data.start && !in_edges.contains(r.as_str()))
        .map(String::as_str)
        .collect();
    nodes.sort_unstable();
    nodes
}

/// Emits `data` as a Graphviz DOT digraph string.
pub fn emit_dot(data: &GraphData) -> String {
    let mut lines: Vec<String> = vec!["digraph grammar {".to_string()];

    if !data.start.is_empty() {
        lines.push(format!("  {} [shape=doublecircle];", data.start));
    }

    let mut undef: Vec<&str> = data.undefined.iter().map(String::as_str).collect();
    undef.sort_unstable();
    for name in &undef {
        lines.push(format!("  {} [style=dashed];", name));
    }

    for name in isolated_nodes(data) {
        lines.push(format!("  {};", name));
    }

    for (lhs, rhs) in &data.edges {
        lines.push(format!("  {} -> {};", lhs, rhs));
    }

    lines.push("}".to_string());
    lines.join("\n") + "\n"
}

/// Emits `data` as a Mermaid flowchart string.
pub fn emit_mermaid(data: &GraphData) -> String {
    let mut lines: Vec<String> = vec!["graph TD".to_string()];

    if !data.start.is_empty() {
        lines.push(format!("  {}([\"{}  ★\"])", data.start, data.start));
    }

    let mut undef: Vec<&str> = data.undefined.iter().map(String::as_str).collect();
    undef.sort_unstable();
    for name in &undef {
        lines.push(format!("  {}[\"{} ⚠\"]", name, name));
    }

    for name in isolated_nodes(data) {
        lines.push(format!("  {}", name));
    }

    if !data.edges.is_empty() {
        lines.push(String::new());
    }

    for (lhs, rhs) in &data.edges {
        lines.push(format!("  {} --> {}", lhs, rhs));
    }

    lines.join("\n") + "\n"
}

/// Runs `dot -T<format>` on `dot_input` and returns the rendered bytes.
///
/// Returns an error if `dot` is not found on `PATH` or the process exits non-zero.
pub fn run_graphviz(dot_input: &str, format: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut child = Command::new("dot")
        .arg(format!("-T{format}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| -> Box<dyn Error> {
            if e.kind() == std::io::ErrorKind::NotFound {
                "error: `dot` not found on PATH; install Graphviz: https://graphviz.org/download/"
                    .into()
            } else {
                e.into()
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(dot_input.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dot exited with error:\n{stderr}").into());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{nt, p};
    use crate::dom::{Grammar, GrammarNode};

    /// Convenience: build a two-rule grammar `root -> a ; a -> /x/ ;`.
    fn two_rule_grammar() -> Grammar {
        Grammar::from_rules([
            p("root", GrammarNode::NonTerminal("a".into())),
            p("a", GrammarNode::TerminalPattern("/x/".into())),
        ])
    }

    #[test]
    /// Non-terminal references in rule bodies produce directed edges.
    fn basic_edges() {
        let g = Grammar::from_rules([
            p(
                "expr",
                GrammarNode::Sequence(vec![
                    nt("term"),
                    GrammarNode::TerminalLiteral("'+'".into()),
                    nt("term"),
                ]),
            ),
            p("term", GrammarNode::TerminalPattern("/[0-9]+/".into())),
        ]);
        let (data, _) = build_graph(&g, None).unwrap();
        assert_eq!(data.edges, vec![("expr".to_string(), "term".to_string())]);
    }

    #[test]
    /// A directly recursive rule emits a self-edge.
    fn self_edge() {
        let g = Grammar::from_rules([p(
            "expr",
            GrammarNode::Choice(vec![
                GrammarNode::Sequence(vec![
                    nt("expr"),
                    GrammarNode::TerminalLiteral("'+'".into()),
                    GrammarNode::TerminalPattern("/[0-9]+/".into()),
                ]),
                GrammarNode::TerminalPattern("/[0-9]+/".into()),
            ]),
        )]);
        let (data, _) = build_graph(&g, None).unwrap();
        assert!(data
            .edges
            .contains(&("expr".to_string(), "expr".to_string())));
    }

    #[test]
    /// The start symbol is the first production in declaration order.
    fn start_symbol_is_first_production() {
        let (data, _) = build_graph(&two_rule_grammar(), None).unwrap();
        assert_eq!(data.start, "root");
    }

    #[test]
    /// A non-terminal that is referenced but never defined appears in `undefined` with a warning.
    fn undefined_reference_detected() {
        let g = Grammar::from_rules([p("root", nt("extern_rule"))]);
        let (data, warnings) = build_graph(&g, None).unwrap();
        assert!(data.undefined.contains("extern_rule"));
        assert!(warnings.iter().any(|w| w.contains("extern_rule")));
    }

    #[test]
    /// `--start` restricts the graph to rules reachable from the named rule.
    fn start_filter_prunes_unreachable() {
        let g = Grammar::from_rules([
            p("root", nt("a")),
            p("a", GrammarNode::TerminalPattern("/x/".into())),
            p("unreachable", GrammarNode::TerminalPattern("/y/".into())),
        ]);
        let (data, _) = build_graph(&g, Some("a")).unwrap();
        let lhs_set: HashSet<&str> = data.edges.iter().map(|(l, _)| l.as_str()).collect();
        assert!(!lhs_set.contains("unreachable"));
        assert_eq!(data.start, "a");
    }

    #[test]
    /// `--start` with a rule name not in the grammar returns an error.
    fn start_unknown_rule_returns_error() {
        let g = Grammar::from_rules([p("root", GrammarNode::TerminalPattern("/x/".into()))]);
        assert!(build_graph(&g, Some("missing")).is_err());
    }

    #[test]
    /// An empty grammar produces an empty graph without panicking.
    fn empty_grammar_no_panic() {
        let (data, warnings) = build_graph(&Grammar::new(), None).unwrap();
        assert!(data.edges.is_empty());
        assert!(warnings.is_empty());
        assert!(emit_dot(&data).contains("digraph grammar {"));
    }

    #[test]
    /// The start symbol node carries `shape=doublecircle` in DOT output.
    fn dot_start_is_doublecircle() {
        let (data, _) = build_graph(&two_rule_grammar(), None).unwrap();
        assert!(emit_dot(&data).contains("root [shape=doublecircle]"));
    }

    #[test]
    /// Undefined references carry `style=dashed` in DOT output.
    fn dot_undefined_is_dashed() {
        let g = Grammar::from_rules([p("root", nt("extern_rule"))]);
        let (data, _) = build_graph(&g, None).unwrap();
        assert!(emit_dot(&data).contains("extern_rule [style=dashed]"));
    }

    #[test]
    /// The start symbol node carries the `★` suffix in Mermaid output.
    fn mermaid_start_has_star() {
        let (data, _) = build_graph(&two_rule_grammar(), None).unwrap();
        let mermaid = emit_mermaid(&data);
        assert!(mermaid.contains("★"));
        assert!(mermaid.contains("root"));
    }

    #[test]
    /// Undefined references carry the `⚠` suffix in Mermaid output.
    fn mermaid_undefined_has_warning_symbol() {
        let g = Grammar::from_rules([p("root", nt("extern_rule"))]);
        let (data, _) = build_graph(&g, None).unwrap();
        let mermaid = emit_mermaid(&data);
        assert!(mermaid.contains("⚠") && mermaid.contains("extern_rule"));
    }
}

/// A rule-dependency graph extracted from a BNF grammar.
pub struct GraphData {
    /// Deduplicated directed edges in declaration order: `(from_rule, to_rule)`.
    pub edges: Vec<(String, String)>,
    /// The name of the start (root) rule.
    pub start: String,
    /// Rule names that are defined in the grammar (after any `--start` filtering).
    pub defined: HashSet<String>,
    /// Rule names referenced on a RHS but never defined.
    pub undefined: HashSet<String>,
}

/// Returns all deduplicated `(lhs, rhs)` non-terminal edges for the grammar in declaration order.
fn build_raw_edges(grammar: &Grammar) -> Vec<(String, String)> {
    let mut edge_set: HashSet<(String, String)> = HashSet::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    for (lhs, production) in &grammar.productions {
        let mut seen_rhs: HashSet<&str> = HashSet::new();
        for rhs in production.body.nonterminal_names() {
            if seen_rhs.insert(rhs) {
                let key = (lhs.clone(), rhs.to_string());
                if edge_set.insert(key.clone()) {
                    edges.push(key);
                }
            }
        }
    }
    edges
}
