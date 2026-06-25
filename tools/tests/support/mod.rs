//! Shared support for tests that shell out to the real `tree-sitter` CLI (#235).
//!
//! These tests verify that `convert --generate`'s output actually compiles and
//! behaves correctly under upstream `tree-sitter`, not just that our own `check`
//! is satisfied. They must degrade gracefully — skip rather than fail — when the
//! CLI is absent or too old, since it is not a build dependency of this crate.
//!
//! ## Verification scope per directive
//!
//! Per-directive tests run against a real subprocess CLI, which constrains how
//! deep "behavior" verification can go. The scope below is decided once here so
//! individual tests implement against a settled decision rather than improvising:
//!
//! - `%axiom`, `%word`, `%extras`, `%conflicts`, `%precedences`, `%inline` — full
//!   parse-behavior checks. `tree-sitter generate` then `tree-sitter parse` on a
//!   sample input, asserting on the printed s-expression (root symbol, presence/
//!   absence of named nodes, no `ERROR` nodes, tree shape).
//! - `%supertypes` — compile-only. Supertype membership has no footprint in
//!   `tree-sitter parse`'s plain s-expression output; it is only observable via
//!   the generated `src/node-types.json`. Tests assert `generate` succeeds and
//!   inspect that file, not parse output.
//! - `%externals` — compile-only. Exercising an external token at parse time
//!   requires a compiled-and-linked external scanner (hand-written C), which is
//!   out of reach for a lightweight CI fixture. Tests assert `generate` succeeds
//!   and that `parser.c`/`node-types.json` reference the external token name(s).
//! - `%include` — no tree-sitter equivalent (BNF-only composition resolved away
//!   before codegen), so no real-CLI test applies; covered by existing
//!   parse/check-level tests instead (see 235.23).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns the installed tree-sitter CLI version as `(major, minor)`, or `None` if not found.
pub fn tree_sitter_version() -> Option<(u32, u32)> {
    let out = Command::new("tree-sitter").arg("--version").output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    // output: "tree-sitter 0.26.9"
    let ver = s.trim().split_whitespace().nth(1)?;
    let mut parts = ver.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Runs `ts-bnf-tool convert --generate` on `bnf_source` into a freshly-cleared
/// directory named `dir_name` under the system temp dir, optionally passing
/// `--name`. Panics if generation does not exit successfully. Returns the output
/// directory so callers can locate generated files
/// (`src/parser.c`, `src/node-types.json`, `tree-sitter.json`, ...) themselves.
pub fn generate(dir_name: &str, name: Option<&str>, bnf_source: &str) -> PathBuf {
    let bnf_path = std::env::temp_dir().join(format!("{dir_name}.bnf"));
    std::fs::write(&bnf_path, bnf_source).unwrap();

    let out_dir = std::env::temp_dir().join(dir_name);
    let _ = std::fs::remove_dir_all(&out_dir);

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ts-bnf-tool"));
    cmd.args(["convert", "--generate"]);
    if let Some(name) = name {
        cmd.args(["--name", name]);
    }
    cmd.arg("--output-dir").arg(&out_dir).arg(&bnf_path);

    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "convert --generate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out_dir
}

/// Writes `input` to a file under `out_dir` and runs `tree-sitter parse` on it
/// with `out_dir` as the working directory, so the CLI picks up the grammar
/// generated there. Panics if the parse subprocess does not exit successfully.
/// Returns the captured stdout (the printed s-expression).
pub fn parse(out_dir: &Path, input: &str) -> String {
    let input_path = out_dir.join("sample-input.txt");
    std::fs::write(&input_path, input).unwrap();

    let out = Command::new("tree-sitter")
        .arg("parse")
        .arg(&input_path)
        .current_dir(out_dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "tree-sitter parse failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
