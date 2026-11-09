//! `ts-bnf-runtime` is the part of the generated typed ASTs that has nothing to
//! do with tree-sitter, so a consumer can depend on it alone. This keeps it so:
//! it fails if a dependency on tree-sitter, direct or through a crate named
//! after it, is ever added to the manifest.

use std::fs;
use std::path::Path;

/// The names listed under `[dependencies]`, `[build-dependencies]` and
/// `[dev-dependencies]` in the crate's manifest.
fn declared_dependencies() -> Vec<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = fs::read_to_string(&manifest).expect("the manifest must be readable");

    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_dependencies = line
                .trim_matches(|c| c == '[' || c == ']')
                .ends_with("dependencies");
            continue;
        }
        if in_dependencies && !line.is_empty() && !line.starts_with('#') {
            let name = line.split(['=', ' ']).next().unwrap_or_default();
            names.push(name.to_string());
        }
    }
    names
}

/// No dependency of the crate is tree-sitter, or a crate named after it.
#[test]
fn does_not_depend_on_tree_sitter() {
    for name in declared_dependencies() {
        assert!(
            !name.contains("tree-sitter"),
            "ts-bnf-runtime must not depend on `{name}`"
        );
    }
}

/// The crate has no dependencies at all today; a new one should be a decision
/// somebody makes on purpose, so it is noticed here and not just in a review.
#[test]
fn has_no_dependencies_at_all() {
    assert_eq!(declared_dependencies(), Vec::<String>::new());
}
