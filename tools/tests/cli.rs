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
/// Rerunning `convert --generate` over an existing output directory must not
/// clobber a hand-edited `queries/highlights.scm` — the tutorial invites
/// users to refine that file by hand after the first generation (#375).
fn generate_rerun_preserves_hand_edited_highlights_scm() {
    let path = write_tmp("ts_bnf_gen_highlights_rerun.bnf", CLEAN_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_gen_highlights_rerun_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let run = || {
        tool()
            .args(["convert", "--generate", "--output-dir"])
            .arg(&out_dir)
            .arg(&path)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(first.status.success(), "first generate must succeed");

    let highlights = out_dir.join("queries").join("highlights.scm");
    std::fs::write(&highlights, "; user-added-marker\n(string) @string\n").unwrap();

    let second = run();
    assert!(second.status.success(), "second generate must succeed");

    let content = std::fs::read_to_string(&highlights).unwrap();
    assert!(
        content.contains("user-added-marker"),
        "queries/highlights.scm must not be clobbered on rerun: {content}"
    );
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

// ── %word real-CLI test (#265) ───────────────────────────────────────────────

/// Grammar with `%word identifier` so the real parser performs keyword
/// extraction: `if` is recognized as the keyword token while `ifx` is treated
/// as a single identifier rather than a mis-split keyword + dangling suffix.
const WORD_BNF: &str = indoc! {"
    %axiom program
    %word identifier

    program -> stmt+ ;
    stmt -> kw_if name ';' | name ';' ;
    kw_if -> 'if' ;
    identifier -> /[a-z]+/ ;
    name -> identifier ;
"};

#[test]
/// With `%word identifier`, `if` parses as the keyword node (`kw_if`) while
/// `ifx` parses as a single identifier — not as `if` followed by a dangling
/// suffix.
fn generate_word_keyword_distinguished_from_prefixed_identifier() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate("ts_bnf_gen_word_project", Some("wordtest"), WORD_BNF);

    // `if x;` — `if` must be recognised as the keyword, not an identifier.
    let kw_out = support::parse(&out_dir, "if x;");
    assert!(
        !kw_out.contains("ERROR"),
        "expected no ERROR for 'if x;'; got: {kw_out}"
    );
    assert!(
        kw_out.contains("(kw_if"),
        "expected '(kw_if' node for keyword 'if'; got: {kw_out}"
    );

    // `ifx;` — `ifx` must be a single identifier, not split at the keyword prefix.
    let id_out = support::parse(&out_dir, "ifx;");
    assert!(
        !id_out.contains("ERROR"),
        "expected no ERROR for 'ifx;'; got: {id_out}"
    );
    assert!(
        !id_out.contains("(kw_if"),
        "'ifx' must not produce a kw_if node; got: {id_out}"
    );
    assert!(
        id_out.contains("(identifier"),
        "expected '(identifier' node for 'ifx'; got: {id_out}"
    );
}

// ── %precedences real-CLI test (#268) ────────────────────────────────────────

/// Grammar with a classic `+`/`*` operator-precedence ambiguity.
/// `%precedences [mul_expr, add_expr]` declares `mul_expr > add_expr`, so `*`
/// binds tighter than `+`.  `%conflicts` suppresses the within-rule
/// associativity conflicts that would otherwise block generation.
const PRECEDENCES_BNF: &str = indoc! {"
    %axiom expr
    %precedences [mul_expr, add_expr]
    %conflicts [mul_expr]
    %conflicts [add_expr]

    expr -> mul_expr | add_expr | num ;
    mul_expr -> expr '*' expr ;
    add_expr -> expr '+' expr ;
    num -> /[0-9]+/ ;
"};

#[test]
/// With `%precedences [mul_expr, add_expr]`, `*` binds tighter than `+`:
/// parsing `1+2*3` must produce a tree where `mul_expr` is nested inside
/// `add_expr` (i.e. `1+(2*3)`, not `(1+2)*3`).
fn generate_precedences_mul_binds_tighter_than_add() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::generate(
        "ts_bnf_gen_precedences_project",
        Some("precedencestest"),
        PRECEDENCES_BNF,
    );
    let stdout = support::parse(&out_dir, "1+2*3");
    assert!(
        !stdout.contains("ERROR"),
        "expected no ERROR node; got: {stdout}"
    );
    assert!(
        stdout.trim_start().starts_with("(expr "),
        "expected 'expr' as root node; got: {stdout}"
    );
    let add_pos = stdout
        .find("(add_expr")
        .expect("expected '(add_expr' in parse output; got: {stdout}");
    let mul_pos = stdout
        .find("(mul_expr")
        .expect("expected '(mul_expr' in parse output; got: {stdout}");
    assert!(
        add_pos < mul_pos,
        "expected mul_expr nested inside add_expr (1+(2*3)); \
         add_expr at {add_pos}, mul_expr at {mul_pos}; tree: {stdout}"
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

// ── scaffold ─────────────────────────────────────────────────────────────────

#[test]
/// `scaffold`'s `check_visitor` pre-flight runs before `run_generate` (which
/// shells out to the real `tree-sitter` CLI), so this is the one `scaffold`
/// scenario testable without it: exits non-zero with a diagnostic naming
/// both offending kinds, without ever touching the filesystem or spawning
/// `tree-sitter`.
fn scaffold_colliding_kind_names_exits_nonzero() {
    let path = write_tmp(
        "ts_bnf_scaffold_collision.bnf",
        indoc! {"
            fooBar -> 'x' ;
            foo_bar -> 'y' ;
        "},
    );
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_collision_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["scaffold", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        !out_dir.exists(),
        "nothing should be written when the collision check fails first"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("'fooBar'") && stderr.contains("'foo_bar'"),
        "stderr must name both offending kinds: {stderr}"
    );
}

#[test]
/// A grammar name that isn't a valid JavaScript identifier (e.g. a filename
/// stem with a dash, no `--name` override) must fail with `check_grammar_name`'s
/// own diagnostic before anything is written — same as `convert`'s check —
/// rather than reaching `tree-sitter generate` and dying as a raw Node.js
/// stack trace with a partial crate left on disk.
fn scaffold_invalid_grammar_name_exits_nonzero_before_writing() {
    let path = write_tmp("my-lang.bnf", "expr -> 'x' ;\n");
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_invalid_name_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["scaffold", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        !out_dir.exists(),
        "nothing should be written when the grammar-name check fails first"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("'my-lang'") && stderr.contains("not a valid JavaScript identifier"),
        "stderr must carry check_grammar_name's own diagnostic: {stderr}"
    );
}

/// A tiny grammar with a field, used by the `scaffold`-generation tests
/// below: enough to exercise `RustVisitor` genuinely, without needing
/// `visitor_sample.bnf` or `grammar/bnf.bnf`'s own fixtures.
const SCAFFOLD_BNF: &str = indoc! {"
    program -> decl* ;
    decl -> target: ident '=' value: ident ';' ;
    ident -> /[a-z]+/ ;
"};

#[test]
/// `scaffold` writes the parser scaffold (same as `convert --generate`) plus
/// the Rust-specific files this subcommand adds on top of it: `Cargo.toml`,
/// `bindings/rust/build.rs`, `bindings/rust/lib.rs`,
/// `bindings/rust/visitor.rs`, `examples/walk.rs`, `.gitignore`.
fn scaffold_writes_full_crate_layout() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let path = write_tmp("ts_bnf_scaffold_layout.bnf", SCAFFOLD_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_layout_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["scaffold", "--name", "mylang", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "scaffold must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for relative in [
        "grammar.js",
        "queries/highlights.scm",
        "tree-sitter.json",
        "src/parser.c",
        "src/node-types.json",
        "Cargo.toml",
        "bindings/rust/build.rs",
        "bindings/rust/lib.rs",
        "bindings/rust/visitor.rs",
        "examples/walk.rs",
        ".gitignore",
    ] {
        assert!(
            out_dir.join(relative).exists(),
            "{relative} must be created"
        );
    }

    let cargo_toml = std::fs::read_to_string(out_dir.join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("name = \"mylang\""));
    assert!(cargo_toml.contains("path = \"bindings/rust/lib.rs\""));
    assert!(
        cargo_toml.contains("\"examples/*\","),
        "Cargo.toml's include list must ship examples/*, or `cargo package` \
         drops the runnable examples the tutorial tells users to run: {cargo_toml}"
    );

    let lib_rs = std::fs::read_to_string(out_dir.join("bindings/rust/lib.rs")).unwrap();
    assert!(lib_rs.contains("fn tree_sitter_mylang() -> *const ();"));
    assert!(lib_rs.contains("pub mod visitor;"));
    assert!(lib_rs.contains("pub fn parse(source: &str)"));

    let build_rs = std::fs::read_to_string(out_dir.join("bindings/rust/build.rs")).unwrap();
    assert!(
        build_rs.contains("Generated by ts-bnf-tool"),
        "bindings/rust/build.rs is regenerated on every run and must carry \
         the generated-file header by default: {build_rs}"
    );

    let visitor_rs = std::fs::read_to_string(out_dir.join("bindings/rust/visitor.rs")).unwrap();
    assert!(visitor_rs.contains("pub trait Visitor<'tree>"));
    assert!(visitor_rs.contains("fn visit_decl("));

    let walk_rs = std::fs::read_to_string(out_dir.join("examples/walk.rs")).unwrap();
    assert!(walk_rs.contains("use mylang::visitor::{SourceNode, Visitor};"));
    assert!(walk_rs.contains("fn combine(&mut self, _results: Vec<()>)"));
}

#[test]
/// Re-running `scaffold` over an existing generated directory must not
/// destroy hand-edits to the files the tutorial invites users to edit
/// (`Cargo.toml`, `bindings/rust/lib.rs`, `examples/walk.rs`) — but
/// `bindings/rust/visitor.rs` is genuinely derived from the grammar and must
/// still be regenerated every time, since it's meant to track grammar
/// changes automatically.
fn scaffold_rerun_preserves_user_edits_but_regenerates_visitor() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let path = write_tmp("ts_bnf_scaffold_rerun.bnf", SCAFFOLD_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_rerun_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let run = || {
        tool()
            .args(["scaffold", "--name", "mylang", "--output-dir"])
            .arg(&out_dir)
            .arg(&path)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "first scaffold run must succeed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let cargo_toml_path = out_dir.join("Cargo.toml");
    let lib_rs_path = out_dir.join("bindings/rust/lib.rs");
    let walk_rs_path = out_dir.join("examples/walk.rs");
    let visitor_rs_path = out_dir.join("bindings/rust/visitor.rs");
    let highlights_path = out_dir.join("queries/highlights.scm");
    let gitignore_path = out_dir.join(".gitignore");

    let mut cargo_toml = std::fs::read_to_string(&cargo_toml_path).unwrap();
    cargo_toml.push_str("\n# user-added-marker\n");
    std::fs::write(&cargo_toml_path, &cargo_toml).unwrap();

    let mut lib_rs = std::fs::read_to_string(&lib_rs_path).unwrap();
    lib_rs.push_str("\n// user-added-marker\npub mod my_visitor;\n");
    std::fs::write(&lib_rs_path, &lib_rs).unwrap();

    let mut walk_rs = std::fs::read_to_string(&walk_rs_path).unwrap();
    walk_rs.push_str("\n// user-added-marker\n");
    std::fs::write(&walk_rs_path, &walk_rs).unwrap();

    std::fs::write(&visitor_rs_path, "GARBAGE").unwrap();

    std::fs::write(&highlights_path, "; user-added-marker\n").unwrap();

    let mut gitignore = std::fs::read_to_string(&gitignore_path).unwrap();
    gitignore.push_str("/sample.decls\n");
    std::fs::write(&gitignore_path, &gitignore).unwrap();

    let second = run();
    assert!(
        second.status.success(),
        "second scaffold run must succeed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let cargo_toml_after = std::fs::read_to_string(&cargo_toml_path).unwrap();
    assert!(
        cargo_toml_after.contains("user-added-marker"),
        "Cargo.toml must not be clobbered on rerun: {cargo_toml_after}"
    );

    let lib_rs_after = std::fs::read_to_string(&lib_rs_path).unwrap();
    assert!(
        lib_rs_after.contains("user-added-marker") && lib_rs_after.contains("pub mod my_visitor;"),
        "bindings/rust/lib.rs must not be clobbered on rerun: {lib_rs_after}"
    );

    let walk_rs_after = std::fs::read_to_string(&walk_rs_path).unwrap();
    assert!(
        walk_rs_after.contains("user-added-marker"),
        "examples/walk.rs must not be clobbered on rerun: {walk_rs_after}"
    );

    let visitor_rs_after = std::fs::read_to_string(&visitor_rs_path).unwrap();
    assert!(
        !visitor_rs_after.contains("GARBAGE")
            && visitor_rs_after.contains("pub trait Visitor<'tree>"),
        "bindings/rust/visitor.rs must still be regenerated on rerun: {visitor_rs_after}"
    );

    let highlights_after = std::fs::read_to_string(&highlights_path).unwrap();
    assert!(
        highlights_after.contains("user-added-marker"),
        "queries/highlights.scm must not be clobbered on rerun: {highlights_after}"
    );

    let gitignore_after = std::fs::read_to_string(&gitignore_path).unwrap();
    assert!(
        gitignore_after.contains("/sample.decls"),
        ".gitignore must not be clobbered on rerun: {gitignore_after}"
    );
}

#[test]
/// `--no-header` suppresses the generated-file comment on the Rust files
/// this subcommand hand-authors, including `bindings/rust/build.rs` (#376).
fn scaffold_no_header_suppresses_generated_file_comments() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let path = write_tmp("ts_bnf_scaffold_no_header.bnf", SCAFFOLD_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_no_header_project");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["scaffold", "--no-header", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let lib_rs = std::fs::read_to_string(out_dir.join("bindings/rust/lib.rs")).unwrap();
    assert!(!lib_rs.contains("Generated by ts-bnf-tool"));
    assert!(lib_rs.starts_with("//! This crate provides"));

    let build_rs = std::fs::read_to_string(out_dir.join("bindings/rust/build.rs")).unwrap();
    assert!(!build_rs.contains("Generated by ts-bnf-tool"));
    assert!(build_rs.starts_with("fn main() {"));
}

#[test]
/// The generated crate isn't just plausible-looking text: it's a real Cargo
/// project that compiles and runs against the real `tree-sitter`/`cc`
/// toolchain, with zero edits — `cargo run --example walk -- <file>` genuinely
/// parses the file and counts its nodes, cross-checked against an
/// independent direct tree walk rather than a hardcoded literal (same
/// principle `dogfood_visitor_trait_runs_and_counts_rule_nodes` in
/// `tests/visitor.rs` uses for the underlying derivation/emission engine).
fn scaffold_generated_crate_builds_and_walk_example_runs() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::scaffold("ts_bnf_scaffold_e2e_project", "e2elang", SCAFFOLD_BNF);

    let sample = "x = y;\ny = z;\n";
    let stdout = support::run_walk_example(&out_dir, sample);

    let expected = expected_node_count(sample);
    assert!(
        stdout.contains(&format!("{expected} node(s)")),
        "expected a count of {expected} node(s) in output: {stdout}"
    );
}

/// Independently counts the nodes `SCAFFOLD_BNF`'s grammar (`program ->
/// decl*`, `decl -> target: ident '=' value: ident ';'`) produces for
/// `source`, by direct arithmetic over its line count rather than by
/// re-deriving the same traversal the generated example already performs —
/// each `x = y;` line is one `decl` (1) + its `target` `ident` (1) + its
/// `value` `ident` (1) = 3 nodes, plus the root `program` node (1) counted
/// once for the whole input.
fn expected_node_count(source: &str) -> usize {
    let decls = source.lines().filter(|l| !l.trim().is_empty()).count();
    1 + decls * 3
}

// ── scaffold --ast-types ────────────────────────────────────────────────

/// A tiny grammar with a *labeled, repeated* root field, used by the
/// `--ast-types` scaffold tests below: `items: decl*` is exactly the shape
/// `field_value_is_multiple` (`dom/ast/mod.rs`) was added to detect
/// correctly (see `plans/342.3.md`, 342.3.9) — reusing it here means a
/// regression in that fix would fail this end-to-end test too, not just
/// the unit tests it was originally caught by.
const SCAFFOLD_AST_BNF: &str = indoc! {"
    program -> items: decl* ;
    decl -> target: ident '=' value: ident ';' ;
    ident -> /[a-z]+/ ;
"};

#[test]
/// `scaffold --ast-types` writes `bindings/rust/ast.rs` and
/// `examples/ast.rs` on top of the usual crate layout, adds `pub mod ast;`
/// to `lib.rs`, and the generated crate isn't just plausible-looking text:
/// `cargo run --example ast -- <file>` genuinely builds a typed `Program`
/// root node (with its `items: Vec<Decl>` field populated) and
/// pretty-prints it, proving the whole derive→emit→compile→run pipeline
/// works for a real grammar, not just the synthetic ones the unit tests in
/// `dom/ast/rust.rs` construct.
fn scaffold_ast_types_writes_files_and_ast_example_runs() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::scaffold_with_ast_types(
        "ts_bnf_scaffold_ast_e2e_project",
        "astlang",
        SCAFFOLD_AST_BNF,
    );

    for relative in ["bindings/rust/ast.rs", "examples/ast.rs"] {
        assert!(
            out_dir.join(relative).exists(),
            "{relative} must be created by --ast-types"
        );
    }

    let lib_rs = std::fs::read_to_string(out_dir.join("bindings/rust/lib.rs")).unwrap();
    assert!(lib_rs.contains("pub mod ast;"));

    let stdout = support::run_ast_example(&out_dir, "x = y;\ny = z;\n");
    assert!(stdout.contains("Program"), "{stdout}");
    assert!(stdout.contains("items:"), "{stdout}");
    assert!(stdout.contains("Decl"), "{stdout}");
    assert!(stdout.contains("target:"), "{stdout}");
    assert!(stdout.contains("Ident"), "{stdout}");
}

/// A grammar for `--merge-config` end-to-end testing (342.5.8): a
/// `program` root with an `items:` field whose target set is exactly the
/// three loop-kind choice merged below (Rule 2 — reuses `enum Loop`
/// directly), plus a `doc:` field targeting a `passthrough`-renamed
/// `comment` kind.
const SCAFFOLD_MERGE_BNF: &str = indoc! {"
    program -> items: (for_statement | while_statement | repeat_statement)* doc: comment ;
    for_statement -> 'for' ;
    while_statement -> 'while' ;
    repeat_statement -> 'repeat' ;
    comment -> /#[^\\n]*/ ;
"};

/// Merges the three loop kinds into `enum Loop`, and renames `comment` to
/// `DocComment` — the config `SCAFFOLD_MERGE_BNF` is written against.
const SCAFFOLD_MERGE_CONFIG_TOML: &str = indoc! {r#"
    [[merge]]
    target = "Loop"
    from = ["for_statement", "while_statement", "repeat_statement"]

    [[passthrough]]
    kind = "comment"
    target = "DocComment"
"#};

#[test]
/// `scaffold --ast-types --merge-config <path>` end-to-end: the generated
/// `bindings/rust/ast.rs` has the merge-generated `pub enum Loop` (with its
/// `#[allow(private_interfaces)]`) and the `passthrough`-renamed
/// `DocComment`, and does *not* expose any of the three merged-away kinds'
/// structs as `pub` — then, matching the bar
/// `scaffold_ast_types_writes_files_and_ast_example_runs` already holds
/// plain `--ast-types` to, `cargo run --example ast` on the generated crate
/// genuinely builds and runs against real parsed input, proving the full
/// CLI-to-compiled-crate path works, not just that the emitted text looks
/// plausible.
fn scaffold_merge_config_writes_enum_and_hides_merged_structs() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::scaffold_with_merge_config(
        "ts_bnf_scaffold_merge_config_e2e_project",
        "mergelang",
        SCAFFOLD_MERGE_BNF,
        SCAFFOLD_MERGE_CONFIG_TOML,
    );

    let ast_rs = std::fs::read_to_string(out_dir.join("bindings/rust/ast.rs")).unwrap();
    assert!(ast_rs.contains("#[allow(private_interfaces)]\n#[derive(Debug)]\npub enum Loop {"));
    assert!(ast_rs.contains("pub struct DocComment {"));
    assert!(!ast_rs.contains("pub struct ForStatement {"));
    assert!(!ast_rs.contains("pub struct WhileStatement {"));
    assert!(!ast_rs.contains("pub struct RepeatStatement {"));
    assert!(!ast_rs.contains("pub struct Comment {"));

    let stdout = support::run_ast_example(&out_dir, "for while repeat #a comment\n");
    assert!(stdout.contains("Program"), "{stdout}");
    assert!(stdout.contains("ForStatement("), "{stdout}");
    assert!(stdout.contains("WhileStatement("), "{stdout}");
    assert!(stdout.contains("RepeatStatement("), "{stdout}");
    assert!(stdout.contains("DocComment"), "{stdout}");
}

#[test]
/// An invalid `--merge-config` (a kind name with a typo) exits non-zero
/// before any file is written — mirrors
/// `scaffold_colliding_kind_names_exits_nonzero`'s gating pattern, closing
/// the loop on 342.5.6's `check_merge_config` call actually running before
/// `run_generate`/any file write, not just that it's wired up to parse.
fn scaffold_merge_config_invalid_exits_nonzero_before_writing() {
    let bnf_path = write_tmp(
        "ts_bnf_scaffold_merge_config_invalid.bnf",
        SCAFFOLD_MERGE_BNF,
    );
    let config_path = write_tmp(
        "ts_bnf_scaffold_merge_config_invalid.toml",
        indoc! {r#"
            [[merge]]
            target = "Loop"
            from = ["bogus_statement", "while_statement"]
        "#},
    );
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_merge_config_invalid_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = tool()
        .args([
            "scaffold",
            "--ast-types",
            "--merge-config",
            config_path.to_str().unwrap(),
            "--output-dir",
        ])
        .arg(&out_dir)
        .arg(&bnf_path)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        !out_dir.exists(),
        "nothing should be written when merge-config validation fails first"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bogus_statement"), "{stderr}");
}

#[test]
/// `scaffold --ast-types --merge-config <config missing a kind>` (342.6): a
/// merge config that only covers the three loop kinds, leaving `comment`
/// untriaged, still exits 0 and still writes every file — the coverage
/// report is advisory only — but reports the missing kind to stderr.
fn scaffold_merge_config_missing_kind_still_succeeds_but_reports_coverage_gap() {
    let bnf_path = write_tmp(
        "ts_bnf_scaffold_merge_config_coverage_gap.bnf",
        SCAFFOLD_MERGE_BNF,
    );
    let config_path = write_tmp(
        "ts_bnf_scaffold_merge_config_coverage_gap.toml",
        indoc! {r#"
            [[merge]]
            target = "Loop"
            from = ["for_statement", "while_statement", "repeat_statement"]
        "#},
    );
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_merge_config_coverage_gap_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = tool()
        .args([
            "scaffold",
            "--ast-types",
            "--merge-config",
            config_path.to_str().unwrap(),
            "--output-dir",
        ])
        .arg(&out_dir)
        .arg(&bnf_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "an uncovered kind must not fail the command: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ast_rs = std::fs::read_to_string(out_dir.join("bindings/rust/ast.rs")).unwrap();
    assert!(
        ast_rs.contains("pub struct Comment {"),
        "an uncovered kind must still render as an ordinary baseline struct"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("comment"),
        "stderr must report the uncovered kind 'comment'; got: {stderr}"
    );
}

#[test]
/// Without `--ast-types`, no AST-types files are written at all, and
/// `lib.rs` has no `pub mod ast;` — the flag is genuinely opt-in, not
/// always-on.
fn scaffold_without_ast_types_flag_omits_ast_files() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let out_dir = support::scaffold(
        "ts_bnf_scaffold_no_ast_types_project",
        "noastlang",
        SCAFFOLD_AST_BNF,
    );
    assert!(!out_dir.join("bindings/rust/ast.rs").exists());
    assert!(!out_dir.join("examples/ast.rs").exists());
    let lib_rs = std::fs::read_to_string(out_dir.join("bindings/rust/lib.rs")).unwrap();
    assert!(!lib_rs.contains("pub mod ast;"));
}

#[test]
/// Re-running `scaffold --ast-types` must regenerate `bindings/rust/ast.rs`
/// (genuinely derived from the grammar, like `visitor.rs`) while leaving
/// hand-edits to `examples/ast.rs` alone (tutorial-editable, like
/// `examples/walk.rs`) — same "safe to re-run" guarantee
/// `scaffold_rerun_preserves_user_edits_but_regenerates_visitor` already
/// covers for the non-`--ast-types` files.
fn scaffold_ast_types_rerun_regenerates_ast_rs_but_preserves_example() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let path = write_tmp("ts_bnf_scaffold_ast_rerun.bnf", SCAFFOLD_AST_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_ast_rerun_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let run = || {
        tool()
            .args([
                "scaffold",
                "--name",
                "mylang",
                "--ast-types",
                "--output-dir",
            ])
            .arg(&out_dir)
            .arg(&path)
            .output()
            .unwrap()
    };

    let first = run();
    assert!(
        first.status.success(),
        "first scaffold --ast-types run must succeed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let ast_rs_path = out_dir.join("bindings/rust/ast.rs");
    let ast_example_path = out_dir.join("examples/ast.rs");

    let mut ast_example = std::fs::read_to_string(&ast_example_path).unwrap();
    ast_example.push_str("\n// user-added-marker\n");
    std::fs::write(&ast_example_path, &ast_example).unwrap();

    std::fs::write(&ast_rs_path, "GARBAGE").unwrap();

    let second = run();
    assert!(
        second.status.success(),
        "second scaffold --ast-types run must succeed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let ast_example_after = std::fs::read_to_string(&ast_example_path).unwrap();
    assert!(
        ast_example_after.contains("user-added-marker"),
        "examples/ast.rs must not be clobbered on rerun: {ast_example_after}"
    );

    let ast_rs_after = std::fs::read_to_string(&ast_rs_path).unwrap();
    assert!(
        !ast_rs_after.contains("GARBAGE") && ast_rs_after.contains("pub struct Program {"),
        "bindings/rust/ast.rs must still be regenerated on rerun: {ast_rs_after}"
    );
}

#[test]
/// `--ast-types` *added* on a rerun over a crate that was originally
/// scaffolded without it (#360): the first run's `bindings/rust/lib.rs` has
/// no `pub mod ast;` at all, since `--ast-types` was never passed. Unlike
/// `scaffold_ast_types_rerun_regenerates_ast_rs_but_preserves_example`
/// (which starts *with* `--ast-types` on both runs, so `lib.rs` already has
/// the line from the first run and this gap never triggers), the second run
/// here must patch the existing, otherwise-untouched `lib.rs` to add it —
/// proven both by inspecting the file directly and, matching the issue's
/// own repro, by actually building and running the generated crate's
/// `examples/ast.rs` (which fails to compile with an unresolved `use
/// {crate}::ast::...` import without the fix).
fn scaffold_ast_types_added_on_rerun_patches_lib_rs() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }
    let path = write_tmp("ts_bnf_scaffold_ast_added_rerun.bnf", SCAFFOLD_AST_BNF);
    let out_dir = std::env::temp_dir().join("ts_bnf_scaffold_ast_added_rerun_project");
    let _ = std::fs::remove_dir_all(&out_dir);

    let first = tool()
        .args(["scaffold", "--name", "mylang", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "first scaffold run (without --ast-types) must succeed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let lib_rs_path = out_dir.join("bindings/rust/lib.rs");
    let lib_rs_before = std::fs::read_to_string(&lib_rs_path).unwrap();
    assert!(
        !lib_rs_before.contains("pub mod ast;"),
        "sanity check: lib.rs must not yet declare `ast` before --ast-types is ever passed"
    );

    let second = tool()
        .args([
            "scaffold",
            "--name",
            "mylang",
            "--ast-types",
            "--output-dir",
        ])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "second scaffold run (adding --ast-types) must succeed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let lib_rs_after = std::fs::read_to_string(&lib_rs_path).unwrap();
    assert!(
        lib_rs_after.contains("pub mod ast;"),
        "lib.rs must gain `pub mod ast;` once --ast-types is added on a rerun: {lib_rs_after}"
    );

    let stdout = support::run_ast_example(&out_dir, "x = y;\ny = z;\n");
    assert!(stdout.contains("Program"), "{stdout}");
}

#[test]
/// The generated crate must build standing alone even when it lands inside
/// an existing Cargo workspace (the natural "run `ts-bnf-tool scaffold`
/// inside my Rust project" workflow) — `dom::scaffold::rust::cargo_toml`'s `[workspace]`
/// table (added for this reason) stops cargo from walking up to the
/// enclosing workspace root and rejecting the generated crate for not being
/// listed as one of its members.
fn scaffold_generated_crate_builds_inside_an_enclosing_workspace() {
    let Some(version) = support::tree_sitter_version() else {
        return; // tree-sitter not in PATH, skip
    };
    if version < (0, 25) {
        return; // ABI 15 requires tree-sitter >= 0.25
    }

    let workspace_root = std::env::temp_dir().join("ts_bnf_scaffold_enclosing_workspace");
    let _ = std::fs::remove_dir_all(&workspace_root);
    std::fs::create_dir_all(&workspace_root).unwrap();
    std::fs::write(
        workspace_root.join("Cargo.toml"),
        indoc! {r#"
            [workspace]
            members = []
        "#},
    )
    .unwrap();

    let bnf_path = workspace_root.join("crate.bnf");
    std::fs::write(&bnf_path, SCAFFOLD_BNF).unwrap();
    let out_dir = workspace_root.join("generated-crate");

    let out = tool()
        .args(["scaffold", "--name", "wslang", "--output-dir"])
        .arg(&out_dir)
        .arg(&bnf_path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "scaffold must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let sample = "x = y;\n";
    let stdout = support::run_walk_example(&out_dir, sample);
    let expected = expected_node_count(sample);
    assert!(
        stdout.contains(&format!("{expected} node(s)")),
        "expected a count of {expected} node(s) in output: {stdout}"
    );
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

#[test]
/// Without `--annotate`, a field label is not drawn in the SVG output (#182).
fn railroad_without_annotate_omits_field_label() {
    let bnf = "expr -> operand: 'NUM' ;\n";
    let path = write_tmp("ts_bnf_rr_annotate_off.bnf", bnf);
    let out = tool().args(["railroad"]).arg(&path).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("operand"));
}

#[test]
/// `--annotate` draws a field label in a stylesheet-styled labeled box in the SVG output (#182).
fn railroad_annotate_shows_field_label() {
    let bnf = "expr -> operand: 'NUM' ;\n";
    let path = write_tmp("ts_bnf_rr_annotate_on.bnf", bnf);
    let out = tool()
        .args(["railroad", "--annotate"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "railroad --annotate must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("operand:"),
        "field name must appear as an annotation label in the dialect's `name:` syntax"
    );
    assert!(
        stdout.contains("labeledbox annotation-field"),
        "annotation box must carry the labeledbox class (styled by the embedded stylesheet) plus a per-kind class"
    );
    assert!(
        stdout.contains("fill-opacity"),
        "stylesheet must re-state the labeledbox fill in Inkscape-safe syntax (rgba() fills render as opaque black in Inkscape)"
    );
}

#[test]
/// `--annotate` also applies in `--split` mode (#182).
fn railroad_annotate_applies_in_split_mode() {
    let bnf = "expr -> operand: 'NUM' ;\n";
    let path = write_tmp("ts_bnf_rr_annotate_split.bnf", bnf);
    let out_dir = std::env::temp_dir().join("ts_bnf_rr_annotate_split_out");
    let _ = std::fs::remove_dir_all(&out_dir);
    let out = tool()
        .args(["railroad", "--annotate", "--split", "--output-dir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "railroad --annotate --split must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let svg = std::fs::read_to_string(out_dir.join("expr.svg")).unwrap();
    assert!(
        svg.contains("operand:"),
        "field name must appear as an annotation label in split output"
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
// Two separately-recoverable rules (each just missing its pattern),
// blank-line-separated, rather than one rule with a missing pattern
// immediately followed by another with a stray token: that shape's
// error-recovery tree isn't stable across tree-sitter runtime versions —
// 0.26.11 recovered it as two independent `rule` nodes, 0.26.12 merges it
// into one `rule`/`ERROR` node that swallows the second line entirely.
// Confirmed this shape (two clean `rule` nodes, each with its own `MISSING
// pattern`) is identical under both versions.
const TWO_SYNTAX_ERRORS_BNF: &str = indoc! {"
    root -> ;

    foo -> ;
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
    assert!(
        stderr.contains(":3:7: missing 'pattern'"),
        "stderr: {stderr}"
    );
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
