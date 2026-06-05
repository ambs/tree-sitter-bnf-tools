//! End-to-end tests for the `ts-bnf-tool` binary.
//!
//! These run the compiled binary as a subprocess so that the full `main()`
//! dispatch — including the `--json` output branches — is exercised.
use std::fs;
use std::process::Command;

fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ts-bnf-tool"))
}

/// A small, clean grammar with no diagnostics.
const CLEAN_BNF: &str = "expr -> term ('+' term)* ;\nterm -> /[0-9]+/ | '(' expr ')' ;\n";

/// A grammar that produces an "unused rule" warning.
const WARN_BNF: &str = "root -> 'a' ;\nunused -> 'b' ;\n";

/// A left-recursive grammar that produces an error.
const ERROR_BNF: &str = "expr -> expr '+' term | term ;\nterm -> /[0-9]+/ ;\n";

fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    fs::write(&path, content).unwrap();
    path
}

// ── check --json ──────────────────────────────────────────────────────────────

#[test]
fn check_json_clean_exits_zero_and_emits_empty_array() {
    let path = write_tmp("ts_bnf_check_clean.bnf", CLEAN_BNF);
    let out = tool()
        .args(["check", "--json"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success(), "expected exit 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), "[]");
}

#[test]
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
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().any(|d| d["severity"] == "warning"));
}

#[test]
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
    let arr = parsed.as_array().unwrap();
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

// ── highlights ────────────────────────────────────────────────────────────────

/// A grammar with a variety of rule names to exercise the heuristics.
const HIGHLIGHTS_BNF: &str = "\
value      -> string | number | expr ;\n\
string     -> '\"' /[^\"]*/ '\"' ;\n\
number     -> /[0-9]+/ ;\n\
line_comment -> '#' /.*/ ;\n\
expr       -> value '+' value ;\n\
";

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
