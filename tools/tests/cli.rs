//! End-to-end tests for the `ts-bnf-tool` binary.
//!
//! These run the compiled binary as a subprocess so that the full `main()`
//! dispatch — including the `--json` output branches — is exercised.
use indoc::indoc;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

mod support;

fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ts-bnf-tool"))
}

/// A small, clean grammar with no diagnostics.
const CLEAN_BNF: &str = indoc! {"
    expr -> term ('+' term)* ;
    term -> /[0-9]+/ | '(' expr ')' ;
"};

/// A grammar that produces an "unused rule" warning.
const WARN_BNF: &str = indoc! {"
    root -> 'a' ;
    unused -> 'b' ;
"};

/// A grammar with a duplicate `%axiom` that produces an error.
const ERROR_BNF: &str = indoc! {"
    %axiom expr
    %axiom term
    expr -> term ;
    term -> /[0-9]+/ ;
"};

/// A left-recursive grammar — valid for tree-sitter, must pass `check` (#197).
const LEFT_RECURSIVE_BNF: &str = indoc! {"
    expr -> expr '+' term | term ;
    term -> /[0-9]+/ ;
"};

fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    fs::write(&path, content).unwrap();
    path
}

// ── check --json ──────────────────────────────────────────────────────────────

#[test]
/// --json emits an object with a "diagnostics" key (never a bare array).
fn check_json_clean_exits_zero_and_emits_empty_diagnostics() {
    let path = write_tmp("ts_bnf_check_clean.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "expected exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["diagnostics"], serde_json::json!([]));
}

#[test]
/// --json warning output is nested under the "diagnostics" key.
fn check_json_warning_exits_one_and_contains_severity() {
    let path = write_tmp("ts_bnf_check_warn.bnf", WARN_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "expected exit 1 for warnings");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = parsed["diagnostics"].as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().any(|d| d["severity"] == "warning"));
}

#[test]
/// --json error output is nested under the "diagnostics" key.
fn check_json_error_exits_two_and_contains_severity() {
    let path = write_tmp("ts_bnf_check_err.bnf", ERROR_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "expected exit 2 for errors");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = parsed["diagnostics"].as_array().unwrap();
    assert!(arr.iter().any(|d| d["severity"] == "error"));
}

#[test]
fn check_plain_text_goes_to_stderr_not_stdout() {
    let path = write_tmp("ts_bnf_check_plain.bnf", WARN_BNF);
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(
        out.stdout.is_empty(),
        "plain-text output must not appear on stdout"
    );
    assert!(
        !out.stderr.is_empty(),
        "plain-text diagnostics must appear on stderr"
    );
}

#[test]
/// Left recursion is idiomatic tree-sitter style; `check` must exit 0 (#197).
fn check_left_recursive_grammar_exits_zero() {
    let path = write_tmp("ts_bnf_check_left_rec.bnf", LEFT_RECURSIVE_BNF);
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "left-recursive grammar must pass check: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
/// `convert` must accept a left-recursive grammar without `--no-check` (#197).
fn convert_left_recursive_grammar_succeeds_without_no_check() {
    let path = write_tmp("ts_bnf_convert_left_rec.bnf", LEFT_RECURSIVE_BNF);
    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "left-recursive grammar must convert without --no-check: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
/// `check --summary` still reports left-recursion counts as a property (#197).
fn check_summary_reports_left_recursion_counts() {
    let path = write_tmp("ts_bnf_summary_left_rec.bnf", LEFT_RECURSIVE_BNF);
    let out = tool()
        .args(["check", "--json", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "expected exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["summary"]["left_recursive_direct"], 1);
    assert_eq!(parsed["summary"]["left_recursive_mutual"], 0);
}

// ── firsts --json ─────────────────────────────────────────────────────────────

#[test]
fn firsts_json_emits_object_with_rule_keys() {
    let path = write_tmp("ts_bnf_firsts.bnf", CLEAN_BNF);
    let out = tool()
        .args(["firsts", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(obj.contains_key("expr"), "expected 'expr' key");
    assert!(obj.contains_key("term"), "expected 'term' key");
}

#[test]
fn firsts_json_terminals_are_sorted_arrays_of_strings() {
    let path = write_tmp("ts_bnf_firsts_sorted.bnf", CLEAN_BNF);
    let out = tool()
        .args(["firsts", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let obj = parsed.as_object().unwrap();
    for terminals in obj.values() {
        let arr = terminals.as_array().unwrap();
        assert!(!arr.is_empty());
        assert!(arr.iter().all(|v| v.is_string()));
        // Verify sorted order
        let strings: Vec<&str> = arr.iter().map(|v| v.as_str().unwrap()).collect();
        let mut sorted = strings.clone();
        sorted.sort_unstable();
        assert_eq!(strings, sorted, "terminals must be sorted");
    }
}

#[test]
fn firsts_json_output_is_valid_json() {
    let path = write_tmp("ts_bnf_firsts_valid.bnf", CLEAN_BNF);
    let out = tool()
        .args(["firsts", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(stdout.trim()).is_ok(),
        "output must be valid JSON"
    );
}

// ── convert --generate ────────────────────────────────────────────────────────

#[test]
fn generate_writes_queries_highlights_scm() {
    let path = write_tmp("ts_bnf_gen.bnf", CLEAN_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_gen_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["convert", "--generate", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "convert --generate must succeed");
    let highlights = out_dir.join("queries").join("highlights.scm");
    assert!(
        highlights.exists(),
        "queries/highlights.scm must be created"
    );
    let content = std::fs::read_to_string(&highlights).unwrap();
    assert!(content.contains("; Generated by ts-bnf-tool"));
}

#[test]
fn generate_writes_tree_sitter_json() {
    let path = write_tmp("ts_bnf_gen_json.bnf", CLEAN_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_gen_json_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args([
            "convert",
            "--generate",
            "--name",
            "mygrammar",
            "--output-dir",
        ])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "convert --generate must succeed");
    let ts_json = out_dir.join("tree-sitter.json");
    assert!(ts_json.exists(), "tree-sitter.json must be created");
    let content = std::fs::read_to_string(&ts_json).unwrap();
    assert!(content.contains("\"name\": \"mygrammar\""));
    assert!(content.contains("\"camelcase\": \"Mygrammar\""));
    assert!(content.contains("\"scope\": \"source.mygrammar\""));
}

#[test]
fn generate_does_not_overwrite_existing_tree_sitter_json() {
    let path = write_tmp("ts_bnf_gen_no_overwrite.bnf", CLEAN_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_gen_no_overwrite_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let ts_json = out_dir.join("tree-sitter.json");
    std::fs::write(
        &ts_json,
        r#"{"grammars":[{"name":"preexisting","camelcase":"Preexisting","scope":"source.preexisting","file-types":[]}],"metadata":{"version":"9.9.9","license":"Apache-2.0"}}"#,
    ).unwrap();
    let out = tool()
        .args(["convert", "--generate", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "convert --generate must succeed");
    let content = std::fs::read_to_string(&ts_json).unwrap();
    assert!(
        content.contains("\"preexisting\""),
        "existing tree-sitter.json must not be overwritten"
    );
}

#[test]
fn generate_produces_abi_15_with_tree_sitter_json() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate("ts_bnf_gen_abi_project", Some("mygrammar"), CLEAN_BNF);
    let parser_c = out_dir.join("src").join("parser.c");
    assert!(parser_c.exists(), "src/parser.c must be generated");
    let content = fs::read_to_string(&parser_c).unwrap();
    assert!(
        content.contains("#define LANGUAGE_VERSION 15"),
        "parser.c must use ABI 15; got: {}",
        content
            .lines()
            .find(|l| l.contains("LANGUAGE_VERSION"))
            .unwrap_or("(not found)")
    );
}

// ── %axiom real-CLI start symbol (#264) ─────────────────────────────────────

/// `term` is declared first, `expression` second, with no `%axiom`. Used to
/// pin the default declaration-order fallback through the real CLI.
const BASELINE_BNF: &str = indoc! {"
    term -> /[0-9]+/ ;
    expression -> term '+' term ;
"};

/// Same two rules as `BASELINE_BNF`, but `%axiom expression` overrides the
/// declaration-order default, making `expression` the start symbol despite
/// `term` being declared first.
const AXIOM_BNF: &str = indoc! {"
    %axiom expression
    term -> /[0-9]+/ ;
    expression -> term '+' term ;
"};

#[test]
/// With no `%axiom`, the real `tree-sitter` CLI's parser root is the
/// first-declared rule (`term`), not `expression`.
fn generate_default_root_is_first_declared_rule_with_tree_sitter_parse() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate(
        "ts_bnf_gen_baseline_project",
        Some("baselinetest"),
        BASELINE_BNF,
    );
    let stdout = support::parse(&out_dir, "1");
    assert!(
        stdout.trim_start().starts_with("(term "),
        "expected 'term' as root node; got: {stdout}"
    );
}

#[test]
/// `%axiom expression` overrides declaration order: the real `tree-sitter`
/// CLI's parser root is `expression`, not the first-declared rule `term`.
fn generate_axiom_rule_becomes_parser_root_symbol() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate("ts_bnf_gen_axiom_project", Some("axiomtest"), AXIOM_BNF);
    let stdout = support::parse(&out_dir, "1+1");
    assert!(
        stdout.trim_start().starts_with("(expression "),
        "expected 'expression' as root node; got: {stdout}"
    );
}

// ── %extras real-CLI test (#266) ─────────────────────────────────────────────

/// Grammar with `%extras /\s/, comment` so whitespace and `#`-line comments
/// are accepted anywhere between tokens without causing errors.
const EXTRAS_BNF: &str = indoc! {"
    %axiom program
    %extras /\\s/, comment
    comment -> '#' /[^\\n]*/ ;
    program -> word+ ;
    word -> /[a-z]+/ ;
"};

#[test]
/// Whitespace and named comment extras are skipped between ordinary tokens:
/// the real `tree-sitter` parser produces no ERROR node and correctly roots
/// the tree at `program`.
fn generate_extras_whitespace_and_comments_are_skipped() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate("ts_bnf_gen_extras_project", Some("extrastest"), EXTRAS_BNF);
    let stdout = support::parse(&out_dir, "hello # a comment\nworld");
    assert!(
        stdout.trim_start().starts_with("(program "),
        "expected 'program' as root node; got: {stdout}"
    );
    assert!(
        !stdout.contains("ERROR"),
        "expected no ERROR node; got: {stdout}"
    );
    assert!(
        stdout.contains("(word"),
        "expected '(word' nodes in tree; got: {stdout}"
    );
}

// ── %conflicts real-CLI test (#267) ──────────────────────────────────────────

/// Grammar with `%conflicts [stmt]` to whitelist the dangling-else
/// shift/reduce conflict.  Without the declaration, `tree-sitter generate`
/// would exit non-zero (see `generate_without_conflicts_decl_fails_generate`).
const CONFLICTS_BNF: &str = indoc! {"
    %axiom prog
    %conflicts [stmt]

    prog -> stmt ;
    stmt -> 'if' name stmt
          | 'if' name stmt else_clause
          | name ';'
          ;
    else_clause -> 'else' stmt ;
    name -> /[a-z]+/ ;
"};

/// Same grammar as `CONFLICTS_BNF` but without the `%conflicts` declaration,
/// so `tree-sitter generate` fails due to the unresolved LR conflict.
const CONFLICTS_WITHOUT_DECL_BNF: &str = indoc! {"
    %axiom prog

    prog -> stmt ;
    stmt -> 'if' name stmt
          | 'if' name stmt else_clause
          | name ';'
          ;
    else_clause -> 'else' stmt ;
    name -> /[a-z]+/ ;
"};

#[test]
/// `%conflicts [stmt]` whitelists the dangling-else LR conflict: the real
/// `tree-sitter` CLI generates successfully and parsing a sample input
/// produces no ERROR node.
fn generate_conflicts_whitelist_succeeds_and_parses_cleanly() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate(
        "ts_bnf_gen_conflicts_project",
        Some("conflictstest"),
        CONFLICTS_BNF,
    );
    let stdout = support::parse(&out_dir, "if x foo;");
    assert!(
        stdout.trim_start().starts_with("(prog "),
        "expected 'prog' as root node; got: {stdout}"
    );
    assert!(
        !stdout.contains("ERROR"),
        "expected no ERROR node; got: {stdout}"
    );
    assert!(
        stdout.contains("(stmt "),
        "expected '(stmt' nodes in tree; got: {stdout}"
    );
}

#[test]
/// Without `%conflicts`, the same dangling-else grammar causes a genuine
/// LR(1) conflict that `tree-sitter generate` cannot resolve — `convert
/// --generate` must exit non-zero.
fn generate_without_conflicts_decl_fails_generate() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let bnf_path = std::env::temp_dir().join("ts_bnf_gen_conflicts_neg.bnf");
    std::fs::write(&bnf_path, CONFLICTS_WITHOUT_DECL_BNF).unwrap();

    let out_dir = std::env::temp_dir().join("ts_bnf_gen_conflicts_neg_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = Command::new(env!("CARGO_BIN_EXE_ts-bnf-tool"))
        .args([
            "convert",
            "--generate",
            "--name",
            "conflictstest",
            "--output-dir",
        ])
        .arg(&out_dir)
        .arg(&bnf_path)
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "convert --generate must fail for an ambiguous grammar without %conflicts; \
         stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── %supertypes real-CLI test (#270) ─────────────────────────────────────────

/// Grammar with `%supertypes expression` so that `expression` acts as a
/// supertype over its concrete alternatives `number` and `string_lit`.
const SUPERTYPES_BNF: &str = indoc! {"
    %axiom program
    %supertypes expression
    program -> expression+ ;
    expression -> number | string_lit ;
    number -> /[0-9]+/ ;
    string_lit -> '\"' /[^\"]*/ '\"' ;
"};

#[test]
/// After `convert --generate`, `src/node-types.json` must contain an entry for
/// `expression` with a `subtypes` array, confirming it is a supertype.
fn generate_supertypes_rule_marked_in_node_types_json() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate(
        "ts_bnf_gen_supertypes_project",
        Some("supertypestest"),
        SUPERTYPES_BNF,
    );
    let node_types_path = out_dir.join("src").join("node-types.json");
    assert!(
        node_types_path.exists(),
        "src/node-types.json must be generated"
    );
    let content = fs::read_to_string(&node_types_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let entries = parsed
        .as_array()
        .expect("node-types.json must be a JSON array");
    let supertype_entry = entries
        .iter()
        .find(|e| e["type"].as_str() == Some("expression"))
        .expect("node-types.json must contain an entry for 'expression'");
    let subtypes = supertype_entry["subtypes"]
        .as_array()
        .expect("supertype entry must have a 'subtypes' array");
    assert!(
        !subtypes.is_empty(),
        "expression's subtypes array must not be empty; entry: {supertype_entry}"
    );
}

// ── %inline real-CLI test (#269) ─────────────────────────────────────────────

/// Grammar with `%inline kv_pair` so the helper rule is absent from the parse
/// tree: its children (`key`, `value`) should appear directly under `program`.
const INLINE_BNF: &str = indoc! {"
    %axiom program
    %inline kv_pair
    program -> kv_pair+ ;
    kv_pair -> key '=' value ;
    key -> /[a-z]+/ ;
    value -> /[0-9]+/ ;
"};

#[test]
/// `%inline kv_pair` causes the inlined rule's node to be absent from the
/// real `tree-sitter` parse output while its children appear directly under
/// the caller (`program`).
fn generate_inline_rule_absent_from_parse_tree() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate("ts_bnf_gen_inline_project", Some("inlinetest"), INLINE_BNF);
    let stdout = support::parse(&out_dir, "x=1");
    assert!(
        stdout.trim_start().starts_with("(program "),
        "expected 'program' as root node; got: {stdout}"
    );
    assert!(
        !stdout.contains("(kv_pair "),
        "inlined rule 'kv_pair' must not appear as a node; got: {stdout}"
    );
    assert!(
        stdout.contains("(key "),
        "expected '(key' node directly under program; got: {stdout}"
    );
    assert!(
        stdout.contains("(value "),
        "expected '(value' node directly under program; got: {stdout}"
    );
}

// ── %externals real-CLI test (#271) ──────────────────────────────────────────

/// Grammar with `%externals indent` so the generated JS includes an `externals`
/// array; `program` uses the external token in sequence with a pattern rule so
/// the grammar is valid.
const EXTERNALS_BNF: &str = indoc! {"
    %axiom program
    %externals indent
    program -> indent word+ ;
    word -> /[a-z]+/ ;
"};

#[test]
/// After `convert --generate`, `src/parser.c` must reference the external token
/// name, confirming the `%externals` declaration was forwarded to tree-sitter.
///
/// Compile-only scope: exercising `indent` at parse time requires a
/// compiled-and-linked external scanner (hand-written C). That is out of reach
/// for a lightweight CI fixture. Full parse-behaviour coverage is therefore
/// intentionally omitted here; this test only verifies that `generate` succeeds
/// and that the generated artefacts carry the token name. See
/// `tools/tests/support/mod.rs` §`%externals` for the recorded scope decision.
fn generate_externals_token_name_appears_in_parser_c() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate(
        "ts_bnf_gen_externals_project",
        Some("externalstest"),
        EXTERNALS_BNF,
    );
    let parser_c = out_dir.join("src").join("parser.c");
    assert!(parser_c.exists(), "src/parser.c must be generated");
    let content = fs::read_to_string(&parser_c).unwrap();
    assert!(
        content.contains("indent"),
        "parser.c must reference the external token name 'indent'; \
         first 500 chars: {}",
        &content[..content.len().min(500)]
    );
}

// ── highlights ────────────────────────────────────────────────────────────────

/// A grammar with a variety of rule names to exercise the heuristics.
/// A grammar with an `%inline` directive referencing `expr`, for directive rename tests.
const RENAME_DIRECTIVE_BNF: &str = indoc! {"
    %inline expr
    expr -> term '+' term ;
    term -> /[0-9]+/ ;
"};

const HIGHLIGHTS_BNF: &str = indoc! {r#"
    value      -> string | number | expr ;
    string     -> '"' /[^"]*/ '"' ;
    number     -> /[0-9]+/ ;
    line_comment -> '#' /.*/ ;
    expr       -> value '+' value ;
"#};

// ── rename ────────────────────────────────────────────────────────────────────

/// After renaming `term` to `terminal`, the `expression` rule body must reference
/// the new name and the old name must not appear anywhere in the output.
#[test]
fn rename_renames_rhs_references() {
    let path = write_tmp("ts_bnf_rename_rhs.bnf", CLEAN_BNF);
    let out = tool()
        .args(["rename"])
        .arg(&path)
        .args(["term", "terminal"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("terminal"),
        "new name must appear in output"
    );
    assert!(
        !stdout.contains("term "),
        "old name must not appear as a standalone word"
    );
}

/// Renaming `expr` to `expression` produces output where `expression ->` is the
/// definition and `expr ->` no longer appears.
#[test]
fn rename_renames_definition() {
    let path = write_tmp("ts_bnf_rename1.bnf", CLEAN_BNF);
    let out = tool()
        .args(["rename"])
        .arg(&path)
        .args(["expr", "expression"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("expression ->"),
        "new rule name must appear as a definition"
    );
    assert!(
        !stdout.contains("expr ->"),
        "old rule name must not appear as a definition"
    );
}

/// The `%inline` directive is updated when the referenced rule is renamed.
#[test]
fn rename_renames_directive() {
    let path = write_tmp("ts_bnf_rename_dir.bnf", RENAME_DIRECTIVE_BNF);
    let out = tool()
        .args(["rename"])
        .arg(&path)
        .args(["expr", "expression"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("%inline expression"),
        "directive must use the new name"
    );
    assert!(
        !stdout.contains("%inline expr\n"),
        "directive must not use the old name"
    );
}

/// Renaming to an unknown source rule exits non-zero and prints an error to stderr.
#[test]
fn rename_unknown_source_exits_nonzero() {
    let path = write_tmp("ts_bnf_rename_err1.bnf", CLEAN_BNF);
    let out = tool()
        .args(["rename"])
        .arg(&path)
        .args(["nonexistent", "something"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "must exit non-zero for unknown source rule"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("nonexistent"),
        "stderr must name the missing rule"
    );
}

/// Renaming to a name that is already defined exits non-zero and prints an error to stderr.
#[test]
fn rename_target_already_defined_exits_nonzero() {
    let path = write_tmp("ts_bnf_rename_err2.bnf", CLEAN_BNF);
    let out = tool()
        .args(["rename"])
        .arg(&path)
        .args(["expr", "term"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "must exit non-zero when target name is taken"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("term"),
        "stderr must name the conflicting rule"
    );
}

/// `--in-place` rewrites the file on disk with the renamed rule.
#[test]
fn rename_in_place_rewrites_file() {
    let path = write_tmp("ts_bnf_rename_inplace.bnf", CLEAN_BNF);
    let out = tool()
        .args(["rename", "--in-place"])
        .arg(&path)
        .args(["expr", "expression"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "--in-place must not write to stdout");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("expression ->"),
        "file must contain the new rule name"
    );
    assert!(
        !content.contains("expr ->"),
        "file must not contain the old rule name"
    );
}

#[test]
fn highlights_emits_scheme_header() {
    let path = write_tmp("ts_bnf_hl.bnf", HIGHLIGHTS_BNF);
    let out = tool().args(["highlights"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("; Generated by ts-bnf-tool"));
}

#[test]
fn highlights_classifies_known_rules() {
    let path = write_tmp("ts_bnf_hl2.bnf", HIGHLIGHTS_BNF);
    let out = tool().args(["highlights"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("(string) @string"),
        "string rule must be classified"
    );
    assert!(
        stdout.contains("(number) @number"),
        "number rule must be classified"
    );
    assert!(
        stdout.contains("(line_comment) @comment"),
        "line_comment rule must be classified"
    );
}

#[test]
fn highlights_emits_todo_for_unknown_rules() {
    let path = write_tmp("ts_bnf_hl3.bnf", HIGHLIGHTS_BNF);
    let out = tool().args(["highlights"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("; (expr) TODO: @???"),
        "unclassified rule must get a TODO"
    );
}

#[test]
fn highlights_omits_pure_structural_rules() {
    let path = write_tmp("ts_bnf_hl4.bnf", HIGHLIGHTS_BNF);
    let out = tool().args(["highlights"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    // `value` is purely structural (only references non-terminals)
    assert!(
        !stdout.contains("(value)"),
        "purely structural rule must be omitted"
    );
}

#[test]
fn highlights_no_todos_suppresses_placeholders() {
    let path = write_tmp("ts_bnf_hl5.bnf", HIGHLIGHTS_BNF);
    let out = tool()
        .args(["highlights", "--no-todos"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("TODO"),
        "--no-todos must suppress placeholder entries"
    );
}

#[test]
fn highlights_output_file_flag() {
    let path = write_tmp("ts_bnf_hl6.bnf", HIGHLIGHTS_BNF);
    let out_path = std::env::temp_dir().join("ts_bnf_highlights_out.scm");
    let _ = std::fs::remove_file(&out_path);
    let out = tool()
        .args(["highlights", "-o"])
        .arg(&out_path)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "-o must suppress stdout");
    assert!(out_path.exists(), "-o must create the output file");
    let content = std::fs::read_to_string(&out_path).unwrap();
    assert!(content.contains("(string) @string"));
}

// ── check --summary ───────────────────────────────────────────────────────────

#[test]
/// Plain-text summary appears on stdout, not stderr, so it is separable from
/// diagnostic output in shell pipelines.
fn check_summary_plain_goes_to_stdout() {
    let path = write_tmp("ts_bnf_summary_plain.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.is_empty(), "summary must appear on stdout");
    assert!(out.stderr.is_empty(), "no diagnostics expected on stderr");
}

#[test]
/// With warnings present: diagnostics go to stderr, summary still goes to
/// stdout, and the exit code is 1 (warnings) — unaffected by --summary.
fn check_summary_with_warnings_exit_one_and_separates_streams() {
    let path = write_tmp("ts_bnf_summary_warn.bnf", WARN_BNF);
    let out = tool()
        .args(["check", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "expected exit 1 for warnings");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("Rules"), "summary must be on stdout");
    assert!(stderr.contains("warning"), "diagnostics must be on stderr");
}

#[test]
/// With errors present: exit code is 2 — --summary does not change exit
/// code semantics.
fn check_summary_with_errors_exit_two() {
    let path = write_tmp("ts_bnf_summary_err.bnf", ERROR_BNF);
    let out = tool()
        .args(["check", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "expected exit 2 for errors");
}

#[test]
/// --json --summary emits a JSON object with both "diagnostics" and "summary"
/// keys, and the summary contains all expected fields.
fn check_summary_json_contains_summary_key() {
    let path = write_tmp("ts_bnf_summary_json.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--json", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["diagnostics"], serde_json::json!([]));
    let summary = &parsed["summary"];
    assert!(summary.is_object(), "summary must be a JSON object");
    for field in &[
        "rules",
        "leaf_rules",
        "unreachable_rules",
        "unique_literals",
        "unique_patterns",
        "undefined_refs",
        "left_recursive_direct",
        "left_recursive_mutual",
    ] {
        assert!(
            summary[field].is_number(),
            "missing or non-numeric field: {field}"
        );
    }
    assert!(
        summary["first_sets"].is_object(),
        "first_sets must be present for non-empty grammar"
    );
}

#[test]
/// --json without --summary must not include a "summary" key.
fn check_json_without_summary_has_no_summary_key() {
    let path = write_tmp("ts_bnf_summary_json_absent.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        parsed["summary"].is_null(),
        "summary key must be absent without --summary"
    );
}

// ── railroad ──────────────────────────────────────────────────────────────────

#[test]
/// Undefined non-terminal reference emits a warning to stderr, still produces
/// valid SVG on stdout, and exits 0 (R-18).
fn railroad_undefined_ref_warns_but_exits_zero() {
    // `ghost` is referenced but never defined.
    let bnf = "expr -> ghost '+' expr ;\n";
    let path = write_tmp("ts_bnf_rr_undef.bnf", bnf);
    let out = tool().args(["railroad"]).arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "undefined reference must not abort; exit code was {:?}",
        out.status.code()
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.starts_with("<svg"),
        "stdout must be a valid SVG element"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("ghost"),
        "stderr must name the undefined rule"
    );
    assert!(
        stderr.contains("warning"),
        "stderr must label the message as a warning"
    );
}

#[test]
/// Dogfood: `grammar/bnf.bnf` renders without error in single-file mode (R-20).
fn railroad_dogfood_single_file() {
    let grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../grammar/bnf.bnf");
    let out = tool().args(["railroad"]).arg(&grammar).output().unwrap();
    assert!(
        out.status.success(),
        "railroad on bnf.bnf must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("<svg"), "output must be an SVG element");
}

#[test]
/// Dogfood: `grammar/bnf.bnf` renders without error in split mode (R-20).
fn railroad_dogfood_split() {
    let grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../grammar/bnf.bnf");
    let out_dir = std::env::temp_dir().join("ts_bnf_rr_dogfood_split");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["railroad", "--split", "--output-dir"])
        .arg(&out_dir)
        .arg(&grammar)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "railroad --split on bnf.bnf must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_dir.exists(), "--output-dir must be created");
    let svgs: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "svg"))
        .collect();
    assert!(!svgs.is_empty(), "at least one .svg file must be written");
}

#[test]
/// Grammar composed via `%include` renders rules from both files in the output (R-19).
fn railroad_include_renders_all_rules() {
    let path = write_include_pair("cli_rr_inc_a.bnf", "cli_rr_inc_b.bnf");
    let out = tool().args(["railroad"]).arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "railroad on included grammar must exit 0"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("id=\"rule-root\""),
        "SVG must contain anchor for root rule"
    );
    assert!(
        stdout.contains("id=\"rule-b_rule\""),
        "SVG must contain anchor for included rule b_rule"
    );
}

#[test]
/// `--rule <unknown>` exits non-zero and names the missing rule in stderr (R-17).
fn railroad_unknown_rule_exits_nonzero() {
    let path = write_tmp("ts_bnf_rr_unknown.bnf", CLEAN_BNF);
    let out = tool()
        .args(["railroad", "--rule", "ghost"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "must exit non-zero for unknown --rule"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("ghost"),
        "stderr must name the missing rule"
    );
}

// ── stdin ─────────────────────────────────────────────────────────────────────

#[test]
/// `check -` reads a clean grammar from stdin and exits 0.
fn check_reads_clean_grammar_from_stdin() {
    let mut child = tool()
        .args(["check", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(CLEAN_BNF.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "check via stdin must succeed for clean grammar"
    );
}

// ── %include ──────────────────────────────────────────────────────────────────

/// Creates a two-file setup: `a_name` has `root -> b_rule ;` followed by an
/// `%include` of `b_name`, which defines `b_rule -> 'y' ;`.
///
/// `root` is declared before the `%include` so it is the first entry in the
/// merged productions map and acts as the implicit root rule.  This keeps the
/// grammar free of "unreachable rule" warnings.
fn write_include_pair(a_name: &str, b_name: &str) -> std::path::PathBuf {
    let tmp = std::env::temp_dir();
    let b_path = tmp.join(b_name);
    fs::write(&b_path, "b_rule -> 'y' ;\n").unwrap();
    let a_path = tmp.join(a_name);
    fs::write(
        &a_path,
        format!("root -> b_rule ;\n%include \"{b_name}\"\n"),
    )
    .unwrap();
    a_path
}

#[test]
/// `check` on a grammar that uses `%include` merges the included file and
/// exits 0 when the combined grammar is clean.
fn include_check_passes_for_valid_included_grammar() {
    let path = write_include_pair("cli_inc_check_a.bnf", "cli_inc_check_b.bnf");
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "check must exit 0 for a clean included grammar; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
/// `firsts --json` output contains rules from both the root file and the
/// included file after merging.
fn include_firsts_contains_rules_from_included_file() {
    let path = write_include_pair("cli_inc_firsts_a.bnf", "cli_inc_firsts_b.bnf");
    let out = tool()
        .args(["firsts", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(obj.contains_key("root"), "firsts must contain root rule");
    assert!(
        obj.contains_key("b_rule"),
        "firsts must contain rule from included file"
    );
}

#[test]
/// `convert --rules-only` output contains rules from both the root file and
/// the included file after merging.
fn include_convert_outputs_merged_rules() {
    let path = write_include_pair("cli_inc_conv_a.bnf", "cli_inc_conv_b.bnf");
    let out = tool()
        .args(["convert", "--rules-only"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("root"),
        "convert output must include root rule"
    );
    assert!(
        stdout.contains("b_rule"),
        "convert output must include rule from included file"
    );
}

#[test]
/// `format` inlines all `%include` directives and emits the merged grammar in
/// canonical BNF form; the `%include` directive itself does not appear in the
/// output.
fn include_format_outputs_merged_grammar() {
    let path = write_include_pair("cli_inc_fmt_a.bnf", "cli_inc_fmt_b.bnf");
    let out = tool().args(["format"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("root ->"),
        "format output must include root rule"
    );
    assert!(
        stdout.contains("b_rule ->"),
        "format output must include rule from included file"
    );
    assert!(
        !stdout.contains("%include"),
        "format output must not contain %include directives"
    );
}

// ── pattern flags (#198) ──────────────────────────────────────────────────────

/// A grammar whose pattern carries a JS regex flag suffix.
const FLAGGED_PATTERN_BNF: &str = indoc! {"
    root -> /select/i ;
"};

#[test]
/// `/select/i` passes `check` clean, `convert` emits the flagged literal
/// verbatim, and `format` round-trips it (#198).
fn pattern_flags_check_convert_format() {
    let path = write_tmp("ts_bnf_pattern_flags.bnf", FLAGGED_PATTERN_BNF);

    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "flagged pattern must check clean");

    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "flagged pattern must convert");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("/select/i"),
        "convert output must carry the flag suffix: {stdout}"
    );

    let out = tool().args(["format"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "flagged pattern must format");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("root -> /select/i;"),
        "format output must round-trip the flag suffix: {stdout}"
    );
}

// ── negative precedence levels (#196) ─────────────────────────────────────────

/// A grammar with a negative precedence level on an alternative.
const NEGATIVE_PREC_BNF: &str = indoc! {"
    a -> b 'x' %prec -1 ;
    b -> 'b' ;
"};

#[test]
/// `%prec -1` passes `check` clean, `convert` emits `prec(-1, …)`, and
/// `format` round-trips the sign (#196).
fn negative_prec_check_convert_format() {
    let path = write_tmp("ts_bnf_negative_prec.bnf", NEGATIVE_PREC_BNF);

    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "negative prec level must check clean");

    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "negative prec level must convert");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("prec(-1, seq($.b, 'x'))"),
        "convert output must carry the negative level: {stdout}"
    );

    let out = tool().args(["format"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "negative prec level must format");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("%prec -1"),
        "format output must round-trip the sign: {stdout}"
    );
}

// ── literal escape passthrough (#201) ─────────────────────────────────────────

/// A grammar exercising the documented JS escape sequences inside literals:
/// `\n`, `\0`, `\xNN`, `\\`, and an escaped quote of each delimiter kind.
const ESCAPED_LITERALS_BNF: &str = indoc! {r#"
    root -> '\n' '\0' '\x41' '\\' '\'' "\"" ;
"#};

#[test]
/// Escaped literals pass `check` clean, `convert` copies each lexeme verbatim
/// into the JS output (normalising double quotes to single), and `format`
/// round-trips them (#201).
fn escaped_literals_check_convert_format() {
    let path = write_tmp("ts_bnf_escaped_literals.bnf", ESCAPED_LITERALS_BNF);

    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "escaped literals must check clean");

    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "escaped literals must convert");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(r#"seq('\n', '\0', '\x41', '\\', '\'', '"')"#),
        "convert output must carry the escapes verbatim: {stdout}"
    );

    let out = tool().args(["format"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "escaped literals must format");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(r#"root -> '\n' '\0' '\x41' '\\' '\'' '"';"#),
        "format output must round-trip the escapes: {stdout}"
    );
}

#[test]
/// An escape the tool has never heard of (`'\q'`) is not rejected: the pair
/// passes through to the JS output untouched, leaving JS to interpret it.
/// This pins the no-validation decision of #201.
fn unknown_escape_passes_through_unvalidated() {
    let path = write_tmp("ts_bnf_unknown_escape.bnf", "root -> '\\q' ;\n");

    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "unknown escape must check clean");

    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "unknown escape must convert");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(r"'\q'"),
        "convert output must carry the unknown escape verbatim: {stdout}"
    );
}

// ── raw line breaks in literals (#208) ────────────────────────────────────────

#[test]
/// A literal containing a raw LF is a syntax error — line breaks must be
/// written as the `\n` escape (#208).
fn raw_lf_in_literal_is_syntax_error() {
    let path = write_tmp("ts_bnf_raw_lf_literal.bnf", "a -> 'x\ny' ;\n");
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "raw LF must be a syntax error");
}

#[test]
/// A literal containing a raw CR is a syntax error — CR is a JS
/// LineTerminator just like LF (#208).
fn raw_cr_in_literal_is_syntax_error() {
    let path = write_tmp("ts_bnf_raw_cr_literal.bnf", "a -> 'x\ry' ;\n");
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "raw CR must be a syntax error");
}

// ── graph ─────────────────────────────────────────────────────────────────────

const GRAPH_BNF: &str = indoc! {"
    program -> statement expression ;
    statement -> 'let' /[a-z]+/ ;
    expression -> term '+' term ;
    term -> /[0-9]+/ ;
"};

const GRAPH_UNDEF_BNF: &str = indoc! {"
    root -> defined extern_rule ;
    defined -> /x/ ;
"};

#[test]
/// DOT output contains the expected `digraph grammar {` wrapper and edges.
fn graph_dot_basic_output() {
    let path = write_tmp("ts_bnf_graph_dot.bnf", GRAPH_BNF);
    let out = tool().args(["graph"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("digraph grammar {"));
    assert!(stdout.contains("\"program\" -> \"statement\""));
    assert!(stdout.contains("\"program\" -> \"expression\""));
}

#[test]
/// The start symbol (first production) carries `shape=doublecircle` in DOT output.
fn graph_dot_start_symbol_doublecircle() {
    let path = write_tmp("ts_bnf_graph_start.bnf", GRAPH_BNF);
    let out = tool().args(["graph"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"program\" [shape=doublecircle]"));
}

#[test]
/// `%axiom` overrides declaration order: the named rule carries `shape=doublecircle`.
fn graph_dot_axiom_is_start_symbol() {
    let bnf = indoc! {"
        %axiom expression
        program -> statement expression ;
        statement -> 'let' /[a-z]+/ ;
        expression -> term '+' term ;
        term -> /[0-9]+/ ;
    "};
    let path = write_tmp("ts_bnf_graph_axiom.bnf", bnf);
    let out = tool().args(["graph"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"expression\" [shape=doublecircle]"));
    assert!(!stdout.contains("\"program\" [shape=doublecircle]"));
}

#[test]
/// An undefined reference produces a `style=dashed` node and a stderr warning.
fn graph_dot_undefined_ref_dashed_and_warns() {
    let path = write_tmp("ts_bnf_graph_undef.bnf", GRAPH_UNDEF_BNF);
    let out = tool().args(["graph"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("\"extern_rule\" [style=dashed]"));
    assert!(stderr.contains("extern_rule") && stderr.contains("not defined"));
}

#[test]
/// Mermaid output starts with `graph TD` and uses `★` for the start symbol.
fn graph_mermaid_basic_output() {
    let path = write_tmp("ts_bnf_graph_mermaid.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--format", "mermaid"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("graph TD"));
    assert!(stdout.contains("★"));
    assert!(stdout.contains("program"));
}

#[test]
/// Mermaid output marks undefined references with `⚠` and warns on stderr.
fn graph_mermaid_undefined_ref_warns() {
    let path = write_tmp("ts_bnf_graph_mermaid_undef.bnf", GRAPH_UNDEF_BNF);
    let out = tool()
        .args(["graph", "--format", "mermaid"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("⚠"));
    assert!(stderr.contains("extern_rule"));
}

#[test]
/// `--start` restricts the graph to the reachable subgraph; unreachable rules are absent.
fn graph_start_prunes_unreachable() {
    let path = write_tmp("ts_bnf_graph_prune.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--start", "expression"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("\"statement\""),
        "unreachable rule 'statement' must not appear in output"
    );
    assert!(stdout.contains("expression"));
}

#[test]
/// `--start` with an unknown rule name exits non-zero.
fn graph_start_unknown_rule_exits_nonzero() {
    let path = write_tmp("ts_bnf_graph_bad_start.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--start", "no_such_rule"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
/// `--format pdf` without `-o` exits non-zero with an error message.
fn graph_pdf_without_output_exits_nonzero() {
    let path = write_tmp("ts_bnf_graph_pdf.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--format", "pdf"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("requires -o"));
}

#[test]
/// `--format png` without `-o` exits non-zero with an error message.
fn graph_png_without_output_exits_nonzero() {
    let path = write_tmp("ts_bnf_graph_png.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--format", "png"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("requires -o"));
}

#[test]
/// Mermaid output can be written to a file with `-o`.
fn graph_mermaid_output_to_file() {
    let path = write_tmp("ts_bnf_graph_mermaid_out.bnf", GRAPH_BNF);
    let out_path = std::env::temp_dir().join("ts_bnf_graph_mermaid_out.mmd");
    let out = tool()
        .args(["graph", "--format", "mermaid", "-o"])
        .arg(&out_path)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let content = fs::read_to_string(&out_path).unwrap();
    assert!(content.starts_with("graph TD"));
}

#[test]
/// An unknown `--format` value exits non-zero with a helpful message.
fn graph_unknown_format_errors() {
    let path = write_tmp("ts_bnf_graph_badfmt.bnf", GRAPH_BNF);
    let out = tool()
        .args(["graph", "--format", "tikz"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown format 'tikz'"));
}

#[test]
/// `--format svg` without Graphviz on PATH exits non-zero with an install hint.
fn graph_svg_without_dot_on_path_errors() {
    let path = write_tmp("ts_bnf_graph_nodot.bnf", GRAPH_BNF);
    let out = tool()
        .env("PATH", "")
        .args(["graph", "--format", "svg"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("`dot` not found on PATH"));
}

#[test]
/// `--format svg` renders via Graphviz, both to stdout and to a file with `-o`.
fn graph_svg_renders_via_graphviz() {
    if std::process::Command::new("dot")
        .arg("-V")
        .output()
        .is_err()
    {
        eprintln!("skipping: graphviz `dot` not installed");
        return;
    }
    let path = write_tmp("ts_bnf_graph_svg.bnf", GRAPH_BNF);

    let out = tool()
        .args(["graph", "--format", "svg"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("<svg"));

    let out_path = std::env::temp_dir().join("ts_bnf_graph_out.svg");
    let out = tool()
        .args(["graph", "--format", "svg", "-o"])
        .arg(&out_path)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(fs::read_to_string(&out_path).unwrap().contains("<svg"));
}

#[test]
/// DOT output can be written to a file with `-o`.
fn graph_dot_output_to_file() {
    let path = write_tmp("ts_bnf_graph_out.bnf", GRAPH_BNF);
    let out_path = std::env::temp_dir().join("ts_bnf_graph_out.dot");
    let out = tool()
        .args(["graph", "-o"])
        .arg(&out_path)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let content = fs::read_to_string(&out_path).unwrap();
    assert!(content.contains("digraph grammar {"));
}

// ── syntax errors (#200) ──────────────────────────────────────────────────────

/// A grammar with a single tree-sitter syntax error (`=>` is not a valid arrow).
const SYNTAX_ERROR_BNF: &str = indoc! {"
    root => 'a' ;
"};

/// A grammar with two independent syntax errors on separate lines.
const TWO_SYNTAX_ERRORS_BNF: &str = indoc! {"
    root -> ;
    foo --> bar ;
"};

#[test]
/// `check` reports syntax errors on stderr with file:line:col and a snippet, exiting 2.
fn check_syntax_error_reports_location_and_snippet() {
    let path = write_tmp("ts_bnf_check_synerr.bnf", SYNTAX_ERROR_BNF);
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "expected exit 2");
    let stderr = String::from_utf8(out.stderr).unwrap();
    let expected = format!(
        "error: syntax error at {}:1:1 near 'root => 'a' ;'",
        path.display()
    );
    assert!(
        stderr.contains(&expected),
        "stderr missing located message: {stderr}"
    );
}

#[test]
/// `check --json` diagnostics carry the location inside the message.
fn check_json_syntax_error_carries_location() {
    let path = write_tmp("ts_bnf_check_synerr_json.bnf", SYNTAX_ERROR_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "expected exit 2");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = parsed["diagnostics"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["severity"], "error");
    let message = arr[0]["message"].as_str().unwrap();
    assert!(
        message.contains(":1:1 near 'root => 'a' ;'"),
        "message missing location: {message}"
    );
}

#[test]
/// Multiple syntax errors in one file are each reported with their own location.
fn check_reports_multiple_syntax_errors() {
    let path = write_tmp("ts_bnf_check_synerr_multi.bnf", TWO_SYNTAX_ERRORS_BNF);
    let out = tool().args(["check"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "expected exit 2");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        stderr.matches("syntax error at").count(),
        2,
        "expected two located diagnostics: {stderr}"
    );
    assert!(
        stderr.contains(":1:8: missing 'pattern'"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains(":2:5 near '-'"), "stderr: {stderr}");
}

#[test]
/// `convert` aborts on syntax errors with a located message on stderr, exiting 1.
fn convert_syntax_error_aborts_with_located_message() {
    let path = write_tmp("ts_bnf_convert_synerr.bnf", SYNTAX_ERROR_BNF);
    let out = tool().args(["convert"]).arg(&path).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "expected exit 1");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("error: syntax error at") && stderr.contains(":1:1 near"),
        "stderr missing located message: {stderr}"
    );
}
