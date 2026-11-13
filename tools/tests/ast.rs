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

    // The generated `ast.rs` has a `parse_ast` that calls the crate's own
    // `parse` (the scaffolded `lib.rs` defines it), which a bare harness file
    // does not have. Most fixtures here have no compiled parser, so they only
    // need it to type-check: a stand-in that always fails. A runner that
    // brings a real `parse` of its own (the ones that drive the BNF parser)
    // is left alone.
    let parse_stand_in = if extra_source.contains("fn parse(") {
        ""
    } else {
        "#[allow(dead_code)]\n\
         fn parse(_source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {\n    \
             Err(\"this harness has no compiled parser\".into())\n}\n\n"
    };
    let program = format!(
        "mod visitor {{\n{visitor_source}\n}}\n\nmod ast {{\n{ast_source}\n}}\n\n{parse_stand_in}{extra_source}"
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
/// or Rust keywords (`type`) — and kinds literally named `error` and `token`,
/// whose children-enum variants would collide with the fixed `Error` and
/// `Token` ones if only the fixed ones were named, and are renamed instead.
/// Partial-only: a field labelled `node` is a separate, pre-existing problem
/// in strict mode's `try_from`, which names its own parameter `node`.
#[test]
fn partial_ast_naming_edge_cases_compile() {
    let grammar = parse_fixture(
        "partial naming edge cases",
        "root -> a: pragma b: text c: diagnostic d: recovered e: recover f: runtime \
                 g: source_node node: ident diags: ident type: ident h: holder ;\n\
         holder -> error token 'x' ;\n\
         error -> /e/ ;\n\
         token -> /t/ ;\n\
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

/// Partial-mode compile-check for a grammar with no field labels at all, the
/// case that motivated unlabelled-children capture: every struct would be
/// `_pragma`-only without `_children`. Covers a single child kind, per-kind
/// child enums, string literals beside named kinds (`Token(String)`
/// variants), a kind whose only unlabelled children are literals
/// (`Vec<String>`), and a literal-only leaf (no `_children` at all), all
/// type-checked against the real `tree_sitter` crate, with no warnings from
/// the generated code.
#[test]
fn partial_unlabelled_children_compile() {
    let grammar = parse_fixture(
        "partial unlabelled children",
        "program -> stmt* ;\n\
         stmt -> decl | assign | call ;\n\
         decl -> 'var' ident ';' ;\n\
         assign -> ident '=' expr op expr ';' ;\n\
         call -> name: ident '(' ')' ';' ;\n\
         expr -> ident | num ;\n\
         op -> '+' | '-' ;\n\
         ident -> /[a-z]+/ ;\n\
         num -> /[0-9]+/ ;\n",
    );

    let output = compile_generated_ast_in_mode(
        "ast_partial_unlabelled_children_harness",
        &grammar,
        "partial_unlabelled_sample",
        "fn main() {}\n",
        false,
        None,
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Partial-mode compile-check for unlabelled children meeting
/// `--merge-config`: a three-kind child enum that includes merged-away
/// (private) structs, a list holding a single merged-away struct directly
/// (needs `#[allow(private_interfaces)]` on its `pub` owner), a child set
/// that is exactly one merge entry's `from`, so it reuses that entry's enum,
/// and a literal beside a merged-away kind (an enum with a `Token` variant
/// wrapping a private struct).
#[test]
fn partial_unlabelled_children_with_merge_config_compile() {
    let grammar = parse_fixture(
        "partial unlabelled children with merge config",
        "root -> program block loops braces ;\n\
         program -> (for_statement | while_statement | helper)* ;\n\
         block -> for_statement* ;\n\
         loops -> (for_statement | while_statement)* ;\n\
         braces -> '{' for_statement* '}' ;\n\
         for_statement -> 'for' ;\n\
         while_statement -> 'while' ;\n\
         helper -> 'h' ;\n",
    );
    let config: MergeConfig = ts_bnf_tool::dom::parse_merge_config(
        r#"
            [[merge]]
            target = "Loop"
            from = ["for_statement", "while_statement"]
        "#,
    )
    .expect("inline merge-config TOML must parse");

    let output = compile_generated_ast_in_mode(
        "ast_partial_unlabelled_children_merge_harness",
        &grammar,
        "partial_unlabelled_merge_sample",
        "fn main() {}\n",
        false,
        Some(&config),
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Compile-check for the shape that used to be rejected with `kind 'x' and
/// shape [x, $token] both generate 'X'` (strict mode, labelled fields): two
/// fields with different names sharing a "kind or literal" shape. The shared
/// enum is named `<Kind>OrToken` so it can't clash with the kind's own struct.
#[test]
fn shared_kind_plus_literal_fields_compile() {
    let grammar = parse_fixture(
        "shared kind-plus-literal fields",
        "root -> a b ;\n\
         a -> p: (c | 'x') ;\n\
         b -> q: (c | 'x') ;\n\
         c -> /[a-z]/ ;\n",
    );

    compile_generated_ast(
        "ast_shared_kind_plus_literal_fields_harness",
        &grammar,
        "shared_kind_literal_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Partial-mode counterpart, with the grammar shape that was actually
/// reported: two string rules (`'..'` and `".."`) whose unlabelled children
/// are the same character kind plus a quote literal, so they share one child
/// enum that used to be named exactly like the character kind's struct.
#[test]
fn partial_shared_kind_plus_literal_children_compile() {
    let grammar = parse_fixture(
        "partial shared kind-plus-literal children",
        "root -> single_string double_string ;\n\
         single_string -> 'a' char* 'a' ;\n\
         double_string -> 'b' char* 'b' ;\n\
         char -> /[a-z]/ ;\n",
    );

    let output = compile_generated_ast_in_mode(
        "ast_partial_shared_children_harness",
        &grammar,
        "partial_shared_children_sample",
        "fn main() {}\n",
        false,
        None,
        BuildMode::Partial,
    );
    assert_no_partial_mode_warnings(&output);
}

/// Behaviour test for the generated `runtime::Reflect` (`--partial-trees`):
/// the typed tree it exposes is the real tree-sitter tree.
///
/// Generates partial-mode types for `grammar/bnf.bnf`, parses the real
/// `bnf.bnf` (and a small multibyte sample) with the compiled BNF parser, and
/// walks the typed tree with an explicit stack — the "one generic, iterative
/// loop" the trait exists for — pairing every node with tree-sitter's own
/// node. For each pair it checks: the kind, `is_named`, the byte range, the
/// start and end line/column (column counted in characters, worked out here
/// independently of the generated code), the leaf text, the child count, and
/// that children are in source order, don't overlap and sit inside their
/// parent. Extras (the grammar's comments) are not part of any rule's shape,
/// so they are left out of what tree-sitter's side is expected to match. It
/// also checks a clean tree gives no diagnostics, which would otherwise hide
/// a kind the derivation missed.
#[test]
fn reflect_walk_mirrors_the_real_tree_sitter_tree() {
    let runner = r#"
use ast::runtime::{recover, walk, Reflect};
use tree_sitter::{Node, Parser};
use visitor::SourceNode;

/// 1-indexed (line, character column) of byte offset `at`, worked out here,
/// independently of the generated code.
fn line_col(source: &str, at: usize) -> (usize, usize) {
    let before = &source[..at];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, source[line_start..at].chars().count() + 1)
}

/// What tree-sitter says a node's children are, each with the field name it
/// reports for it, minus extras and `MISSING` points. An `ERROR` node stays —
/// tree-sitter flags the ones it creates while skipping tokens as extras too,
/// so they are let through explicitly.
fn expected_children<'a>(node: Node<'a>) -> Vec<(Option<&'a str>, Node<'a>)> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return out;
    }
    loop {
        let child = cursor.node();
        if (child.is_error() || !child.is_extra()) && !child.is_missing() {
            out.push((cursor.field_name(), child));
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    out
}

/// The kinds of typed children, for a failure message.
fn typed_kinds(children: &[(Option<&'static str>, &dyn Reflect)]) -> Vec<String> {
    let mut out = Vec::new();
    for (field, child) in children {
        out.push(format!("{}{}", field.map(|f| format!("{f}:")).unwrap_or_default(), child.kind()));
    }
    out
}

/// The kinds of tree-sitter's children, for a failure message.
fn real_kinds(children: &[(Option<&str>, Node<'_>)]) -> Vec<String> {
    let mut out = Vec::new();
    for (field, child) in children {
        out.push(format!("{}{}", field.map(|f| format!("{f}:")).unwrap_or_default(), child.kind()));
    }
    out
}

/// Walks the typed tree for `source` against tree-sitter's, returning
/// `(nodes visited, literal tokens, leaves, labelled children)`. With `broken`
/// the source must have syntax errors: the typed tree is then expected to keep
/// every error region in place, matching tree-sitter's `ERROR` nodes one for
/// one. Every child's field label must be tree-sitter's own. Along the way it
/// records the pre-order sequence of `(depth, field, kind, bytes)` and checks
/// `runtime::walk` yields exactly that.
fn check(source: &str, broken: bool) -> (usize, usize, usize, usize) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = parser.parse(source, None).expect("sample must parse");
    assert_eq!(tree.root_node().has_error(), broken, "sample's error state");

    let recovered = recover::<ast::Grammar>(SourceNode { node: tree.root_node(), source });
    assert_eq!(
        recovered.diagnostics.is_empty(),
        !broken,
        "a clean tree gives no diagnostics and a broken one at least one: {:?}",
        recovered.diagnostics
    );
    let root = recovered.value.expect("the root must be built");

    let (mut visited, mut tokens, mut leaves, mut labelled) = (0, 0, 0, 0);
    let mut sequence: Vec<(usize, Option<&'static str>, String, std::ops::Range<usize>)> = Vec::new();
    let mut stack: Vec<(&dyn Reflect, Node, usize, Option<&'static str>)> =
        vec![(&root, tree.root_node(), 0, None)];
    while let Some((typed, real, depth, field)) = stack.pop() {
        visited += 1;
        sequence.push((depth, field, typed.kind().to_string(), typed.span().byte_range.clone()));
        let what = format!("`{}` at bytes {:?}", real.kind(), real.byte_range());

        assert_eq!(typed.kind(), real.kind(), "kind of {what}");
        assert_eq!(typed.is_named(), real.is_named(), "is_named of {what}");

        let span = typed.span();
        assert_eq!(span.byte_range, real.byte_range(), "byte range of {what}");
        assert_eq!(
            (span.start.line, span.start.column),
            line_col(source, real.start_byte()),
            "start of {what}"
        );
        assert_eq!(
            (span.end.line, span.end.column),
            line_col(source, real.end_byte()),
            "end of {what}"
        );

        let children = typed.children();
        if let Some(text) = typed.text() {
            assert_eq!(text, &source[real.byte_range()], "text of {what}");
            if !real.is_named() {
                tokens += 1;
            }
            if children.is_empty() {
                // A leaf: its text is its whole source, so whatever tree-sitter
                // shows inside it (a literal-only rule's anonymous child) is
                // covered by that text.
                leaves += 1;
                continue;
            }
        }

        let expected = expected_children(real);
        assert_eq!(
            children.len(),
            expected.len(),
            "child count of {what}: typed {:?} vs tree-sitter {:?}",
            typed_kinds(&children),
            real_kinds(&expected)
        );

        let mut previous_end = real.start_byte();
        for (_, child) in &children {
            let range = &child.span().byte_range;
            assert!(range.start >= previous_end, "children out of order or overlapping in {what}");
            assert!(range.end <= real.end_byte(), "child outside its parent in {what}");
            previous_end = range.end;
        }

        // Reversed, so the stack pops them in source order.
        for ((field, child), (real_field, real_child)) in children.into_iter().zip(expected).rev() {
            assert_eq!(
                field,
                real_field,
                "field name of the `{}` child at bytes {:?} of {what}",
                real_child.kind(),
                real_child.byte_range()
            );
            if field.is_some() {
                labelled += 1;
            }
            stack.push((child, real_child, depth + 1, field));
        }
    }

    // The library's iterative walk is that very sequence.
    let mut walked = Vec::new();
    for visit in walk(&root) {
        walked.push((
            visit.depth,
            visit.field,
            visit.node.kind().to_string(),
            visit.node.span().byte_range.clone(),
        ));
    }
    assert_eq!(walked, sequence, "runtime::walk must visit in pre-order, with depth and field");
    (visited, tokens, leaves, labelled)
}

/// The kinds of the root's children, as the generic view reports them.
fn root_child_kinds(source: &str) -> Vec<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = parser.parse(source, None).expect("sample must parse");
    let recovered = recover::<ast::Grammar>(SourceNode { node: tree.root_node(), source });
    let root = recovered.value.expect("the root must be built");
    let mut kinds = Vec::new();
    for (_, child) in root.children() {
        kinds.push(child.kind().to_string());
    }
    kinds
}

fn main() {
    let (visited, tokens, leaves, labelled) = check(include_str!("../../grammar/bnf.bnf"), false);
    assert!(visited > 500, "bnf.bnf is a real grammar; only {visited} nodes visited");
    assert!(tokens > 0, "literal tokens must be reflected");
    assert!(leaves > 0, "leaves must be reflected");
    // `name:`, `body:`, `content:` … are labels in bnf.bnf; if none came back
    // the field comparison above would be vacuous.
    assert!(labelled > 100, "labelled children must be reflected; only {labelled}");

    // Multibyte characters in a literal: columns after it count characters.
    let (visited, tokens, _, _) = check("x -> 'çãçã' y ;\ny -> /[a-z]+/ ;\n", false);
    assert!(visited > 5 && tokens > 0);

    // A broken file. tree-sitter's recovery makes `@@@` and the rule after it
    // one `ERROR` node, a direct child of the root, after the good rule. The
    // typed tree keeps it right there — as a child of the root, in source
    // order, with the kind, naming, byte range, line/column and text of the
    // real node — instead of dropping it, and it has no children of its own.
    let broken = "a -> b ;\n@@@\nc -> d ;\n";
    check(broken, true);
    assert_eq!(root_child_kinds(broken), ["rule", "ERROR"]);
}
"#;

    compile_generated_ast_in_mode(
        "ast_reflect_walk_harness",
        &dogfood_grammar(),
        "bnf",
        runner,
        true,
        None,
        BuildMode::Partial,
    );
}

/// Behaviour test for the depth guard (`--partial-trees`), against the real
/// BNF parser: `x -> ((((…y…)))) ;` nests `subSeq` inside `subSeq`, as deep as
/// the input says.
///
/// - **Exact limit** — on a shallow input, `recover_with_limit(L)` for every
///   `L` builds a tree whose non-leaf nesting never passes `L`, and reports
///   `TooDeep` exactly when it had to stop at `L`. `L = 0` builds nothing.
/// - **No abort** — with the default limit, inputs nested 10 to 20 000 parens
///   deep come back as values (and a `TooDeep`) on a *2 MiB* thread in a debug
///   build, where the unguarded builder overflowed the stack at a few hundred.
///   `Debug` and `Drop` on what it built are fine on that stack too.
/// - **A syntax error at the bottom of a deep tree** is found without
///   recursion: the `ERROR`/`MISSING` walk over tree-sitter's tree is
///   iterative.
/// - **`walk` is iterative**: a typed tree thousands of levels deep (built on
///   a big stack, with a raised limit) is walked on a tiny one.
#[test]
fn deep_nesting_is_guarded_and_never_aborts() {
    let runner = r#"
use ast::runtime::{recover, recover_with_limit, walk, DiagnosticKind, DEFAULT_MAX_DEPTH, Reflect};
use tree_sitter::Parser;
use visitor::SourceNode;

fn nested(parens: usize) -> String {
    format!("x -> {}y{} ;\n", "(".repeat(parens), ")".repeat(parens))
}

fn tree_of(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    parser.parse(source, None).expect("sample must parse")
}

/// How many `TooDeep` diagnostics, and the limit they report.
fn too_deep(diagnostics: &[ast::runtime::Diagnostic]) -> (usize, Option<usize>) {
    let (mut count, mut limit) = (0, None);
    for d in diagnostics {
        if let DiagnosticKind::TooDeep { limit: l } = &d.kind {
            count += 1;
            limit = Some(*l);
        }
    }
    (count, limit)
}

/// The deepest nesting of non-leaf nodes in a built tree: a node counts as one
/// level, the root being level 1. Leaves, tokens and error regions have text,
/// and are not levels.
fn levels(root: &dyn Reflect) -> usize {
    let mut deepest = 0;
    for visit in walk(root) {
        if visit.node.text().is_none() {
            deepest = deepest.max(visit.depth + 1);
        }
    }
    deepest
}

/// How deep tree-sitter's own tree is, counted with a cursor, not recursion.
fn depth_of(root: tree_sitter::Node<'_>) -> usize {
    let mut cursor = root.walk();
    let (mut depth, mut deepest) = (0usize, 0usize);
    loop {
        deepest = deepest.max(depth);
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return deepest;
            }
            depth -= 1;
        }
    }
}

/// Runs `f` on a thread with the given stack, so overflowing it aborts here.
fn on_stack<F: FnOnce() + Send + 'static>(bytes: usize, f: F) {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("the thread must not panic");
}

fn exact_limit() {
    let source = "x -> (y) ;\n";
    let tree = tree_of(source);
    let natural = {
        let full = recover::<ast::Grammar>(SourceNode { node: tree.root_node(), source });
        assert!(full.is_complete(), "{:?}", full.diagnostics);
        levels(&full.value.unwrap())
    };
    assert!(natural > 8, "the sample must nest a few levels; got {natural}");

    for limit in 0..=natural + 2 {
        let r = recover_with_limit::<ast::Grammar>(
            SourceNode { node: tree.root_node(), source },
            limit,
        );
        let (count, reported) = too_deep(&r.diagnostics);
        if limit == 0 {
            // Not even the root: nothing is built and the root is what is reported.
            assert!(r.value.is_none(), "limit 0 builds nothing");
            assert_eq!(count, 1);
            assert_eq!(r.diagnostics[0].span.byte_range, 0..source.len());
            continue;
        }
        let built = levels(&r.value.expect("limit >= 1 builds the root"));
        if limit >= natural {
            assert_eq!(count, 0, "limit {limit} is enough: {:?}", r.diagnostics);
            assert_eq!(built, natural);
        } else {
            // It went exactly as deep as it was allowed, then said so.
            assert!(count >= 1, "limit {limit} is too small for {natural} levels");
            assert_eq!(reported, Some(limit));
            assert_eq!(built, limit, "no level wasted, none exceeded");
        }
    }
}

fn deep_inputs_do_not_abort() {
    // Complete below the limit; past it one TooDeep and a built, bounded tree.
    for parens in [10, 100, 1_000, 20_000] {
        let source = nested(parens);
        let tree = tree_of(&source);
        let r = recover::<ast::Grammar>(SourceNode { node: tree.root_node(), source: &source });
        let (count, limit) = too_deep(&r.diagnostics);
        let root = r.value.as_ref().expect("the shallow part is always built");
        assert!(levels(root) <= DEFAULT_MAX_DEPTH);
        if parens <= 10 {
            assert!(r.is_complete(), "{:?}", r.diagnostics);
        } else {
            assert_eq!(limit, Some(DEFAULT_MAX_DEPTH));
            // The one subtree that was not built, not every level after it.
            assert_eq!(count, 1, "{parens} parens: {:?}", r.diagnostics);
            let span = &r.diagnostics[0].span.byte_range;
            assert!(span.start < span.end && span.end <= source.len());
            assert_eq!(levels(root), DEFAULT_MAX_DEPTH, "built right up to the limit");
        }
        // Debug and Drop recurse too; at the limit they fit the same small stack.
        let shown = format!("{:?}", r.value);
        assert!(shown.starts_with("Some(Grammar"));
        drop(r);
    }
}

fn error_at_the_bottom_of_a_deep_tree() {
    let parens = 5_000;
    // The junk comes *after* `y`: before it, tree-sitter swallows the whole
    // nest into one shallow `ERROR`. Here it skips the junk where it stands, so
    // the error sits 20 000 levels down in a tree that is otherwise intact.
    let source = format!("x -> {}y @@@{} ;\n", "(".repeat(parens), ")".repeat(parens));
    let tree = tree_of(&source);
    assert!(tree.root_node().has_error());
    assert!(depth_of(tree.root_node()) > 20_000, "the tree itself must be deep");
    let r = recover::<ast::Grammar>(SourceNode { node: tree.root_node(), source: &source });
    let mut errors = 0;
    for d in &r.diagnostics {
        if matches!(d.kind, DiagnosticKind::Error) {
            errors += 1;
        }
    }
    assert!(errors >= 1, "the syntax error must be found: {:?}", r.diagnostics.len());
    drop(r);
}

fn main() {
    exact_limit();
    on_stack(2 << 20, deep_inputs_do_not_abort);
    on_stack(2 << 20, error_at_the_bottom_of_a_deep_tree);

    // A typed tree thousands of levels deep, built where the stack is big and
    // the limit raised, then walked on a stack far too small to recurse on.
    on_stack(256 << 20, || {
        let source = nested(1_500);
        let tree = tree_of(&source);
        let r = recover_with_limit::<ast::Grammar>(
            SourceNode { node: tree.root_node(), source: &source },
            1_000_000,
        );
        assert!(r.is_complete(), "{:?}", r.diagnostics.len());
        let root = r.value.as_ref().unwrap();
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(256 << 10)
                .spawn_scoped(scope, || {
                    let (mut visited, mut deepest) = (0usize, 0usize);
                    for visit in walk(root) {
                        visited += 1;
                        deepest = deepest.max(visit.depth);
                    }
                    assert!(deepest > 5_000, "only {deepest} deep");
                    assert!(visited > deepest);
                })
                .expect("spawn")
                .join()
                .expect("walk must not overflow a small stack");
        });
        // Dropped here, back on the big stack.
        drop(r);
    });
}
"#;

    compile_generated_ast_in_mode(
        "ast_deep_nesting_harness",
        &dogfood_grammar(),
        "bnf",
        runner,
        true,
        None,
        BuildMode::Partial,
    );
}

/// Compile-check for a kind literally named `token` beside a literal
/// alternative (strict mode, labelled field). The enum for "kind or literal"
/// has a fixed `Token` variant, and the kind's own variant used to be named
/// `Token` too: two variants of one name, which does not compile. Only that
/// kind's variant is renamed (`TokenNode`), so this now builds.
#[test]
fn kind_named_token_beside_a_literal_compiles() {
    let grammar = parse_fixture(
        "kind named token",
        "root -> a: (token | 'x') ;\n\
         token -> /[a-z]/ ;\n",
    );

    compile_generated_ast(
        "ast_kind_named_token_harness",
        &grammar,
        "kind_named_token_sample",
        "fn main() {}\n",
        false,
        None,
    );
}

/// Behaviour test for the generated `parse_ast` (`--partial-trees`): the
/// tree-sitter-free entry point. The runner never names `Parser`, `Tree` or
/// `SourceNode` when it calls it (only its stand-in for the scaffolded
/// crate's own `parse` does, as that crate's `lib.rs` would). A clean file
/// comes back complete, with the root rule's struct. A broken one is *not* an
/// `Err`: it comes back `Ok`, with diagnostics, and the broken region kept in
/// the tree under its parent.
#[test]
fn parse_ast_recovers_without_the_caller_touching_tree_sitter() {
    let runner = r#"
use ast::runtime::Reflect;

/// What the scaffolded crate's `lib.rs` provides, which `ast::parse_ast`
/// calls; here it drives the BNF parser.
fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_bnf::LANGUAGE.into())?;
    parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse the given source".into())
}

fn main() {
    // A clean file: complete, and the root rule's struct.
    let clean = ast::parse_ast(include_str!("../../grammar/bnf.bnf")).expect("the parser must run");
    assert!(clean.is_complete(), "diagnostics: {:?}", clean.diagnostics);
    let root = clean.value.expect("a clean file builds a root");
    assert_eq!(root.kind(), "grammar");

    // A broken file is not an error: whatever could be built comes back, with
    // the problem as a diagnostic and the broken region in the tree.
    let broken = ast::parse_ast("a -> b ;\n@@@\nc -> d ;\n").expect("a syntax error is not an Err");
    assert!(!broken.is_complete());
    assert_eq!(broken.diagnostics.len(), 1);
    let root = broken.value.expect("the good rule is still built");
    let mut kinds = Vec::new();
    for (field, child) in root.children() {
        assert_eq!(field, None, "the root has no labelled children");
        kinds.push(child.kind().to_string());
    }
    assert_eq!(kinds, ["rule", "ERROR"]);
}
"#;

    compile_generated_ast_in_mode(
        "ast_parse_ast_harness",
        &dogfood_grammar(),
        "bnf",
        runner,
        true,
        None,
        BuildMode::Partial,
    );
}
