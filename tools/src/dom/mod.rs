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

pub use diagnostic::{Diagnostic, Severity};
pub use directive::{ConflictGroup, DirectiveItem, PrecedenceGroup, PrecedenceItem};
pub use error::ParseError;
pub use format::format_grammar;
pub use highlights::Highlights;
pub use nodes::{GrammarNode, PrecKind};
pub use production::Production;
pub use rename::rename_grammar;
pub use scaffold::Scaffold;
pub use summary::{FirstSetStats, GrammarSummary};
pub use types::Grammar;
