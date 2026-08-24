// Scaffolding for the `scaffold` subcommand's generated crate: every
// hand-authored file it writes besides the parser scaffold `run_generate`
// already produces. Target-language emitters live in sibling modules,
// dispatched from `render_scaffold` below, mirroring `dom::visitor`'s
// derivation/`render_visitor` split — including file *paths* (`Cargo.toml`,
// `bindings/rust/*`, `.gitignore` content), not just file *contents*: those
// are just as target-language-specific as the Rust source text itself, so
// callers outside this module must never hardcode them.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::dom::ast::merge::{MergeConfig, check_merge_config};
use crate::dom::ast::rust::{RustAst, check_root_rule_not_merged, kind_display_name};

use super::grammar_js::{GrammarJs, resolve_output_dir, run_generate};
use super::types::Grammar;
use super::visitor::{check_visitor, render_visitor};

/// The Rust-specific emitter: renders the generated crate's
/// `Cargo.toml`/`build.rs`/`lib.rs`/`examples/walk.rs`/`.gitignore` files,
/// at the paths a Rust crate expects them.
mod rust;

/// One file a scaffolded crate/module/package writes, relative to the crate root.
pub struct ScaffoldFile {
    /// Path relative to the crate's output directory, e.g. `"Cargo.toml"` or
    /// `"bindings/rust/build.rs"`.
    pub path: PathBuf,
    /// The file's full contents.
    pub content: String,
    /// When `true`, an existing file at `path` is left untouched (for files
    /// the tutorial invites users to hand-edit); when `false`, it is always
    /// overwritten (for files that are purely derived from the grammar).
    pub preserve_existing: bool,
}

/// The full set of hand-authored files a scaffolded crate/module/package needs.
pub struct ScaffoldCrate {
    /// Every file to write, each relative to the crate's output directory.
    pub files: Vec<ScaffoldFile>,
}

/// Renders a [`ScaffoldCrate`] for `grammar` in the tool's target language.
///
/// This is the one entry point callers outside this module (`main.rs`) go
/// through: it, not any individual emitter function, is what `dom`
/// re-exports. Adding a second target language means adding a match arm
/// here, not new per-file imports or hardcoded paths at every call site.
///
/// Rust is currently the only target, so there's nothing to select between
/// yet. Once a second language emitter exists, this signature will need a
/// target-language parameter (e.g. an enum) to dispatch on, alongside the
/// added match arm.
pub fn render_scaffold(
    grammar: &Grammar,
    name: &str,
    source: &str,
    no_header: bool,
    ast_types: bool,
    merge_config: Option<&MergeConfig>,
) -> Result<ScaffoldCrate, String> {
    let visitor_source = render_visitor(grammar, name, source, no_header)?;
    let ast_source = if ast_types {
        // `kind_display_name`, not bare `pascal_case`: a `passthrough`
        // entry can rename the root rule's own struct, and `examples/ast.rs`
        // must `use` it under the name it's actually declared under.
        let root_rule = kind_display_name(
            grammar
                .root_rule()
                .expect("grammar without rules should have been treated earlier"),
            merge_config,
        );
        let ast = RustAst::new(grammar, source, no_header, merge_config)?;
        Some((ast.to_string(), root_rule))
    } else {
        None
    };
    Ok(ScaffoldCrate {
        files: rust::render(name, no_header, visitor_source, ast_source),
    })
}

/// Scaffolds a complete Rust library crate: the parser (via [`run_generate`]),
/// plus every hand-authored file [`render_scaffold`] produces
/// (`Cargo.toml`/`build.rs`/`bindings/rust/lib.rs`/`visitor.rs` matching this
/// repo's own `tree-sitter-bnf` crate's shape, and a runnable
/// `examples/walk.rs` that counts every parsed node using only the trait's
/// default methods) — proof the crate works before the user writes a line
/// of their own code.
///
/// The Rust bindings are hand-authored here rather than produced by shelling
/// out to `tree-sitter init`: that command also scaffolds Node/Python/Go/
/// Swift bindings this Rust-only feature has no use for, and its exact
/// output isn't something this tool controls across `tree-sitter` CLI
/// versions — [`run_generate`]'s existing `tree-sitter generate` step already
/// covers everything the Rust binding needs (`src/parser.c`,
/// `src/node-types.json`).
pub fn run_scaffold(
    grammar: &Grammar,
    name: &str,
    source: &str,
    output_dir: Option<&str>,
    no_header: bool,
    ast_types: bool,
    merge_config: Option<&MergeConfig>,
) -> Result<(), Box<dyn Error>> {
    check_visitor(grammar).map_err(|msg| -> Box<dyn Error> { msg.into() })?;
    if let Some(config) = merge_config {
        check_merge_config(grammar, config).map_err(|msg| -> Box<dyn Error> { msg.into() })?;
        check_root_rule_not_merged(grammar, config)
            .map_err(|msg| -> Box<dyn Error> { msg.into() })?;
    }

    let grammar_js = GrammarJs {
        grammar,
        name,
        source,
        no_header,
    };
    run_generate(&grammar_js, output_dir)?;

    let dir = resolve_output_dir(output_dir, name);
    let crate_files = render_scaffold(grammar, name, source, no_header, ast_types, merge_config)
        .map_err(|msg| -> Box<dyn Error> { msg.into() })?;
    for file in crate_files.files {
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if file.preserve_existing {
            write_if_absent(&path, &file.content)?;
        } else {
            fs::write(&path, &file.content)?;
        }
    }

    if ast_types {
        ensure_lib_rs_declares_ast_module(&dir)?;
    }

    Ok(())
}

/// Writes `content` to `path` unless a file already exists there — same
/// never-clobber guard `run_generate`'s `tree-sitter.json` write already
/// uses, applied here to the files [`render_scaffold`] generates that the
/// tutorial invites users to hand-edit afterwards (`Cargo.toml`,
/// `bindings/rust/lib.rs`, `examples/walk.rs`). Re-running `scaffold` after a
/// grammar change must not destroy those edits, unlike
/// `bindings/rust/visitor.rs` and the parser scaffold, which are genuinely
/// derived and are meant to be regenerated every time.
fn write_if_absent(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, content)?;
    Ok(())
}

/// Patches `bindings/rust/lib.rs` (already written by the caller's own file
/// loop, fresh or preserved) to add `pub mod ast;` if it's missing.
///
/// `write_if_absent` above leaves an existing `lib.rs` completely
/// untouched — including the case where [`rust::render`]'s freshly
/// computed content would have included `pub mod ast;` this time, but
/// never gets written because the file already exists. A `lib.rs` scaffolded
/// before `--ast-types` was ever passed therefore permanently lacks the
/// declaration even once a later rerun adds the flag, leaving
/// `examples/ast.rs`'s `use {crate}::ast::{Root};` an unresolved import
/// (#360). Only called when `ast_types` is `true`, so a rerun that never
/// passes `--ast-types` is untouched, matching `write_if_absent`'s
/// documented "won't touch them" guarantee for every other case.
///
/// Inserted right after the `pub mod visitor;` line — [`rust::render`]'s own
/// insertion point — so the patched file ends up matching what a fresh
/// scaffold would have produced; appended at the end as a defensive
/// fallback if that anchor line isn't found (e.g. removed by hand). A no-op
/// if `pub mod ast;` is already present, which covers every ordinary
/// `--ast-types` run, fresh or rerun.
fn ensure_lib_rs_declares_ast_module(dir: &Path) -> Result<(), Box<dyn Error>> {
    let path = dir.join("bindings/rust/lib.rs");
    let content = fs::read_to_string(&path)?;
    if content.contains("pub mod ast;") {
        return Ok(());
    }

    let anchor = "pub mod visitor;\n";
    let patched = match content.find(anchor) {
        Some(idx) => {
            let mut patched = content;
            patched.insert_str(idx + anchor.len(), "pub mod ast;\n");
            patched
        }
        None => format!("{content}\npub mod ast;\n"),
    };
    fs::write(&path, patched)?;
    Ok(())
}
