//! End-to-end compile-check for the generated typed AST (`--ast-types`,
//! issue #342 phase 3).
//!
//! Mirrors `tools/tests/visitor.rs` file-for-file: [`compile_generated_ast`]
//! renders both [`RustVisitor`] and [`RustAst`] fresh, in-process, right
//! before the check, writes them into a real `cargo` example of this
//! package, and runs `cargo build --example` on it. There is deliberately
//! no checked-in generated `.rs` fixture and no separate Makefile step to
//! keep in sync — regenerating here is exactly as cheap as the
//! derivation+emitter code itself, so nothing can go stale between what's
//! tested and what the current code actually produces (see
//! `compile_generated_visitor`'s own doc comment in `visitor.rs` for the
//! full rationale, which applies unchanged here).
//!
//! The generated `ast.rs` assumes it's a sibling module of the generated
//! `visitor.rs` (`use super::visitor::SourceNode;`, matching the real
//! scaffolded crate's `lib.rs`, which declares `pub mod visitor;` and
//! `pub mod ast;` side by side) — so the harness file wraps each rendered
//! source in its own `mod visitor { .. }` / `mod ast { .. }` block, at the
//! harness file's top level, rather than splicing `ast_source` in
//! unqualified the way `visitor.rs`'s harness does for `trait_source` alone.
//!
//! `visitor_sample.bnf` (the same fixture `sample_grammar_visitor_trait_compiles`
//! in `visitor.rs` uses) has no compiled tree-sitter parser, so this is a
//! compile-check only, never run — matching that test's own scope.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

use ts_bnf_tool::dom::Grammar;
use ts_bnf_tool::dom::ast::BuildMode;
use ts_bnf_tool::dom::ast::merge::MergeConfig;
use ts_bnf_tool::dom::ast::rust::RustAst;
use ts_bnf_tool::dom::visitor::rust::RustVisitor;
use ts_bnf_tool::visitors::parse_source;

/// Removes a harness example file on drop, so a failed assertion (or a
/// panic) still leaves the working tree clean for the next run.
struct HarnessGuard(PathBuf);

impl Drop for HarnessGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Serializes every [`compile_generated_ast`] call in *this* test binary —
/// see `HARNESS_MUTEX`'s own doc comment in `visitor.rs` for why: all
/// callers share the `tools/examples/` directory, and `tools/Cargo.toml`
/// autodiscovers example targets there. A separate static from
/// `visitor.rs`'s own `HARNESS_MUTEX`, since each integration-test file
/// compiles to its own binary with its own statics — this only needs to
/// protect calls within this file.
static HARNESS_MUTEX: Mutex<()> = Mutex::new(());

/// Strict-mode [`compile_generated_ast_in_mode`]: what every test written
/// before `--partial-trees` existed checks.
fn compile_generated_ast(
    example_name: &str,
    grammar: &Grammar,
    rust_name: &str,
    extra_source: &str,
    run: bool,
    merge_config: Option<&MergeConfig>,
) -> Output {
    compile_generated_ast_in_mode(
        example_name,
        grammar,
        rust_name,
        extra_source,
        run,
        merge_config,
        BuildMode::Strict,
    )
}

/// Renders `grammar` via [`RustVisitor`] and [`RustAst`] (with an optional
/// `merge_config` — 342.5.7, in the given [`BuildMode`]), wraps each in its own `mod visitor { .. }` /
/// `mod ast { .. }` block (matching the real scaffolded crate's
/// sibling-module layout `ast.rs`'s `use super::visitor::SourceNode;`
/// depends on), appends `extra_source`, and writes the result into a real
/// `cargo` example of this package named `example_name`. Then runs `cargo
/// build` (`run` when `run` is `true`) `--example <example_name>` and
/// asserts it succeeds — panicking with both captured stdout and stderr
/// otherwise.
fn compile_generated_ast_in_mode(
    example_name: &str,
    grammar: &Grammar,
    rust_name: &str,
    extra_source: &str,
    run: bool,
    merge_config: Option<&MergeConfig>,
    mode: BuildMode,
) -> Output {
    // Declared before `_guard` so it's dropped *after* it (Rust drops
    // locals in reverse declaration order): the lock isn't released until
    // the harness file is gone, not merely until `cargo` exits.
    let _lock = HARNESS_MUTEX
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    let visitor_source = RustVisitor::new(grammar, rust_name, "<test>", true)
        .expect("grammar must be visitor-safe")
        .to_string();
    let ast_source = RustAst::new(grammar, "<test>", true, merge_config, mode)
        .expect("grammar must be ast-safe")
        .to_string();

    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    fs::create_dir_all(&examples_dir)
        .unwrap_or_else(|e| panic!("must be able to create {}: {e}", examples_dir.display()));
    let harness_path = examples_dir.join(format!("{example_name}.rs"));
    let _guard = HarnessGuard(harness_path.clone());

    let program = format!(
        "mod visitor {{\n{visitor_source}\n}}\n\nmod ast {{\n{ast_source}\n}}\n\n{extra_source}"
    );
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
        "freshly generated AST types for '{example_name}' failed to compile or run:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    output
}

/// Compile-check: the small synthetic `visitor_sample.bnf` fixture
/// exercises fields (`target:`/`value:`), a leaf kind, and the "unlabeled
/// top-level choice" known v1 limitation (`expr -> ident | num ;`) — no
/// compiled tree-sitter parser exists for this made-up grammar, so this
/// only `cargo build`s it (`run: false`), proving the emitted AST types
/// type-check against the real `tree_sitter` crate; they can't be executed
/// without a real parser behind them.
#[test]
fn sample_grammar_ast_types_compile() {
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

    compile_generated_ast(
        "ast_sample_harness",
        &grammar,
        "sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Compile-check for a `--merge-config`-driven grammar (342.5.7): three
/// leaf kinds merged into one `pub enum`, and a fourth `passthrough`-renamed
/// — proving the `#[allow(private_interfaces)]`/`#[allow(dead_code)]`
/// placement (`plans/342.5.md`'s `rustc`-verified addendum) is correct in
/// real generated output, not just in hand-written unit-test string
/// assertions. Specifically asserts **no `private_interfaces` warning**
/// (the lint the whole privacy design exists to suppress): the empirical
/// regression guard for that promise. Not a blanket zero-warnings
/// assertion — this harness's own `fn main() {}` deliberately never
/// constructs any of the generated types, so ordinary `dead_code`/
/// `missing_docs` warnings on the unused harness itself are expected noise,
/// unrelated to this phase's `#[allow(...)]` placement.
#[test]
fn sample_grammar_ast_types_with_merge_config_compiles_without_private_interfaces_warning() {
    let source = "program -> for_statement | while_statement | repeat_statement | comment ;\n\
                   for_statement -> 'for' ;\n\
                   while_statement -> 'while' ;\n\
                   repeat_statement -> 'repeat' ;\n\
                   comment -> /#.*/ ;\n";
    let (grammar, diagnostics) =
        parse_source(source).unwrap_or_else(|e| panic!("merge-config fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "merge-config fixture must be diagnostic-free: {diagnostics:?}"
    );

    let config: MergeConfig = ts_bnf_tool::dom::parse_merge_config(
        r#"
            [[merge]]
            target = "Loop"
            from = ["for_statement", "while_statement", "repeat_statement"]

            [[passthrough]]
            kind = "comment"
            target = "DocComment"
        "#,
    )
    .expect("inline merge-config TOML must parse");

    let output = compile_generated_ast(
        "ast_merge_config_harness",
        &grammar,
        "merge_sample",
        "fn main() {}\n",
        false,
        Some(&config),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("private_interfaces"),
        "generated AST types with a merge config must not trigger `private_interfaces`:\n{stderr}"
    );
}

/// Compile-check for a repeated anonymous-token field (#357): a field
/// that's both `multiple` and untyped derives `Vec<String>`
/// (`field_rust_type`), so its `TryFrom` body must collect `.text()`, not
/// `.try_into()` a `String` (no `TryFrom<SourceNode> for String` exists).
/// Regression guard for the case no other compile-checked fixture covers —
/// `visitor_sample.bnf`'s one repeated field is unlabeled and derives no
/// field at all.
#[test]
fn repeated_anonymous_token_field_compiles() {
    let source = "program -> ops: ('+' | '-')* ;\n";
    let (grammar, diagnostics) = parse_source(source)
        .unwrap_or_else(|e| panic!("repeated anonymous-token fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "repeated anonymous-token fixture must be diagnostic-free: {diagnostics:?}"
    );

    compile_generated_ast(
        "ast_repeated_anonymous_token_harness",
        &grammar,
        "repeated_token_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Compile-check for a field labeled with a Rust keyword (#358): `type:`
/// is a plausible label (types, loops, etc.) that, before this fix, was
/// emitted verbatim as `pub type: Ident,` / `let type = ...` — both
/// invalid Rust (`type` is a keyword). `rust_field_ident` now escapes it as
/// a raw identifier (`r#type`) in every identifier position, while
/// `.child_by_field("type")` and the `BuildError::MissingField` message
/// keep referencing the grammar's actual, unescaped field name.
#[test]
fn keyword_field_label_compiles() {
    let source = "stmt -> type: ident ';' ;\nident -> /[a-z]+/ ;\n";
    let (grammar, diagnostics) = parse_source(source)
        .unwrap_or_else(|e| panic!("keyword-field-label fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "keyword-field-label fixture must be diagnostic-free: {diagnostics:?}"
    );

    compile_generated_ast(
        "ast_keyword_field_label_harness",
        &grammar,
        "keyword_field_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Compile-check for grammar kinds literally named after the tool's own
/// fixed top-level names (#359): a kind named `pragma`, `text`,
/// `build_error`, or `source_node` derives a top-level `struct Pragma`,
/// `struct Text`, `struct BuildError`, or `struct SourceNode` respectively.
/// Before the `runtime`-module namespacing fix, `Pragma`/`BuildError` were
/// declared unqualified at `ast.rs`'s own top level and `SourceNode` was
/// imported unqualified from `visitor.rs`, so any of these four kind names
/// would collide with the tool's own fixed type (`E0428`) or shadow the
/// import. Now `Pragma`/`BuildError` live under `runtime::`, and every
/// reference to the real tree-sitter-backed type is spelled out as
/// `super::visitor::SourceNode`, so a grammar-derived `struct SourceNode`
/// at the top level can never collide with it.
#[test]
fn kind_named_after_tool_fixed_names_compiles() {
    let source = "root -> a: pragma b: text c: build_error d: source_node ;\n\
                  pragma -> 'p' ;\n\
                  text -> 't' ;\n\
                  build_error -> 'b' ;\n\
                  source_node -> 's' ;\n";
    let (grammar, diagnostics) =
        parse_source(source).unwrap_or_else(|e| panic!("fixed-name-kind fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "fixed-name-kind fixture must be diagnostic-free: {diagnostics:?}"
    );

    compile_generated_ast(
        "ast_fixed_name_kind_harness",
        &grammar,
        "fixed_name_kind_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Compile-check crossing all four field shapes — single named, single
/// anonymous-token, repeated named (`Vec<T>`), repeated anonymous-token —
/// with leaf/non-leaf kind status (#363, split from #356 item 8). A leaf
/// kind's body can never reference a visible non-terminal (`is_leaf_body`),
/// so a leaf can only carry token-shaped fields; the real cross is those
/// four shapes on `branch`
/// (non-leaf) plus the two token shapes again on `leaf_kind` (still a
/// leaf, since `Field` is transparent to leaf-ness). Before #357 and #359
/// were fixed, no compile-checked fixture combined a `Vec<T>` named-kind
/// field with a leaf kind carrying an explicit field label, so this gap
/// let both ship broken; this fixture is the regression guard for that
/// combination going forward.
#[test]
fn field_shapes_cross_leaf_status_compile() {
    let source = "root -> branch leaf_kind ;\n\
                  branch -> single: ident tok: '+' items: ident* ops: ('+' | '-')* ;\n\
                  leaf_kind -> single_tok: '#' many_tok: ('a' | 'b')* ;\n\
                  ident -> /[a-z]+/ ;\n";
    let (grammar, diagnostics) = parse_source(source)
        .unwrap_or_else(|e| panic!("field-shapes-cross-leaf-status fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "field-shapes-cross-leaf-status fixture must be diagnostic-free: {diagnostics:?}"
    );

    compile_generated_ast(
        "ast_field_shapes_cross_leaf_status_harness",
        &grammar,
        "field_shapes_cross_leaf_status_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Compile-check for grammar fields literally labeled `pragma` or `text`
/// (#359): every generated struct unconditionally gets an injected
/// `_pragma: runtime::Pragma` field, and a leaf kind also gets an injected
/// `_text: String` field. Before the injected fields were renamed with a
/// leading underscore, a field labeled `pragma` or `text` verbatim produced
/// a duplicate field declaration, `let` binding, and struct-literal entry
/// (`E0124`). The leading-underscore reservation
/// (`check_field_labels_are_representable`) means `pragma`/`text` remain
/// completely free for grammar authors to use as field labels.
#[test]
fn field_labeled_pragma_or_text_compiles() {
    let source = "root -> decl | other ;\n\
                  decl -> pragma: ident ';' ;\n\
                  other -> text: ident ';' ;\n\
                  ident -> /[a-z]+/ ;\n";
    let (grammar, diagnostics) = parse_source(source)
        .unwrap_or_else(|e| panic!("pragma/text field-label fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "pragma/text field-label fixture must be diagnostic-free: {diagnostics:?}"
    );

    compile_generated_ast(
        "ast_pragma_text_field_label_harness",
        &grammar,
        "pragma_text_field_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Reads and parses `grammar/bnf.bnf` — the dialect's own grammar, the one
/// grammar this crate has a compiled tree-sitter parser for
/// (`tree_sitter_bnf::LANGUAGE`), so generated AST code for it can actually
/// be run against real trees.
fn dogfood_grammar() -> Grammar {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../grammar/bnf.bnf");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    let (grammar, diagnostics) =
        parse_source(&source).unwrap_or_else(|e| panic!("grammar/bnf.bnf must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "grammar/bnf.bnf must be diagnostic-free: {diagnostics:?}"
    );
    grammar
}

/// Behaviour test for the generated `runtime::Pragma`: its `column` counts
/// characters, not bytes (tree-sitter's own column is a byte offset), and a
/// leading byte-order mark is not counted. Runs the freshly generated
/// `Pragma::from` against trees the real BNF parser produces, so it checks
/// the emitted code's behaviour, not its text. Each expected `(line, column)`
/// is spelled out by hand: the byte column that tree-sitter would report is
/// noted where it differs.
#[test]
fn pragma_column_counts_characters_and_ignores_a_leading_bom() {
    let runner = r#"
use tree_sitter::Parser;
use visitor::SourceNode;

/// `Pragma` of the smallest node covering the first `needle` in `source`,
/// as `(line, column)`.
fn position_of(source: &str, needle: &str) -> (usize, usize) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = parser.parse(source, None).expect("sample must parse");
    let at = source.find(needle).expect("needle must occur in source");
    let node = tree
        .root_node()
        .descendant_for_byte_range(at, at + needle.len())
        .expect("a node must cover the needle");
    assert_eq!(node.start_byte(), at, "needle {needle:?} must start a node");
    let pragma = ast::runtime::Pragma::from(SourceNode { node, source });
    (pragma.line, pragma.column)
}

fn main() {
    // Plain ASCII: unchanged.
    assert_eq!(position_of("a -> b ;\n", "b"), (1, 6));

    // Four two-byte characters before `b`: 12 characters, 16 bytes
    // (tree-sitter's own column would be 17).
    assert_eq!(position_of("a -> 'çãçã' b ;\n", "b"), (1, 13));

    // The multibyte text is on line 2: the line start is found per line.
    // 9 characters, 10 bytes before `w` (byte column 11).
    assert_eq!(position_of("x -> y ;\nz -> 'é' w ;\n", "w"), (2, 10));

    // A leading BOM (three bytes, one character) is not part of line 1.
    assert_eq!(position_of("\u{feff}a -> b ;\n", "a"), (1, 1));
    assert_eq!(position_of("\u{feff}a -> b ;\n", "b"), (1, 6));

    // ... and does not touch later lines.
    assert_eq!(position_of("\u{feff}a -> b ;\nc -> d ;\n", "c"), (2, 1));

    // BOM and multibyte characters together: 10 characters before `b`.
    assert_eq!(position_of("\u{feff}a -> 'çã' b ;\n", "b"), (1, 11));
}
"#;

    compile_generated_ast(
        "ast_pragma_char_column_harness",
        &dogfood_grammar(),
        "bnf",
        runner,
        true,
        None,
    );
}

/// Parses `source`, asserting it is a valid, diagnostic-free grammar.
fn parse_fixture(what: &str, source: &str) -> Grammar {
    let (grammar, diagnostics) =
        parse_source(source).unwrap_or_else(|e| panic!("{what} fixture must parse: {e}"));
    assert!(
        diagnostics.is_empty(),
        "{what} fixture must be diagnostic-free: {diagnostics:?}"
    );
    grammar
}

/// Asserts that compiling a generated `ast.rs` printed neither of the
/// warnings the partial-mode emitter is specifically responsible for
/// avoiding: an unused `diags`/`node` parameter, and a `private_interfaces`
/// leak from a merged-away kind. Not a blanket zero-warnings check — the
/// harness's own `fn main() {}` never constructs any generated type, so
/// ordinary `dead_code` noise is expected (see the merge-config test above).
fn assert_no_partial_mode_warnings(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    for warning in ["unused variable", "private_interfaces"] {
        assert!(
            !stderr.contains(warning),
            "generated partial-mode AST must not trigger `{warning}`:\n{stderr}"
        );
    }
}

/// Partial-mode counterpart of [`sample_grammar_ast_types_compile`]
/// (`--partial-trees`): the same small fixture, rendered with `Option`
/// fields and `Recover` impls instead of `TryFrom`.
#[test]
fn sample_grammar_partial_ast_types_compile() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/visitor_sample.bnf");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
    let grammar = parse_fixture("visitor_sample.bnf", &source);

    let output = compile_generated_ast_in_mode(
        "ast_partial_sample_harness",
        &grammar,
        "partial_sample",
        "fn main() {}\n",
        false,
        None,
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Partial-mode counterpart of the `--merge-config` compile-check: merged-away
/// kinds stay private yet still get a `Recover` impl, the merge entry's enum
/// dispatches to them, and nothing leaks a private type.
#[test]
fn partial_ast_types_with_merge_config_compile() {
    let grammar = parse_fixture(
        "partial merge-config",
        "program -> for_statement | while_statement | repeat_statement | comment ;\n\
         for_statement -> 'for' ;\n\
         while_statement -> 'while' ;\n\
         repeat_statement -> 'repeat' ;\n\
         comment -> /#.*/ ;\n",
    );
    let config: MergeConfig = ts_bnf_tool::dom::parse_merge_config(
        r#"
            [[merge]]
            target = "Loop"
            from = ["for_statement", "while_statement", "repeat_statement"]

            [[passthrough]]
            kind = "comment"
            target = "DocComment"
        "#,
    )
    .expect("inline merge-config TOML must parse");

    let output = compile_generated_ast_in_mode(
        "ast_partial_merge_config_harness",
        &grammar,
        "partial_merge_sample",
        "fn main() {}\n",
        false,
        Some(&config),
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Partial-mode compile-check crossing every field shape — single named,
/// single token, repeated named, repeated token, a multi-shape enum with and
/// without a bare-token variant — on both a non-leaf kind and a leaf kind
/// that carries only token fields (and so has no use for `_diags`).
#[test]
fn partial_field_shapes_cross_leaf_status_compile() {
    let grammar = parse_fixture(
        "partial field-shapes",
        "root -> branch leaf_kind ;\n\
         branch -> single: ident tok: '+' items: ident* ops: ('+' | '-')* \
                   value: (ident | num | 'nil') many: (ident | num)* ;\n\
         leaf_kind -> single_tok: '#' many_tok: ('a' | 'b')* ;\n\
         ident -> /[a-z]+/ ;\n\
         num -> /[0-9]+/ ;\n",
    );

    let output = compile_generated_ast_in_mode(
        "ast_partial_field_shapes_harness",
        &grammar,
        "partial_field_shapes_sample",
        "fn main() {}\n",
        false,
        None,
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Partial-mode compile-check for names that would collide with the emitter's
/// own, were it not namespaced: kinds named after the runtime's own types
/// (`diagnostic`, `recovered`, `recover`, `runtime`) and after the strict
/// mode's fixed names (`pragma`, `text`, `source_node`), and field labels
/// that are local variables inside the generated `recover` (`node`, `diags`)
/// or Rust keywords (`type`). Partial-only: a field labelled `node` is a
/// separate, pre-existing problem in strict mode's `try_from`, which names
/// its own parameter `node`.
#[test]
fn partial_ast_naming_edge_cases_compile() {
    let grammar = parse_fixture(
        "partial naming edge cases",
        "root -> a: pragma b: text c: diagnostic d: recovered e: recover f: runtime \
                 g: source_node node: ident diags: ident type: ident ;\n\
         pragma -> 'p' ;\n\
         text -> 't' ;\n\
         diagnostic -> 'd' ;\n\
         recovered -> 'r' ;\n\
         recover -> 'c' ;\n\
         runtime -> 'u' ;\n\
         source_node -> 's' ;\n\
         ident -> /[a-z]+/ ;\n",
    );

    let output = compile_generated_ast_in_mode(
        "ast_partial_naming_edge_cases_harness",
        &grammar,
        "partial_naming_sample",
        "fn main() {}\n",
        false,
        None,
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}
