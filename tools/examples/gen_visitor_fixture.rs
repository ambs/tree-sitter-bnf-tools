//! Renders a `Visitor<'tree>` trait from a `.bnf` file to stdout — the
//! interim stand-in for the `visitor` CLI subcommand (#210, not yet wired
//! into `main.rs` as of task 210.19) used by `make visitor-fixture` to
//! regenerate `tools/tests/fixtures/visitor_sample.rs`. Delete this once
//! 210.24 lands the real subcommand and point the Makefile at
//! `$(BNF_TOOL) visitor` instead.
//!
//! Usage: `cargo run -p ts-bnf-tool --example gen_visitor_fixture -- <file.bnf> [name]`
//! `name` defaults to the file's stem, matching the eventual subcommand's
//! own `--name` default.

use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

use ts_bnf_tool::dom::RustVisitor;
use ts_bnf_tool::visitors::parse_source;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: gen_visitor_fixture <file.bnf> [name]");
        return ExitCode::FAILURE;
    };
    let name = args.next().unwrap_or_else(|| {
        Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "grammar".to_string())
    });

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("error reading {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (grammar, diagnostics) = match parse_source(&source) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !diagnostics.is_empty() {
        eprintln!("diagnostics: {diagnostics:?}");
        return ExitCode::FAILURE;
    }

    print!(
        "{}",
        RustVisitor {
            grammar: &grammar,
            name: &name,
            source: &path,
            no_header: true,
        }
    );
    ExitCode::SUCCESS
}
