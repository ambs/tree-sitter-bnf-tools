//! Library interface for `ts-bnf-tool`, exposing the DOM and visitor layer.
//!
//! The binary (`main.rs`) is a thin wrapper around these modules.

/// DOM types representing the BNF grammar as a Rust value tree.
pub mod dom;
/// Visitor functions that walk a tree-sitter parse tree and build the DOM.
pub mod visitors;
