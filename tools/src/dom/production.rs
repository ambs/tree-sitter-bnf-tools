use std::fmt;
use std::fmt::{Display, Formatter};

use super::nodes::GrammarNode;

/// A single named grammar rule (`name -> body`).
pub struct Production {
    /// The rule name (left-hand side of `->`)
    pub name: String,
    /// The rule body (right-hand side of `->`).
    pub body: GrammarNode,
    /// 1-based source line number of this rule's definition.
    pub line: usize,
    /// Source filename where this rule was defined (empty string if unknown).
    pub filename: String,
}

impl Display for Production {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        fmt.write_str(&self.name)?;
        fmt.write_str(" -> ")?;
        write!(fmt, "{}", &self.body)?;
        Ok(())
    }
}
