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

/// Renders `grammar` via [`RustVisitor`] and [`RustAst`] (with an optional
/// `merge_config` — 342.5.7), wraps each in its own `mod visitor { .. }` /
/// `mod ast { .. }` block (matching the real scaffolded crate's
/// sibling-module layout `ast.rs`'s `use super::visitor::SourceNode;`
/// depends on), appends `extra_source`, and writes the result into a real
/// `cargo` example of this package named `example_name`. Then runs `cargo
/// build` (`run` when `run` is `true`) `--example <example_name>` and
/// asserts it succeeds — panicking with both captured stdout and stderr
/// otherwise.
fn compile_generated_ast(
    example_name: &str,
    grammar: &Grammar,
    rust_name: &str,
    extra_source: &str,
    run: bool,
    merge_config: Option<&MergeConfig>,
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
    let ast_source = RustAst::new(grammar, "<test>", true, merge_config)
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
