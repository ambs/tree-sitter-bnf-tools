//! End-to-end tests for the generated ANTLR-style `Visitor<'tree>` trait
//! (issue #210).
//!
//! Every test in this file that needs to know whether the *emitted* trait
//! actually compiles (and, where relevant, behaves correctly at runtime)
//! goes through one path: [`compile_generated_visitor`] renders
//! [`RustVisitor`] fresh, in-process, right before the check, writes it
//! into a real `cargo` example of this package, and runs `cargo build` or
//! `cargo run --example` on it. There is deliberately no checked-in
//! generated `.rs` fixture and no separate Makefile step to keep in sync —
//! regenerating here is exactly as cheap as the derivation+emitter code
//! itself, so nothing can go stale between what's tested and what the
//! current code actually produces.
//!
//! A real example under `examples/` (rather than a standalone temp crate)
//! is used so the check reuses this workspace's already-resolved
//! dependency graph and build cache for `tree-sitter`/`tree-sitter-bnf`
//! (whose C parser needs `cc`-driven native linking), with no manual
//! rlib-path lookup. The example file is written immediately before each
//! `cargo` invocation and removed again right after (see [`HarnessGuard`]),
//! so it never lingers as a stray untracked file; it's also listed in
//! `.gitignore` as a backstop.
//!
//! Two grammars feed this:
//!
//! - `visitor_sample.bnf` — a small, self-contained synthetic grammar
//!   (round 2 finding 4) exercising fields, a multi-arm dispatcher, and
//!   leaf/non-leaf bodies. No compiled tree-sitter parser exists for this
//!   made-up grammar, so its trait is compile-checked only, never run.
//! - `grammar/bnf.bnf` — the real grammar this dialect uses to describe
//!   itself. Because `tree_sitter_bnf::LANGUAGE` is a real compiled parser
//!   *for exactly this grammar*, its generated trait can also be run: fed a
//!   parsed `.bnf` sample and driven by an actual `Visitor` implementation.
//!
//! The dogfood *sync* tests below don't need either: they check the
//! derivation layer's output directly against
//! `tree-sitter-bnf/src/node-types.json`, the ground truth for what
//! tree-sitter itself considers this grammar's kinds.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ts_bnf_tool::dom::{Grammar, RustVisitor, resolve_field_target_kinds, visible_kinds};
use ts_bnf_tool::visitors::parse_source;

/// Reads `grammar/bnf.bnf`'s raw source text.
fn dogfood_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../grammar/bnf.bnf");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// Parses `grammar/bnf.bnf` into the DOM `Grammar`, the dogfood grammar this
/// dialect uses to describe itself.
fn dogfood_grammar() -> Grammar {
    let source = dogfood_source();
    let (grammar, diagnostics) =
        parse_source(&source).unwrap_or_else(|e| panic!("grammar/bnf.bnf must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "grammar/bnf.bnf must be diagnostic-free: {diagnostics:?}"
    );
    grammar
}

/// Reads and parses `tree-sitter-bnf/src/node-types.json` — tree-sitter's
/// own ground truth for what this dialect's real, generated parser
/// considers each kind's shape to be.
fn node_types() -> Vec<serde_json::Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tree-sitter-bnf/src/node-types.json");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} must be valid JSON: {e}", path.display()));
    parsed
        .as_array()
        .expect("node-types.json must be a JSON array")
        .clone()
}

/// Returns the set of `type` names from `entries` that are named,
/// non-supertype kinds — a supertype entry has a `subtypes` array instead of
/// `fields`/`children` and represents the umbrella, not a kind a real node
/// ever reports via `node.kind()`.
fn node_types_named_non_supertype_kinds(entries: &[serde_json::Value]) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|entry| entry["named"].as_bool() == Some(true) && entry.get("subtypes").is_none())
        .map(|entry| {
            entry["type"]
                .as_str()
                .expect("named entry must have a string 'type'")
                .to_string()
        })
        .collect()
}

/// Returns the set of `type` names listed under `kind`'s `field`, per
/// `entries` — the ground-truth answer to "what kinds can this field's
/// value be?", straight from tree-sitter's own `generate` output.
fn node_types_field_types(
    entries: &[serde_json::Value],
    kind: &str,
    field: &str,
) -> BTreeSet<String> {
    let entry = entries
        .iter()
        .find(|e| e["type"].as_str() == Some(kind))
        .unwrap_or_else(|| panic!("node-types.json must contain an entry for '{kind}'"));
    entry["fields"][field]["types"]
        .as_array()
        .unwrap_or_else(|| panic!("'{kind}' must have a '{field}' field with a 'types' array"))
        .iter()
        .map(|t| {
            t["type"]
                .as_str()
                .expect("field type entry must have a string 'type'")
                .to_string()
        })
        .collect()
}

/// Dogfood sync test (210.29): the kind set [`visible_kinds`] derives for
/// `grammar/bnf.bnf` must exactly match `tree-sitter-bnf/src/node-types.json`'s
/// named, non-supertype kinds — the two are supposed to describe the exact
/// same tree, one from our own derivation, the other from tree-sitter's
/// real `generate` output. `grammar/bnf.bnf` declares no `%supertypes` of
/// its own (confirmed empirically: no `node-types.json` entry has a
/// `subtypes` array), so this test's non-supertype filtering is exercised
/// vacuously here — [`Grammar::is_hidden_rule`]'s supertype-membership
/// clause has its own direct unit tests in `grammar.rs`.
#[test]
fn dogfood_visible_kinds_match_node_types_json() {
    let grammar = dogfood_grammar();
    let derived: BTreeSet<String> = visible_kinds(&grammar).into_iter().collect();
    let expected = node_types_named_non_supertype_kinds(&node_types());
    assert_eq!(
        derived, expected,
        "visible_kinds(grammar/bnf.bnf) must equal node-types.json's named, non-supertype kinds"
    );
}

/// Dogfood spot-check (210.30): the real 9-way field-kind union that
/// motivated round 2 finding 3's "fully resolve the union" decision.
///
/// `grammar/bnf.bnf` documents its own limitation (see its header comment):
/// field-name annotations like `content:` are omitted throughout for
/// readability, even though the real `tree-sitter-bnf/grammar.js` does
/// label `symbol`'s content field (`field("content", $._symbolContent)`).
/// So there is no `symbol.content`-labeled field in our own parsed DOM to
/// run [`resolve_field_target_kinds`] on directly — this resolves
/// `_symbolContent`'s body instead, exactly what a `content:` label *would*
/// wrap if the self-description used one, and checks it against
/// `node-types.json`'s real `symbol.content` field, which does exist
/// (`tree-sitter-bnf/grammar.js` is the actual source of truth compiled
/// into the parser).
#[test]
fn dogfood_symbol_content_field_union_matches_node_types_json() {
    let grammar = dogfood_grammar();
    let body = &grammar
        .productions
        .get("_symbolContent")
        .expect("grammar/bnf.bnf must declare _symbolContent")
        .body;
    let resolved = resolve_field_target_kinds(&grammar, body);

    let expected = node_types_field_types(&node_types(), "symbol", "content");
    assert_eq!(
        resolved.named.iter().cloned().collect::<BTreeSet<_>>(),
        expected,
        "_symbolContent's resolved union must match symbol.content's real field types"
    );
    assert!(
        !resolved.anonymous_token,
        "every symbol.content alternative resolves to a named kind; none should reach a bare token"
    );
    assert_eq!(
        expected.len(),
        9,
        "sanity check: this is meant to be the real 9-way union case from round 2 finding 3"
    );
}

/// Removes a harness example file on drop, so a failed assertion (or a
/// panic) still leaves the working tree clean for the next run.
struct HarnessGuard(PathBuf);

impl Drop for HarnessGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Renders `grammar` via [`RustVisitor`] and writes the result, plus
/// whatever `extra_source` supplies (typically a `struct`/`impl Visitor`
/// and a `fn main`), into a real `cargo` example of this package named
/// `example_name`. Then runs `cargo build` (`run` when `run` is `true`)
/// `--example <example_name>` and asserts it succeeds — panicking with
/// both captured stdout and stderr otherwise, since a panic inside the
/// generated program's own `main` is exactly the failure mode this is
/// meant to catch.
///
/// See the module doc comment for why this writes into `examples/` rather
/// than a standalone temp crate, and why nothing here is checked in.
fn compile_generated_visitor(
    example_name: &str,
    grammar: &Grammar,
    rust_name: &str,
    extra_source: &str,
    run: bool,
) -> Output {
    let trait_source = RustVisitor {
        grammar,
        name: rust_name,
        source: "<test>",
        no_header: true,
    }
    .to_string();

    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    fs::create_dir_all(&examples_dir)
        .unwrap_or_else(|e| panic!("must be able to create {}: {e}", examples_dir.display()));
    let harness_path = examples_dir.join(format!("{example_name}.rs"));
    let _guard = HarnessGuard(harness_path.clone());

    let program = format!("{trait_source}\n{extra_source}");
    fs::write(&harness_path, program)
        .unwrap_or_else(|e| panic!("must be able to write {}: {e}", harness_path.display()));

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let subcommand = if run { "run" } else { "build" };
    let output = Command::new(&cargo)
        .args([subcommand, "--quiet", "--example", example_name])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .output()
        .unwrap_or_else(|e| {
            panic!("failed to spawn `{cargo} {subcommand} --example {example_name}`: {e}")
        });

    assert!(
        output.status.success(),
        "freshly generated Visitor trait for '{example_name}' failed to compile or run:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    output
}

/// Compile-check (round 2 finding 4): the small synthetic
/// `visitor_sample.bnf` fixture exercises fields (`target:`/`value:`), a
/// leaf kind, and a multi-arm dispatcher — coverage the dogfood grammar
/// can't provide on its own, since `grammar/bnf.bnf` deliberately has zero
/// `field:` annotations. No compiled tree-sitter parser exists for this
/// made-up grammar, so [`compile_generated_visitor`] is asked only to
/// `cargo build` it (`run: false`), proving the emitted trait type-checks
/// against the real `tree_sitter` crate; it can't be executed.
#[test]
fn sample_grammar_visitor_trait_compiles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visitor_sample.bnf");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    let (grammar, diagnostics) =
        parse_source(&source).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    assert!(
        diagnostics.is_empty(),
        "{} must be diagnostic-free: {diagnostics:?}",
        path.display()
    );

    compile_generated_visitor(
        "visitor_sample_harness",
        &grammar,
        "sample",
        "fn main() {}\n",
        false,
    );
}

/// Compile + behavior test: renders the dogfood `grammar/bnf.bnf`'s trait
/// fresh, compiles and runs it via `cargo run --example`, and confirms a
/// `RuleCounter` visitor's count of `rule` nodes over `grammar/bnf.bnf`'s
/// own source (itself valid `.bnf` text, so it doubles as the sample to
/// parse and visit — no separate hand-written sample grammar needed)
/// matches an independent direct tree walk. This proves the *emitted*
/// trait behaves correctly at runtime — dispatch and `children_visitor`
/// recursion actually reach every `rule` node — for genuinely
/// generated-this-run code.
#[test]
fn dogfood_visitor_trait_runs_and_counts_rule_nodes() {
    let grammar = dogfood_grammar();
    let sample_source = dogfood_source();

    let runner = format!(
        r#"
struct RuleCounter {{
    count: usize,
}}

impl<'t> Visitor<'t> for RuleCounter {{
    type Output = ();
    type Error = std::convert::Infallible;

    fn combine(&mut self, _results: Vec<()>) -> Result<(), Self::Error> {{
        Ok(())
    }}

    fn visit_rule(&mut self, node: Node<'t>) -> Result<(), Self::Error> {{
        self.count += 1;
        self.children_visitor(node)
    }}
}}

fn count_rule_nodes_directly(node: Node) -> usize {{
    let mut cursor = node.walk();
    let mut count = usize::from(node.kind() == "rule");
    for child in node.children(&mut cursor) {{
        count += count_rule_nodes_directly(child);
    }}
    count
}}

fn main() {{
    let source = {sample_source:?};
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = parser.parse(source, None).expect("harness sample must parse");

    let expected = count_rule_nodes_directly(tree.root_node());
    assert!(expected > 0, "harness sample must declare at least one rule");

    let mut counter = RuleCounter {{ count: 0 }};
    counter
        .visit(tree.root_node())
        .expect("Infallible visitor must not fail");

    assert_eq!(
        counter.count, expected,
        "RuleCounter must count exactly as many `rule` nodes as a direct tree walk finds"
    );
}}
"#
    );

    compile_generated_visitor("visitor_dogfood_harness", &grammar, "bnf", &runner, true);
}

/// Compile + behavior test: confirms a synthetically `MISSING` node reaches
/// [`Visitor::missing_visitor`] rather than silently flowing through normal
/// kind dispatch (`node.kind()` reports the *expected* kind on a `MISSING`
/// node, not a distinct missing kind — round 2 finding 7). `"start -> ;\n"`
/// (a rule body with nothing at all after `->`) makes tree-sitter's own
/// error recovery insert a `MISSING pattern` node as `symbol`'s `content`
/// value; since `pattern` is a named kind, that node surfaces through
/// ordinary `named_children` traversal like any other child, and a
/// `MissingSpy` visitor detects it by overriding `missing_visitor`.
#[test]
fn dogfood_visitor_trait_reaches_missing_visitor_for_missing_node() {
    let grammar = dogfood_grammar();

    let runner = r#"
struct MissingSpy {
    reached_missing: bool,
}

impl<'t> Visitor<'t> for MissingSpy {
    type Output = ();
    type Error = std::convert::Infallible;

    fn combine(&mut self, _results: Vec<()>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn missing_visitor(&mut self, node: Node<'t>) -> Result<(), Self::Error> {
        assert!(
            node.is_missing(),
            "missing_visitor must only ever be called for a MISSING node"
        );
        self.reached_missing = true;
        self.default_result()
    }
}

fn main() {
    let source = "start -> ;\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = parser.parse(source, None).expect("harness sample must parse");

    let mut spy = MissingSpy {
        reached_missing: false,
    };
    spy.visit(tree.root_node())
        .expect("Infallible visitor must not fail");

    assert!(
        spy.reached_missing,
        "MissingSpy must reach missing_visitor for the synthetically MISSING `pattern` node"
    );
}
"#;

    compile_generated_visitor("visitor_missing_harness", &grammar, "bnf", runner, true);
}
