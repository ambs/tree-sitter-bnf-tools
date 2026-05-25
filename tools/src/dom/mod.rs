/// Parse and conversion error types.
mod error;
/// The complete grammar structure and cross-reference validation.
mod grammar;
/// Core grammar node types and their Display representations.
mod nodes;
/// A single named grammar rule.
mod production;
/// Renders a [`Grammar`] as a complete `grammar.js` file.
mod scaffold;

pub use error::ParseError;
pub use grammar::Grammar;
pub use nodes::{GrammarNode, PrecKind};
pub use production::Production;
pub use scaffold::Scaffold;
