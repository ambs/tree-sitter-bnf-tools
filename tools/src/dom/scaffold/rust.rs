// Rust-specific templates for the `scaffold` subcommand's generated crate:
// `Cargo.toml`, `bindings/rust/build.rs`, `bindings/rust/lib.rs`,
// `examples/walk.rs`, and `.gitignore`. Everything here is plain string
// rendering — no `Grammar` DOM involved — parameterized only by the crate's
// name, header preference, and (for `bindings/rust/visitor.rs`) the
// already-rendered visitor source.
//
// `name`, as received from `super::render_scaffold`, is the raw name the
// user typed (or the filename stem) — possibly hyphenated. `render` below
// normalizes it once into `identifier_name` and passes that to every
// template except `cargo_toml`, which alone keeps the raw form: Cargo's
// own package-naming convention keeps hyphens (`ts-bnf-tool`,
// `tree-sitter-bnf`, this workspace's own crates), while everything else
// here — the `extern "C"` symbol, the `c_config.compile(...)` target, the
// module path `examples/*.rs` `use`s — needs a strict Rust/C identifier
// (#378).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use indoc::formatdoc;

use crate::util::hyphens_to_underscores;

use crate::dom::ast::BuildMode;

use super::{AstOutput, ScaffoldFile};

/// The Rust edition the generated crate is pinned to: written into its
/// `Cargo.toml` by [`cargo_toml`] and handed to `rustfmt` by
/// [`format_sources`], so the two cannot drift apart (a formatter parsing
/// under a different edition than the compiler will is a bug waiting for the
/// first edition-specific syntax). See [`cargo_toml`] for why it is 2021.
const EDITION: &str = "2021";

/// Renders every file this target's generated crate needs, at the paths a
/// Rust crate expects them: `Cargo.toml`/`bindings/rust/lib.rs`/
/// `examples/walk.rs`/`.gitignore` are left alone on rerun (tutorial- or
/// user-editable), the rest are always regenerated.
pub(super) fn render(
    name: &str,
    no_header: bool,
    visitor_source: String,
    ast_output: Option<AstOutput>,
    bundled_bnf_paths: &[PathBuf],
) -> Vec<ScaffoldFile> {
    let identifier_name = hyphens_to_underscores(name);
    let mut files = vec![
        ScaffoldFile {
            path: PathBuf::from("Cargo.toml"),
            content: cargo_toml(name, ast_output.as_ref().map(|ast| ast.mode)),
            preserve_existing: true,
        },
        ScaffoldFile {
            path: PathBuf::from("bindings/rust/build.rs"),
            content: build_rs(&identifier_name, no_header),
            preserve_existing: false,
        },
        ScaffoldFile {
            path: PathBuf::from("bindings/rust/lib.rs"),
            content: lib_rs(&identifier_name, no_header, ast_output.is_some()),
            preserve_existing: true,
        },
        ScaffoldFile {
            path: PathBuf::from("bindings/rust/visitor.rs"),
            content: visitor_source,
            preserve_existing: false,
        },
        ScaffoldFile {
            path: PathBuf::from("examples/walk.rs"),
            content: walk_example(&identifier_name),
            preserve_existing: true,
        },
        ScaffoldFile {
            path: PathBuf::from(".gitignore"),
            content: "/target\n".to_string(),
            // Not grammar-derived — a hardcoded constant that never changes
            // as the grammar evolves — and the tutorial-editable files
            // above already establish the pattern for content like this:
            // written once, then left for the user to extend (#376).
            preserve_existing: true,
        },
        ScaffoldFile {
            path: PathBuf::from("Makefile"),
            content: makefile(bundled_bnf_paths),
            // Hand-editable, like `Cargo.toml`/`lib.rs`/`.gitignore` above —
            // written once, then left for the user to extend with their own
            // targets.
            preserve_existing: true,
        },
    ];
    if let Some(AstOutput { source, mode }) = ast_output {
        files.push(ScaffoldFile {
            path: PathBuf::from("bindings/rust/ast.rs"),
            content: source,
            preserve_existing: false,
        });
        files.push(ScaffoldFile {
            path: PathBuf::from("examples/ast.rs"),
            content: ast_example(&identifier_name, mode),
            preserve_existing: true,
        });
    }

    files
}

/// Formats the Rust sources among `written` with `rustfmt`, so the generated
/// crate is already what the user's own `cargo fmt --check` expects (#496), or
/// returns the warning to print when that could not be done.
///
/// `written` is every file this run actually wrote, relative to `dir` — not
/// every file the crate has. A preserved file that already existed
/// (`bindings/rust/lib.rs`, `examples/walk.rs`, …) is the user's by now and is
/// never passed in, so a rerun cannot reformat hand-edits. Only `.rs` files
/// are formatted; the rest (`Cargo.toml`, the bundled `.bnf`, …) are skipped
/// here, which is why the caller can hand over the whole list.
///
/// The tool passes `rustfmt` no formatting options of its own — only the
/// crate's [`EDITION`] — so the output is default `rustfmt` style, plus
/// whatever `rustfmt.toml` the user has above `dir`, which is the
/// configuration their own `cargo fmt` will apply.
///
/// Formatting is cosmetic and the crate is already complete when this runs, so
/// a missing or failing `rustfmt` is a warning, not an error: unlike
/// `tree-sitter generate`, nothing the crate needs depends on it.
pub(super) fn format_sources(dir: &Path, written: &[PathBuf]) -> Option<String> {
    format_sources_with("rustfmt", dir, written)
}

/// [`format_sources`] with the formatter's program name injectable, so tests
/// can exercise the "not installed" path without uninstalling anything.
fn format_sources_with(program: &str, dir: &Path, written: &[PathBuf]) -> Option<String> {
    let mut sources = Vec::new();
    for path in written {
        if path.extension().is_some_and(|ext| ext == "rs") {
            sources.push(dir.join(path));
        }
    }
    if sources.is_empty() {
        return None;
    }

    let output = match Command::new(program)
        .args(["--edition", EDITION])
        .args(&sources)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Some(format!(
                "warning: `{program}` not found on PATH; the generated Rust files were left \
                 unformatted (install it with `rustup component add rustfmt`)"
            ));
        }
        Err(e) => {
            return Some(format!(
                "warning: could not run `{program}`: {e}; the generated Rust files were left \
                 unformatted"
            ));
        }
    };
    if output.status.success() {
        return None;
    }
    Some(format!(
        "warning: `{program}` failed, so the generated Rust files may be unformatted:\n{}",
        String::from_utf8_lossy(&output.stderr).trim_end()
    ))
}

/// The `Cargo.toml` dependency line a `--partial-trees` crate needs, and what
/// [`cargo_toml`] writes for it: `ts-bnf-runtime`, at the version of the tool
/// doing the generating. The runtime crate and the tool are released together
/// and share a version, so the crate that matches this tool's generated code is
/// always the one with this tool's version.
pub(super) fn runtime_dependency_line() -> String {
    format!("ts-bnf-runtime = \"{}\"", env!("CARGO_PKG_VERSION"))
}

/// Renders the generated crate's `Cargo.toml`.
///
/// Pinned to `edition = "2021"`, same as `tree-sitter-bnf`'s own
/// hand-written `Cargo.toml`: [`lib_rs`] emits a bare
/// `extern "C" { ... }` block (again mirroring `tree-sitter-bnf`'s own
/// `bindings/rust/lib.rs`), which is a hard error under edition 2024
/// (requires `unsafe extern "C"` there instead). Bumping this file's
/// edition without also updating that block breaks generation.
///
/// With `--partial-trees` (`ast` is `Some(BuildMode::Partial)`) it also lists
/// `ts-bnf-runtime` ([`runtime_dependency_line`]): the generated `ast.rs`
/// re-exports that crate's types instead of declaring its own. Every other
/// configuration, strict `--ast-types` included, gets exactly the file it
/// always did. The file is written once and the user's from then on, so a crate
/// that already exists is never given the line (see
/// `super::missing_runtime_dependency_warning`).
fn cargo_toml(name: &str, ast: Option<BuildMode>) -> String {
    let runtime_dependency = if ast == Some(BuildMode::Partial) {
        format!(
            "\n# The types a typed AST shares with every other generated crate.\n{}",
            runtime_dependency_line()
        )
    } else {
        String::new()
    };
    formatdoc! {r#"
        [package]
        name = "{name}"
        description = "Rust library for parsing and traversing {name} source, generated by ts-bnf-tool"
        version = "0.1.0"
        # Pinned: bindings/rust/lib.rs's `extern "C" {{ ... }}` block requires
        # `unsafe extern "C"` under edition 2024; bump both together, or not at all.
        edition = "{EDITION}"
        license = "MIT"

        build = "bindings/rust/build.rs"
        include = [
          "bindings/rust/*",
          "examples/*",
          "grammar.js",
          "queries/*",
          "src/*",
          "tree-sitter.json",
        ]

        [lib]
        path = "bindings/rust/lib.rs"

        [dependencies]
        tree-sitter-language = "0.1"
        tree-sitter = "0.26.8"{runtime_dependency}

        [build-dependencies]
        cc = "1.2"

        [workspace]
    "#}
}

/// Renders the generated crate's `Makefile`: a single `generate` target that
/// reruns `ts-bnf-tool scaffold .` whenever any bundled `.bnf` file or
/// `ts-bnf-tool.toml` changes. No flags are baked into the recipe — every
/// setting `scaffold` needs (`name`, `--ast-types`, `--merge-config`) is
/// already recorded in `ts-bnf-tool.toml`, which directory-mode `scaffold`
/// reads back on its own (see `resolve_scaffold_target` in `main.rs`).
///
/// Tracks staleness against `bindings/rust/visitor.rs` alone — the one file
/// a `scaffold` run always produces — rather than one target per derived
/// file (`grammar.js`, `src/parser.c`, `bindings/rust/ast.rs`): pre-GNU-4.3
/// `make` runs a shared recipe once per stale target when several targets
/// share it, not once overall, and every one of these outputs is always
/// regenerated together by the same single command anyway, so one canary
/// file is exactly as correct as tracking all of them, with no
/// GNU-Make-version-specific features or stamp file needed.
///
/// `bundled_bnf_paths` (root first, then its `%include` closure, from
/// [`super::bundle_grammar_sources`]) becomes the prerequisite list, and its
/// first entry — the root grammar's own bundled path — names itself in the
/// `generate` target's comment.
fn makefile(bundled_bnf_paths: &[PathBuf]) -> String {
    let root = bundled_bnf_paths
        .first()
        .expect("bundled_bnf_paths always includes at least the root grammar")
        .display();
    let prereqs = bundled_bnf_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    formatdoc! {"
        BNF_TOOL ?= ts-bnf-tool

        generate: bindings/rust/visitor.rs ## Regenerate grammar.js, src/parser.c, and bindings/rust/{{visitor,ast}}.rs from {root}

        bindings/rust/visitor.rs: {prereqs} ts-bnf-tool.toml
        \t$(BNF_TOOL) scaffold .

        .PHONY: generate
    "}
}

/// Renders the generated crate's `bindings/rust/build.rs` — compiles
/// `src/parser.c` (and `src/scanner.c`, if present) into the crate, same as
/// `tree-sitter-bnf`'s own build script.
///
/// Always regenerated on rerun (`preserve_existing: false` in [`render`]),
/// unlike `Cargo.toml`/`lib.rs`/`examples/walk.rs`/`.gitignore` — so, like
/// `visitor.rs`, it carries the generated-file header warning against
/// hand-edits (#376).
fn build_rs(name: &str, no_header: bool) -> String {
    let header = if no_header {
        String::new()
    } else {
        let version = env!("CARGO_PKG_VERSION");
        format!("// Generated by ts-bnf-tool v{version} \u{2014} do not edit by hand.\n\n")
    };
    formatdoc! {r#"
        {header}fn main() {{
            let src_dir = std::path::Path::new("src");

            let mut c_config = cc::Build::new();
            c_config.std("c11").include(src_dir);

            #[cfg(target_env = "msvc")]
            c_config.flag("-utf-8");

            let parser_path = src_dir.join("parser.c");
            c_config.file(&parser_path);
            println!("cargo:rerun-if-changed={{}}", parser_path.to_str().unwrap());

            let scanner_path = src_dir.join("scanner.c");
            if scanner_path.exists() {{
                c_config.file(&scanner_path);
                println!("cargo:rerun-if-changed={{}}", scanner_path.to_str().unwrap());
            }}

            c_config.compile("{name}");
        }}
    "#}
}

/// Renders the generated crate's `bindings/rust/lib.rs`: the `LANGUAGE`/
/// `NODE_TYPES` parser bindings (same shape as `tree-sitter generate`'s own
/// per-language output, e.g. `tree-sitter-bnf/bindings/rust/lib.rs`), a
/// `pub mod visitor;` for the generated `Visitor` trait, and a `parse`
/// convenience function wrapping parser setup.
///
/// Only ever scaffolded once (`preserve_existing: true` in [`render`]):
/// unlike `visitor.rs`, this file's `LANGUAGE`/`NODE_TYPES`/`parse` content
/// never needs to change on a grammar edit, and it's exactly where the
/// tutorial invites users to add `pub mod` declarations for their own
/// hand-written `Visitor` implementations.
///
/// The module declarations are written in the order `rustfmt` keeps them in —
/// `pub mod ast;`, its `parse_ast` re-export, then `pub mod visitor;` — because
/// the file is formatted after it is written (#496), and a template `rustfmt`
/// reorders would make a fresh `lib.rs` differ from the one
/// `super::ensure_lib_rs_declares_ast_module` patches an older crate's into.
fn lib_rs(name: &str, no_header: bool, has_ast: bool) -> String {
    let header = if no_header {
        String::new()
    } else {
        let version = env!("CARGO_PKG_VERSION");
        format!(
            "// Generated by ts-bnf-tool v{version} from {name}.bnf \u{2014} do not edit by hand.\n\n"
        )
    };
    let ast_mod = if has_ast {
        "pub mod ast;\npub use ast::parse_ast;\n"
    } else {
        ""
    };
    formatdoc! {r#"
        {header}//! This crate provides `{name}` language support for the [tree-sitter][]
        //! parsing library, plus a generated [`Visitor`][visitor::Visitor] trait for
        //! traversing the parsed tree. See [`parse`] for a minimal end-to-end example,
        //! or `examples/walk.rs` for a runnable one.
        //!
        //! [tree-sitter]: https://tree-sitter.github.io/

        use tree_sitter_language::LanguageFn;

        {ast_mod}pub mod visitor;
        // Add `pub mod` declarations here for your own Visitor implementations.

        extern "C" {{
            fn tree_sitter_{name}() -> *const ();
        }}

        /// The tree-sitter [`LanguageFn`] for this grammar.
        pub const LANGUAGE: LanguageFn = unsafe {{ LanguageFn::from_raw(tree_sitter_{name}) }};

        /// The content of this grammar's `node-types.json` file.
        pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

        /// Parses `source` with this grammar's tree-sitter parser.
        ///
        /// # Errors
        ///
        /// Returns an error if the language fails to load (should never happen for a
        /// language compiled into this crate) or if parsing does not produce a tree.
        pub fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {{
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&LANGUAGE.into())?;
            parser
                .parse(source, None)
                .ok_or_else(|| "tree-sitter failed to parse the given source".into())
        }}
    "#}
}

/// Renders `examples/ast.rs` for `mode`: [`ast_example_strict`] or
/// [`ast_example_partial`]. The two are not interchangeable — each calls the
/// API its mode's generated `ast.rs` actually has.
fn ast_example(name: &str, mode: BuildMode) -> String {
    match mode {
        BuildMode::Strict => ast_example_strict(name),
        BuildMode::Partial => ast_example_partial(name),
    }
}

/// Renders the partial-mode `examples/ast.rs`: parses a file and recovers one
/// typed root node through `parse_ast` — which needs no tree-sitter in the
/// example, and never fails on a syntax error. Pretty-prints whatever was
/// built, then one `path:line:column` line per diagnostic, and exits non-zero
/// if there were any — so it works as a quick "what survives in this broken
/// file" probe, and as the simplest possible use of the generated API.
fn ast_example_partial(name: &str) -> String {
    formatdoc! {r#"
        //! Parses a file with the `{name}` parser and recovers a typed tree of it using
        //! the generated AST types, even if the file has syntax errors. Prints whatever
        //! could be built, then each problem found. Run it with:
        //!
        //! ```sh
        //! cargo run --example ast -- <file>
        //! ```

        fn main() {{
            let path = std::env::args()
                .nth(1)
                .expect("usage: cargo run --example ast -- <file>");
            let source =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {{path}}: {{e}}"));

            let recovered = {name}::parse_ast(&source).expect("the parser must run");
            match &recovered.value {{
                Some(root) => println!("{{path}}:\n{{root:#?}}"),
                None => println!("{{path}}: nothing could be built"),
            }}
            for problem in &recovered.diagnostics {{
                println!("{{path}}:{{}}:{{}}: {{:?}}", problem.span.start.line, problem.span.start.column, problem.kind);
            }}
            if !recovered.diagnostics.is_empty() {{
                std::process::exit(1);
            }}
        }}
    "#}
}

/// Renders the strict-mode `examples/ast.rs`: parses a file and builds one
/// typed root node through `parse_ast`, which needs no tree-sitter in the
/// example, then pretty-prints it with `{:#?}` — every generated struct/enum
/// derives `Debug`, so this recurses through the whole typed tree with no
/// per-kind code, proving the generated AST types actually compile and
/// work, the same bar `walk.rs` is already held to.
fn ast_example_strict(name: &str) -> String {
    formatdoc! {r#"
        //! Parses a file with the `{name}` parser and builds a typed tree of it using
        //! the generated AST types. Run it with:
        //!
        //! ```sh
        //! cargo run --example ast -- <file>
        //! ```

        fn main() {{
            let path = std::env::args()
                .nth(1)
                .expect("usage: cargo run --example ast -- <file>");
            let source =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {{path}}: {{e}}"));

            let root = {name}::parse_ast(&source).expect("the file must parse and build");
            println!("{{path}}:\n{{root:#?}}");
        }}
    "#}
}

/// Renders `examples/walk.rs`: a `Counter` visitor that counts every named
/// node by overriding only [`Visitor::combine`] — the one method the trait
/// doesn't give a default for — proving the crate works without writing any
/// per-kind override.
fn walk_example(name: &str) -> String {
    formatdoc! {r#"
        //! Parses a file with the `{name}` parser and prints how many named
        //! nodes it contains, using nothing but the generated `Visitor` trait's
        //! required `combine` method. Run it with:
        //!
        //! ```sh
        //! cargo run --example walk -- <file>
        //! ```

        use {name}::visitor::{{SourceNode, Visitor}};

        /// Counts every node visited, leaves and non-leaves alike: `combine` runs
        /// exactly once per visited node, regardless of kind.
        struct Counter {{
            total: usize,
        }}

        impl<'t> Visitor<'t> for Counter {{
            type Output = ();
            type Error = std::convert::Infallible;

            fn combine(&mut self, _results: Vec<()>) -> Result<(), Self::Error> {{
                self.total += 1;
                Ok(())
            }}
        }}

        fn main() {{
            let path = std::env::args()
                .nth(1)
                .expect("usage: cargo run --example walk -- <file>");
            let source =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {{path}}: {{e}}"));

            let tree = {name}::parse(&source).expect("parse must succeed");

            let mut counter = Counter {{ total: 0 }};
            counter
                .visit(SourceNode {{ node: tree.root_node(), source: &source }})
                .expect("Infallible visitor must not fail");

            println!("{{path}}: {{}} node(s)", counter.total);
        }}
    "#}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only `--partial-trees` makes the crate depend on `ts-bnf-runtime`:
    /// no AST and strict `--ast-types` get the manifest they always had.
    #[test]
    fn cargo_toml_lists_the_runtime_crate_only_in_partial_mode() {
        assert!(!cargo_toml("decls", None).contains("ts-bnf-runtime"));
        assert!(!cargo_toml("decls", Some(BuildMode::Strict)).contains("ts-bnf-runtime"));
        assert!(cargo_toml("decls", Some(BuildMode::Partial)).contains("ts-bnf-runtime"));
    }

    /// The partial manifest is a valid one with the dependency at the tool's
    /// version under `[dependencies]`, so it is the version of the runtime
    /// crate that was released with this tool that the crate resolves.
    #[test]
    fn partial_cargo_toml_depends_on_the_runtime_at_the_tools_version() {
        let manifest: toml::Table = cargo_toml("decls", Some(BuildMode::Partial))
            .parse()
            .expect("a valid manifest");
        let dependencies = manifest["dependencies"].as_table().expect("[dependencies]");
        assert_eq!(
            dependencies["ts-bnf-runtime"].as_str(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert!(
            !manifest["build-dependencies"]
                .as_table()
                .unwrap()
                .contains_key("ts-bnf-runtime"),
            "a library dependency, not a build one"
        );
        assert!(dependencies.contains_key("tree-sitter"), "the rest is kept");
    }

    /// The runtime dependency is the *only* difference between the partial
    /// manifest and the strict one: nothing else about the crate changes.
    #[test]
    fn partial_cargo_toml_differs_from_strict_only_by_the_dependency() {
        let strict = cargo_toml("decls", Some(BuildMode::Strict));
        let partial = cargo_toml("decls", Some(BuildMode::Partial));
        let added = format!(
            "\n# The types a typed AST shares with every other generated crate.\n{}",
            runtime_dependency_line()
        );
        assert_eq!(partial.replace(&added, ""), strict);
    }

    /// The partial-mode example goes through `parse_ast`, so it needs no
    /// tree-sitter and no `SourceNode`, and reports diagnostics instead of
    /// `.expect`-ing a build.
    #[test]
    fn partial_ast_example_uses_the_entry_point_and_reports_diagnostics() {
        let out = ast_example("decls", BuildMode::Partial);
        assert!(out.contains("decls::parse_ast(&source)"));
        assert!(out.contains("recovered.diagnostics"));
        assert!(!out.contains("SourceNode"));
        assert!(!out.contains("tree_sitter"));
        assert!(!out.contains("try_into"));
    }

    /// The strict-mode example goes through `parse_ast` too, and nothing from
    /// the partial API.
    #[test]
    fn strict_ast_example_uses_the_entry_point() {
        let out = ast_example("decls", BuildMode::Strict);
        assert!(out.contains("decls::parse_ast(&source)"));
        assert!(!out.contains("SourceNode"));
        assert!(!out.contains("tree_sitter"));
        assert!(!out.contains("recover"));
    }

    /// A fresh `lib.rs` re-exports the entry point next to `pub mod ast;`,
    /// and only when there is an AST at all.
    #[test]
    fn lib_rs_reexports_parse_ast_only_with_an_ast() {
        let with = lib_rs("decls", true, true);
        assert!(with.contains("pub mod ast;\npub use ast::parse_ast;\n"));
        let without = lib_rs("decls", true, false);
        assert!(!without.contains("parse_ast"));
    }

    /// The edition `rustfmt` parses under is the one the manifest declares:
    /// both come from [`EDITION`], so bumping one cannot leave the other behind.
    #[test]
    fn cargo_toml_declares_the_edition_the_formatter_uses() {
        let manifest: toml::Table = cargo_toml("decls", None).parse().expect("a valid manifest");
        assert_eq!(manifest["package"]["edition"].as_str(), Some(EDITION));
    }

    /// A program name that is on no PATH, to exercise the "not installed" path.
    const NO_SUCH_FORMATTER: &str = "ts-bnf-test-no-such-formatter";

    /// `true` if a real `rustfmt` can be run, so the tests that need one can
    /// skip on a machine without it, like the ones that need `tree-sitter`.
    fn rustfmt_available() -> bool {
        Command::new("rustfmt").arg("--version").output().is_ok()
    }

    /// A fresh, empty directory under the system temp dir for one test.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Only `.rs` files are handed to the formatter: a run that wrote nothing
    /// else but `Cargo.toml`, the bundled grammar and the like never tries to
    /// run it at all (which a missing program would otherwise report).
    #[test]
    fn format_sources_ignores_files_that_are_not_rust() {
        let written = [
            PathBuf::from("Cargo.toml"),
            PathBuf::from("Makefile"),
            PathBuf::from("decls.bnf"),
            PathBuf::from("ts-bnf-tool.toml"),
        ];
        assert_eq!(
            format_sources_with(NO_SUCH_FORMATTER, Path::new("crate"), &written),
            None
        );
        assert_eq!(
            format_sources_with(NO_SUCH_FORMATTER, Path::new("crate"), &[]),
            None
        );
    }

    /// A missing `rustfmt` is a warning that says how to get it, not an error:
    /// the crate is complete without formatting.
    #[test]
    fn format_sources_warns_when_the_formatter_is_missing() {
        let warning = format_sources_with(
            NO_SUCH_FORMATTER,
            Path::new("crate"),
            &[PathBuf::from("bindings/rust/visitor.rs")],
        )
        .expect("a missing formatter must produce a warning");
        assert!(warning.starts_with("warning: "), "{warning}");
        assert!(warning.contains(NO_SUCH_FORMATTER), "{warning}");
        assert!(warning.contains("not found on PATH"), "{warning}");
        assert!(
            warning.contains("rustup component add rustfmt"),
            "{warning}"
        );
    }

    /// A formatter that exists but cannot be run (a permission error, not a
    /// missing binary) is not reported as "not found on PATH".
    #[test]
    #[cfg(unix)]
    fn format_sources_reports_an_unrunnable_formatter_as_such() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch_dir("ts_bnf_format_sources_unrunnable");
        let program = dir.join("rustfmt");
        std::fs::write(&program, "").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644)).unwrap();

        let warning = format_sources_with(
            program.to_str().unwrap(),
            &dir,
            &[PathBuf::from("visitor.rs")],
        )
        .expect("an unrunnable formatter must produce a warning");
        assert!(warning.contains("could not run"), "{warning}");
        assert!(!warning.contains("not found on PATH"), "{warning}");
    }

    /// Exactly the files listed are formatted — each relative to `dir` — and a
    /// `.rs` file beside them that was not listed (a preserved, user-owned one)
    /// is left byte-for-byte alone.
    #[test]
    fn format_sources_formats_only_the_listed_files() {
        if !rustfmt_available() {
            return;
        }
        let dir = scratch_dir("ts_bnf_format_sources_listed");
        std::fs::write(dir.join("listed.rs"), "fn   main( ){ }\n").unwrap();
        std::fs::write(dir.join("unlisted.rs"), "fn   main( ){ }\n").unwrap();

        let warning = format_sources(&dir, &[PathBuf::from("listed.rs")]);

        assert_eq!(warning, None);
        assert_eq!(
            std::fs::read_to_string(dir.join("listed.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("unlisted.rs")).unwrap(),
            "fn   main( ){ }\n"
        );
    }

    /// The formatter parses under the crate's edition, not `rustfmt`'s own
    /// 2015 default: `async fn` is a hard parse error there.
    #[test]
    fn format_sources_parses_under_the_crates_edition() {
        if !rustfmt_available() {
            return;
        }
        let dir = scratch_dir("ts_bnf_format_sources_edition");
        std::fs::write(dir.join("a.rs"), "async fn   f( ) { }\n").unwrap();

        assert_eq!(format_sources(&dir, &[PathBuf::from("a.rs")]), None);
        assert_eq!(
            std::fs::read_to_string(dir.join("a.rs")).unwrap(),
            "async fn f() {}\n"
        );
    }

    /// A source `rustfmt` cannot parse is a warning carrying its own message,
    /// not an error and not silence.
    #[test]
    fn format_sources_warns_with_rustfmts_message_when_it_fails() {
        if !rustfmt_available() {
            return;
        }
        let dir = scratch_dir("ts_bnf_format_sources_failure");
        std::fs::write(dir.join("broken.rs"), "fn (\n").unwrap();

        let warning = format_sources(&dir, &[PathBuf::from("broken.rs")])
            .expect("an unparseable source must produce a warning");
        assert!(
            warning.starts_with("warning: `rustfmt` failed"),
            "{warning}"
        );
        assert!(warning.contains("unclosed delimiter"), "{warning}");
    }
}
