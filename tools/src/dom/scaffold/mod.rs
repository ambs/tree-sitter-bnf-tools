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
) -> Result<ScaffoldCrate, String> {
    let visitor_source = render_visitor(grammar, name, source, no_header)?;
    Ok(ScaffoldCrate {
        files: rust::render(name, no_header, visitor_source),
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
) -> Result<(), Box<dyn Error>> {
    check_visitor(grammar).map_err(|msg| -> Box<dyn Error> { msg.into() })?;

    let grammar_js = GrammarJs {
        grammar,
        name,
        source,
        no_header,
    };
    run_generate(&grammar_js, output_dir)?;

    let dir = resolve_output_dir(output_dir, name);
    let crate_files = render_scaffold(grammar, name, source, no_header)
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
