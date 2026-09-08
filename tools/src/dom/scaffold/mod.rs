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
use crate::util::{hyphens_to_underscores, resolve_output_dir};

use super::grammar_js::{GrammarJs, run_generate};
use super::types::Grammar;
use super::visitor::{check_visitor, render_visitor};

/// Reads and writes the scaffolded crate's `ts-bnf-tool.toml`: the record of
/// what it was scaffolded from (root grammar filename, name, `--ast-types`,
/// `--merge-config`), so a rerun can find and validate against it.
pub mod config;

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
///
/// `name` is passed through to the target-language emitter (currently
/// [`rust::render`]) exactly as given — raw, possibly hyphenated. Whether
/// and where that matters (e.g. a scaffolded Rust crate keeps hyphens in
/// `Cargo.toml`'s `[package] name` but needs a normalized identifier
/// everywhere else) is entirely that emitter's own call: this module stays
/// agnostic to what a "valid identifier" even means in the target language
/// (#378).
///
/// `bundled_bnf_paths` (root first, then its `%include` closure) is plain
/// data computed by the caller (`run_scaffold`, via `bundle_grammar_sources`)
/// — passing it through keeps this function itself free of filesystem
/// access; it's only used for the Makefile template's prerequisite list.
pub fn render_scaffold(
    grammar: &Grammar,
    name: &str,
    source: &str,
    no_header: bool,
    ast_types: bool,
    merge_config: Option<&MergeConfig>,
    bundled_bnf_paths: &[PathBuf],
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
        files: rust::render(
            name,
            no_header,
            visitor_source,
            ast_source,
            bundled_bnf_paths,
        ),
    })
}

/// Everything [`run_scaffold`] needs. Grouped into one struct rather than
/// grown as a positional parameter list (past `clippy::too_many_arguments`'s
/// threshold once bundling-related fields were added) — see the fields
/// below for what each one means.
pub struct ScaffoldRequest<'a> {
    /// The grammar to scaffold.
    pub grammar: &'a Grammar,
    /// Grammar/crate name, passed through as given (see [`render_scaffold`]).
    pub name: &'a str,
    /// Source label shown in generated-file header comments (`<stdin>` for stdin).
    pub source: &'a str,
    /// Output directory for the generated crate (default: `./<name>`).
    pub output_dir: Option<&'a str>,
    /// Suppress generated-file header comments.
    pub no_header: bool,
    /// Generate ast-types files.
    pub ast_types: bool,
    /// `--merge-config`, already parsed.
    pub merge_config: Option<&'a MergeConfig>,
    /// The effective root grammar filename resolved by the caller — `"-"`
    /// only on a genuinely first-ever stdin scaffold.
    pub root_filename: &'a str,
    /// The root grammar's raw source text, already read by the caller (so
    /// stdin isn't read twice).
    pub root_source: &'a str,
    /// `true` when a `ts-bnf-tool.toml` already existed in the output
    /// directory before this run.
    pub already_bundled: bool,
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
pub fn run_scaffold(request: &ScaffoldRequest) -> Result<(), Box<dyn Error>> {
    let &ScaffoldRequest {
        grammar,
        name,
        source,
        output_dir,
        no_header,
        ast_types,
        merge_config,
        root_filename,
        root_source,
        already_bundled,
    } = request;

    check_visitor(grammar).map_err(|msg| -> Box<dyn Error> { msg.into() })?;
    if let Some(config) = merge_config {
        check_merge_config(grammar, config).map_err(|msg| -> Box<dyn Error> { msg.into() })?;
        check_root_rule_not_merged(grammar, config)
            .map_err(|msg| -> Box<dyn Error> { msg.into() })?;
    }

    // tree-sitter's own `grammar()` call hard-rejects a `name` containing
    // `-` (throws "must not ... contain non-word characters"), so the
    // tree-sitter grammar name is always the normalized identifier form —
    // never the raw `name` this function otherwise uses unchanged (the
    // output directory below, and everything `render_scaffold` decides for
    // itself) (#378).
    let grammar_name = hyphens_to_underscores(name);
    let grammar_js = GrammarJs {
        grammar,
        name: &grammar_name,
        source,
        no_header,
    };
    run_generate(&grammar_js, output_dir)?;

    let dir = resolve_output_dir(output_dir, name);

    // Bundling and `ts-bnf-tool.toml` are language-agnostic — computed here
    // rather than folded into `render_scaffold`, which stays scoped to
    // target-language rendering (see its own doc comment). Done before that
    // call so the bundled `.bnf` paths can be threaded into it as plain
    // data, for the Makefile template's prerequisite list.
    let (bundled_files, bundle_warnings) =
        bundle_grammar_sources(grammar, root_filename, root_source);
    let bundled_grammar_basename = if root_filename == "-" {
        "grammar.bnf".to_string()
    } else {
        Path::new(root_filename)
            .file_name()
            .and_then(|f| f.to_str())
            .expect("root grammar filename always has a basename")
            .to_string()
    };
    // The "bundled a copy, edit that from now on" note only applies the
    // first time a genuinely separate copy is created: a rerun (a prior
    // config already existed) never triggers it, and neither does the
    // in-place workflow (scaffolding directly into the directory the `.bnf`
    // already lives in), where the bundle destination and the source are
    // the same file and no copy ever happens. Checked before the write loop
    // below, since afterwards `dest` always exists.
    let dest = dir.join(&bundled_grammar_basename);
    let is_fresh_external_bundle = !already_bundled && !dest.exists();

    // Root first, matching the Makefile template's own convention (see
    // `rust::render`'s `makefile`) — the rest of the include closure
    // follows in whatever order `bundle_grammar_sources` produced it.
    let mut bundled_bnf_paths = vec![PathBuf::from(&bundled_grammar_basename)];
    bundled_bnf_paths.extend(
        bundled_files
            .iter()
            .map(|f| f.path.clone())
            .filter(|p| p != Path::new(&bundled_grammar_basename)),
    );

    let mut crate_files = render_scaffold(
        grammar,
        name,
        source,
        no_header,
        ast_types,
        merge_config,
        &bundled_bnf_paths,
    )
    .map_err(|msg| -> Box<dyn Error> { msg.into() })?;

    let effective_config = config::ScaffoldConfig {
        grammar: bundled_grammar_basename,
        name: name.to_string(),
        ast_types,
        merge_config: merge_config.cloned(),
    };
    crate_files.files.extend(bundled_files);
    crate_files.files.push(ScaffoldFile {
        path: PathBuf::from("ts-bnf-tool.toml"),
        content: config::render_scaffold_config(&effective_config),
        preserve_existing: true,
    });

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
    config::ensure_scaffold_config_up_to_date(&dir, &effective_config)?;

    if is_fresh_external_bundle {
        eprintln!(
            "bundled a copy of {} into {}; edit that copy from now on — {source} is no longer \
             read",
            effective_config.grammar,
            dir.display()
        );
    }
    for warning in &bundle_warnings {
        eprintln!("{warning}");
    }

    if ast_types {
        ensure_lib_rs_declares_ast_module(&dir)?;
    }

    Ok(())
}

/// Bundles the grammar source(s) that produced `grammar` into the scaffold's
/// output as [`ScaffoldFile`]s: the root file plus its full `%include`
/// closure, each preserving its path relative to the root file's own
/// directory. This is DOM-driven and target-language-agnostic (see this
/// module's own doc comment for why that split matters) — nothing here is
/// Rust-specific, so it lives here rather than in `rust.rs`.
///
/// `root_filename` is `"-"` only on a genuinely first-ever stdin scaffold
/// (stdin can't contain `%include`, so there's no closure to walk); it's
/// bundled under the fixed name `grammar.bnf` using `root_source` directly,
/// since there is no on-disk file to re-read.
///
/// Otherwise, every distinct filename recorded on `grammar`'s productions —
/// the root file's own, plus every `%include`d file's resolved path (see
/// `visit_include_directive` in `tools/src/visitors.rs`) — is canonicalized
/// and compared against the root file's own canonical directory. One that
/// resolves inside it is bundled at its relative path; one that resolves
/// outside it is left external, reported as a warning instead (no bundling,
/// no mismatch/refusal logic — unlike the root grammar itself, an `%include`
/// graph is freely re-bundled every run). Canonicalizing every filename, not
/// just the root's, matters because a root-level production's recorded
/// filename is `root_filename` verbatim (whatever form the caller passed
/// in), while an included production's is already absolute — without
/// canonicalizing both sides, the root file's own entry would never match
/// its own directory.
fn bundle_grammar_sources(
    grammar: &Grammar,
    root_filename: &str,
    root_source: &str,
) -> (Vec<ScaffoldFile>, Vec<String>) {
    if root_filename == "-" {
        return (
            vec![ScaffoldFile {
                path: PathBuf::from("grammar.bnf"),
                content: root_source.to_string(),
                preserve_existing: true,
            }],
            Vec::new(),
        );
    }

    let root_dir = Path::new(root_filename)
        .canonicalize()
        .expect("root grammar file was already successfully read while parsing")
        .parent()
        .expect("a file path always has a parent directory")
        .to_path_buf();

    let mut filenames: Vec<&String> = grammar.productions.values().map(|p| &p.filename).collect();
    filenames.sort_unstable();
    filenames.dedup();

    let mut files = Vec::new();
    let mut warnings = Vec::new();
    for filename in filenames {
        let canonical = Path::new(filename)
            .canonicalize()
            .expect("grammar source file was already successfully read while parsing");
        match canonical.strip_prefix(&root_dir) {
            Ok(relative) => {
                let content = fs::read_to_string(&canonical)
                    .expect("grammar source file was already successfully read while parsing");
                files.push(ScaffoldFile {
                    path: relative.to_path_buf(),
                    content,
                    preserve_existing: true,
                });
            }
            Err(_) => {
                warnings.push(format!(
                    "note: {} is outside {}; leaving it unbundled",
                    canonical.display(),
                    root_dir.display()
                ));
            }
        }
    }

    (files, warnings)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{nt, p_named};

    /// Stdin has no on-disk file to bundle from, so the raw text already
    /// read is written verbatim under the fixed name `grammar.bnf`, and the
    /// grammar's own productions (which carry no usable filenames for
    /// stdin) are never consulted.
    #[test]
    fn bundle_grammar_sources_stdin_bundles_fixed_name() {
        let grammar = Grammar::from_rules([p_named("root", nt("x"), "-")]);

        let (files, warnings) = bundle_grammar_sources(&grammar, "-", "root -> x ;\n");

        assert!(warnings.is_empty());
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("grammar.bnf"));
        assert_eq!(files[0].content, "root -> x ;\n");
        assert!(files[0].preserve_existing);
    }

    /// A root file plus one `%include`d file (as `visit_include_directive`
    /// would have stamped it: an absolute path resolved from the root's own
    /// directory) both get bundled, the included file keeping its path
    /// relative to the root's directory.
    #[test]
    fn bundle_grammar_sources_bundles_root_and_included_files_preserving_relative_paths() {
        let dir = std::env::temp_dir().join("ts_bnf_bundle_sources_include_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        let root_path = dir.join("root.bnf");
        let included_path = dir.join("sub/included.bnf");
        fs::write(&root_path, "root content").unwrap();
        fs::write(&included_path, "included content").unwrap();

        let grammar = Grammar::from_rules([
            p_named("root", nt("x"), root_path.to_str().unwrap()),
            p_named("included", nt("y"), included_path.to_str().unwrap()),
        ]);

        let (mut files, warnings) =
            bundle_grammar_sources(&grammar, root_path.to_str().unwrap(), "unused");

        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        files.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("root.bnf"));
        assert_eq!(files[0].content, "root content");
        assert_eq!(files[1].path, PathBuf::from("sub/included.bnf"));
        assert_eq!(files[1].content, "included content");
    }

    /// A recorded filename resolving outside the root file's own directory
    /// (e.g. an `%include` reaching out of the grammar's own directory tree)
    /// is left unbundled, reported only as a warning.
    #[test]
    fn bundle_grammar_sources_warns_about_file_outside_root_dir() {
        let root_dir = std::env::temp_dir().join("ts_bnf_bundle_sources_outside_root_test");
        let external_dir = std::env::temp_dir().join("ts_bnf_bundle_sources_outside_external_test");
        let _ = fs::remove_dir_all(&root_dir);
        let _ = fs::remove_dir_all(&external_dir);
        fs::create_dir_all(&root_dir).unwrap();
        fs::create_dir_all(&external_dir).unwrap();
        let root_path = root_dir.join("root.bnf");
        let external_path = external_dir.join("external.bnf");
        fs::write(&root_path, "root content").unwrap();
        fs::write(&external_path, "external content").unwrap();

        let grammar = Grammar::from_rules([
            p_named("root", nt("x"), root_path.to_str().unwrap()),
            p_named("external", nt("y"), external_path.to_str().unwrap()),
        ]);

        let (files, warnings) =
            bundle_grammar_sources(&grammar, root_path.to_str().unwrap(), "unused");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("root.bnf"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("external.bnf") && warnings[0].contains("outside"),
            "warning should name the external file and say it's outside the root: {warnings:?}"
        );
    }

    /// The `pub mod visitor;` anchor line missing (e.g. removed by hand)
    /// still gets `pub mod ast;` appended at the end, rather than silently
    /// leaving the file untouched — the defensive fallback branch the
    /// integration tests in `tools/tests/cli.rs` (which always scaffold a
    /// `lib.rs` with the anchor present) never exercise.
    #[test]
    fn ensure_lib_rs_declares_ast_module_appends_when_anchor_missing() {
        let dir = std::env::temp_dir().join("ts_bnf_ensure_lib_rs_no_anchor_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bindings/rust")).unwrap();
        let lib_rs_path = dir.join("bindings/rust/lib.rs");
        fs::write(&lib_rs_path, "// no visitor module declared here\n").unwrap();

        ensure_lib_rs_declares_ast_module(&dir).unwrap();

        let content = fs::read_to_string(&lib_rs_path).unwrap();
        assert!(
            content.contains("pub mod ast;"),
            "pub mod ast; must be appended even without the usual anchor: {content}"
        );
    }
}
