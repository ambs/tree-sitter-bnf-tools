// Scaffolding for the `library` subcommand's generated crate: every
// hand-authored file it writes besides the parser scaffold `run_generate`
// already produces. Target-language emitters live in sibling modules,
// dispatched from `render_library` below, mirroring `dom::visitor`'s
// derivation/`render_visitor` split — including file *paths* (`Cargo.toml`,
// `bindings/rust/*`, `.gitignore` content), not just file *contents*: those
// are just as target-language-specific as the Rust source text itself, so
// callers outside this module must never hardcode them.

use std::path::PathBuf;

use super::types::Grammar;
use super::visitor::render_visitor;

/// The Rust-specific emitter: renders the generated crate's
/// `Cargo.toml`/`build.rs`/`lib.rs`/`examples/walk.rs`/`.gitignore` files,
/// at the paths a Rust crate expects them.
mod rust;

/// One file a scaffolded library crate writes, relative to the crate root.
pub struct LibraryFile {
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

/// The full set of hand-authored files a scaffolded library crate needs.
pub struct LibraryCrate {
    /// Every file to write, each relative to the crate's output directory.
    pub files: Vec<LibraryFile>,
}

/// Renders a [`LibraryCrate`] for `grammar` in the tool's target language.
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
pub fn render_library(
    grammar: &Grammar,
    name: &str,
    source: &str,
    no_header: bool,
) -> Result<LibraryCrate, String> {
    let visitor_source = render_visitor(grammar, name, source, no_header)?;
    Ok(LibraryCrate {
        files: rust::render(name, no_header, visitor_source),
    })
}
