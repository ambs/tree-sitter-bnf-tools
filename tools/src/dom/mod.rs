/// Semantic analyses over a parsed grammar (FIRST sets, left-recursion, …).
pub mod analysis;
/// Derives typed AST node/field shapes from a [`Grammar`] (per-kind fields,
/// multiplicity, leaf status), reusing the `Visitor` derivation layer.
pub mod ast;
/// Structured diagnostic messages with severity levels.
pub mod diagnostic;
/// Types for grammar directive entries with source location.
pub mod directive;
/// Parse and conversion error types.
mod error;
/// BNF pretty-printer that re-emits a [`Grammar`] in canonical style.
pub mod format;
/// Cross-reference and structural validation checks on a [`Grammar`].
mod grammar;
/// Renders a [`Grammar`] as a complete `grammar.js` file.
mod grammar_js;
/// Rule-dependency graph builder and DOT/Mermaid/Graphviz emitters.
pub mod graph;
/// Skeleton `highlights.scm` generator with naming-convention heuristics.
pub mod highlights;
/// Core grammar node types and their Display representations.
mod nodes;
/// A single named grammar rule.
mod production;
/// Walker that converts a [`GrammarNode`] tree into railroad-diagram combinators.
pub mod railroad;
/// Safe mechanical rename of a rule throughout a [`Grammar`].
pub mod rename;
/// Scaffolding for the `scaffold` subcommand's generated crate
/// (`Cargo.toml`/`build.rs`/`lib.rs`/`examples/walk.rs`); target-language
/// emitters live in submodules.
pub mod scaffold;
/// Grammar shape metrics produced by `check --summary`.
pub mod summary;
/// Shared helpers for constructing test fixtures.
#[doc(hidden)]
pub mod test_utils;
/// The [`Grammar`] struct and its basic impls.
mod types;
/// Derives a `Visitor` trait's shape from a [`Grammar`] (visible node-kind set,
/// per-kind fields and leaf status, method-name collisions); target-language
/// emitters live in submodules.
pub mod visitor;

pub use ast::merge::{MergeConfig, parse_merge_config};
pub use diagnostic::{Diagnostic, Severity};
pub use directive::{ConflictGroup, DirectiveItem, NameOrLiteral, PrecedenceGroup, ReservedEntry};
pub use error::ParseError;
pub use format::format_grammar;
pub use grammar_js::{GrammarJs, run_generate};
pub use highlights::Highlights;
pub use nodes::{GrammarNode, PrecKind, PrecLevel};
pub use production::Production;
pub use rename::rename_grammar;
pub use scaffold::{ScaffoldCrate, ScaffoldFile, render_scaffold, run_scaffold};
pub use summary::{FirstSetStats, GrammarSummary};
pub use types::Grammar;
pub use visitor::check_visitor;
pub use visitor::render_visitor;
pub use visitor::{FieldTargetKinds, resolve_field_target_kinds, visible_kinds};
