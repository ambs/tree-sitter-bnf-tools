/// Semantic analyses over a parsed grammar (FIRST sets, left-recursion, …).
pub mod analysis;
/// Structured diagnostic messages with severity levels.
pub mod diagnostic;
/// Parse and conversion error types.
mod error;
/// Cross-reference and structural validation checks on a [`Grammar`].
mod grammar;
/// Core grammar node types and their Display representations.
mod nodes;
/// A single named grammar rule.
mod production;
/// Renders a [`Grammar`] as a complete `grammar.js` file.
mod scaffold;
/// The [`Grammar`] struct and its basic impls.
mod types;

pub use diagnostic::{Diagnostic, Severity};
pub use error::ParseError;
pub use nodes::{GrammarNode, PrecKind};
pub use production::Production;
pub use scaffold::Scaffold;
pub use types::Grammar;
