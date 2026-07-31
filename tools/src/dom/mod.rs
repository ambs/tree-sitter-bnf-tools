/// Semantic analyses over a parsed grammar (FIRST sets, left-recursion, …).
pub mod analysis;
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
/// Rule-dependency graph builder and DOT/Mermaid/Graphviz emitters.
pub mod graph;
/// Skeleton `highlights.scm` generator with naming-convention heuristics.
pub mod highlights;
/// Scaffolding for the `library` subcommand's generated crate
/// (`Cargo.toml`/`build.rs`/`lib.rs`/`examples/walk.rs`); target-language
/// emitters live in submodules.
pub mod library;
/// Core grammar node types and their Display representations.
mod nodes;
/// A single named grammar rule.
mod production;
/// Walker that converts a [`GrammarNode`] tree into railroad-diagram combinators.
pub mod railroad;
/// Safe mechanical rename of a rule throughout a [`Grammar`].
pub mod rename;
/// Renders a [`Grammar`] as a complete `grammar.js` file.
mod scaffold;
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

pub use diagnostic::{Diagnostic, Severity};
pub use directive::{ConflictGroup, DirectiveItem, NameOrLiteral, PrecedenceGroup, ReservedEntry};
pub use error::ParseError;
pub use format::format_grammar;
pub use highlights::Highlights;
pub use library::{LibraryCrate, LibraryFile, render_library, run_library};
pub use nodes::{GrammarNode, PrecKind, PrecLevel};
pub use production::Production;
pub use rename::rename_grammar;
pub use scaffold::{Scaffold, run_generate};
pub use summary::{FirstSetStats, GrammarSummary};
pub use types::Grammar;
pub use visitor::check_visitor;
pub use visitor::render_visitor;
pub use visitor::{FieldTargetKinds, resolve_field_target_kinds, visible_kinds};
