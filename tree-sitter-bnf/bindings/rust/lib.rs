//! This crate provides Bnf language support for the [tree-sitter][] parsing library.
//!
//! Typically, you will use the [LANGUAGE][] constant to add this language to a
//! tree-sitter [Parser][], and then use the parser to parse some code:
//!
//! ```
//! let code = r#"
//! expr -> 'a';
//! "#;
//! let mut parser = tree_sitter::Parser::new();
//! let language = tree_sitter_bnf::LANGUAGE;
//! parser
//!     .set_language(&language.into())
//!     .expect("Error loading Bnf parser");
//! let tree = parser.parse(code, None).unwrap();
//! assert!(!tree.root_node().has_error());
//! ```
//!
//! [Parser]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Parser.html
//! [tree-sitter]: https://tree-sitter.github.io/

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_bnf() -> *const ();
}

/// The tree-sitter [`LanguageFn`][LanguageFn] for this grammar.
///
/// [LanguageFn]: https://docs.rs/tree-sitter-language/*/tree_sitter_language/struct.LanguageFn.html
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_bnf) };

/// The content of the [`node-types.json`][] file for this grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers/6-static-node-types
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The content of the [`highlights.scm`][] query for this grammar.
///
/// [`highlights.scm`]: https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// The content of the [`injections.scm`][] query for this grammar.
///
/// [`injections.scm`]: https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting#language-injection
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");

/// The content of the [`locals.scm`][] query for this grammar.
///
/// [`locals.scm`]: https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting#local-variables
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// The content of the [`tags.scm`][] query for this grammar.
///
/// [`tags.scm`]: https://tree-sitter.github.io/tree-sitter/4-code-navigation
pub const TAGS_QUERY: &str = include_str!("../../queries/tags.scm");

/// The content of the `folds.scm` query for this grammar (code-folding ranges).
pub const FOLDS_QUERY: &str = include_str!("../../queries/folds.scm");

/// The content of the `indents.scm` query for this grammar (auto-indentation rules).
pub const INDENTS_QUERY: &str = include_str!("../../queries/indents.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading Bnf parser");
    }

    /// Every exported query constant is non-empty and is itself a valid
    /// tree-sitter query against this grammar's `LANGUAGE` — a stronger
    /// check than "the file exists", since these queries were never
    /// exercised anywhere before being exposed here (#405).
    #[test]
    fn query_constants_are_valid_against_the_grammar() {
        let language: tree_sitter::Language = super::LANGUAGE.into();
        for (name, query) in [
            ("HIGHLIGHTS_QUERY", super::HIGHLIGHTS_QUERY),
            ("INJECTIONS_QUERY", super::INJECTIONS_QUERY),
            ("LOCALS_QUERY", super::LOCALS_QUERY),
            ("TAGS_QUERY", super::TAGS_QUERY),
            ("FOLDS_QUERY", super::FOLDS_QUERY),
            ("INDENTS_QUERY", super::INDENTS_QUERY),
        ] {
            assert!(!query.is_empty(), "{name} must not be empty");
            tree_sitter::Query::new(&language, query)
                .unwrap_or_else(|e| panic!("{name} is not a valid query: {e}"));
        }
    }
}
