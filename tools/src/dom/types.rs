use std::collections::HashSet;
use std::fmt;
use std::fmt::{Display, Formatter};

use super::production::Production;

/// The complete grammar: all productions and any declared conflict or inline groups.
pub struct Grammar {
    /// Ordered list of grammar rules.
    pub productions: Vec<Production>,
    /// Conflict groups declared with `%conflicts`; each inner `Vec` is one group.
    pub conflicts: Vec<Vec<String>>,
    /// Rule names declared with `%inline` that should be inlined at every call site.
    pub inline: Vec<String>,
    /// Abstract rule names declared with `%supertypes` that group concrete subtypes.
    pub supertypes: Vec<String>,
    /// Extra items declared with `%extras`; each is either a regex pattern (starts with `/`) or a rule name.
    pub extras: Vec<String>,
    /// All non-terminal names that appear on right-hand sides of rules, accumulated by the visitor.
    pub rhs_nonterminals: HashSet<String>,
}

impl Default for Grammar {
    fn default() -> Self {
        Self::new()
    }
}

impl Grammar {
    /// Creates an empty grammar with no productions, conflicts, inline, supertypes, or extras.
    pub fn new() -> Self {
        Self {
            productions: Vec::new(),
            conflicts: Vec::new(),
            inline: Vec::new(),
            supertypes: Vec::new(),
            extras: Vec::new(),
            rhs_nonterminals: HashSet::new(),
        }
    }

    /// Returns the set of all defined rule names, used for cross-reference checks.
    pub fn known_rules(&self) -> HashSet<&str> {
        self.productions.iter().map(|p| p.name.as_str()).collect()
    }
}

impl Display for Grammar {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for production in &self.productions {
            write!(fmt, "\n{}", production)?;
        }
        Ok(())
    }
}
