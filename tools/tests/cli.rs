//! End-to-end tests for the `ts-bnf-tool` binary.
//!
//! These run the compiled binary as a subprocess so that the full `main()`
//! dispatch — including the `--json` output branches — is exercised.
use indoc::indoc;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

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

/// A left-recursive grammar that produces an error.
const ERROR_BNF: &str = indoc! {"
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
/// All five expected row labels are present in the plain-text summary.
fn check_summary_plain_contains_all_rows() {
    let path = write_tmp("ts_bnf_summary_rows.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--summary"])
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    for label in &[
        "Rules",
        "Terminals",
        "Undefined refs",
        "Left-recursive",
        "FIRST sets",
    ] {
        assert!(stdout.contains(label), "missing row: {label}");
    }
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
