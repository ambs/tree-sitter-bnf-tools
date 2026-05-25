use std::fmt;
use std::fmt::{Display, Formatter};

use super::nodes::GrammarNode;

/// A single named grammar rule (`name -> body`).
pub struct Production {
    /// The rule name (left-hand side of `->`)
    pub name: String,
    /// The rule body (right-hand side of `->`).
    pub body: GrammarNode,
}

impl Display for Production {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        fmt.write_str(&self.name)?;
        fmt.write_str(" -> ")?;
        write!(fmt, "{}", &self.body)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::NonTerminal;

    #[test]
    fn production_display() {
        let p = Production {
            name: "expr".into(),
            body: NonTerminal("a".into()),
        };
        assert_eq!(p.to_string(), "expr -> $.a");
    }
}
