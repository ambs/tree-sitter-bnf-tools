use std::fmt;
use std::fmt::{Display, Formatter};

use super::types::Grammar;

/// A wrapper that renders a [`Grammar`] as a complete `grammar.js` file.
pub struct Scaffold<'a> {
    /// The grammar to render.
    pub grammar: &'a Grammar,
    /// The grammar name passed to tree-sitter's `grammar({ name: … })`.
    pub name: &'a str,
}

impl Display for Scaffold<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "module.exports = grammar({{")?;
        writeln!(f, "  name: \"{}\",", self.name)?;
        writeln!(f)?;
        if !self.grammar.extras.is_empty() {
            let items = self
                .grammar
                .extras
                .iter()
                .map(|item| {
                    if item.name.starts_with('/') {
                        item.name.clone()
                    } else {
                        format!("$.{}", item.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  extras: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.inline.is_empty() {
            let items = self
                .grammar
                .inline
                .iter()
                .map(|item| format!("$.{}", item.name))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  inline: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.supertypes.is_empty() {
            let items = self
                .grammar
                .supertypes
                .iter()
                .map(|item| format!("$.{}", item.name))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "  supertypes: $ => [{items}],")?;
            writeln!(f)?;
        }
        if !self.grammar.conflicts.is_empty() {
            writeln!(f, "  conflicts: $ => [")?;
            for group in &self.grammar.conflicts {
                let items = group
                    .rules
                    .iter()
                    .map(|n| format!("$.{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(f, "    [{items}],")?;
            }
            writeln!(f, "  ],")?;
            writeln!(f)?;
        }
        writeln!(f, "  rules: {{")?;
        for production in self.grammar.productions.values() {
            writeln!(f, "    {}: $ => {},", production.name, production.body)?;
        }
        writeln!(f, "  }}")?;
        write!(f, "}});")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::test_utils::{cg, di, p};
    use crate::dom::GrammarNode::TerminalLiteral;
    use crate::dom::{Grammar, GrammarNode};

    #[test]
    fn scaffold_single_rule() {
        let g = Grammar::from_rules([p("expr", TerminalLiteral("'x'".into()))]);
        assert_eq!(
            Scaffold { grammar: &g, name: "expr" }.to_string(),
            "module.exports = grammar({\n  name: \"expr\",\n\n  rules: {\n    expr: $ => 'x',\n  }\n});"
        );
    }

    #[test]
    fn scaffold_multi_rule() {
        let g = Grammar::from_rules([
            p("a", TerminalLiteral("'x'".into())),
            p("b", GrammarNode::NonTerminal("a".into())),
        ]);
        let out = Scaffold {
            grammar: &g,
            name: "test",
        }
        .to_string();
        assert!(out.contains("    a: $ => 'x',"));
        assert!(out.contains("    b: $ => $.a,"));
        assert!(out.starts_with("module.exports = grammar({"));
        assert!(out.ends_with("});"));
    }

    #[test]
    fn scaffold_name_appears_in_output() {
        let g = Grammar::from_rules([p("r", TerminalLiteral("'y'".into()))]);
        let out = Scaffold {
            grammar: &g,
            name: "mygrammar",
        }
        .to_string();
        assert!(out.contains("name: \"mygrammar\""));
    }

    #[test]
    fn scaffold_no_conflicts_omits_key() {
        let g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("conflicts"));
    }

    #[test]
    fn scaffold_with_single_conflict_group() {
        let mut g = Grammar::from_rules([p("expr", TerminalLiteral("'x'".into()))]);
        g.conflicts = vec![cg(&["expr", "term"], 0)];
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  conflicts: $ => ["));
        assert!(out.contains("    [$.expr, $.term],"));
        assert!(out.contains("  ],"));
        assert!(out.find("conflicts").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_with_multiple_conflict_groups() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.conflicts = vec![cg(&["a", "b"], 0), cg(&["c", "d", "e"], 0)];
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("    [$.a, $.b],"));
        assert!(out.contains("    [$.c, $.d, $.e],"));
    }

    #[test]
    fn scaffold_with_supertypes() {
        let mut g = Grammar::from_rules([p("expression", TerminalLiteral("'x'".into()))]);
        g.supertypes = vec![di("expression", 0), di("statement", 0)];
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  supertypes: $ => [$.expression, $.statement],"));
        assert!(out.find("supertypes").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_no_supertypes_omits_key() {
        let g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("supertypes"));
    }

    #[test]
    fn scaffold_with_extras_pattern_and_rule() {
        let mut g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        g.extras = vec![di("/\\s/", 0), di("comment", 0)];
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(out.contains("  extras: $ => [/\\s/, $.comment],"));
        assert!(out.find("extras").unwrap() < out.find("rules").unwrap());
    }

    #[test]
    fn scaffold_no_extras_omits_key() {
        let g = Grammar::from_rules([p("a", TerminalLiteral("'x'".into()))]);
        let out = Scaffold {
            grammar: &g,
            name: "g",
        }
        .to_string();
        assert!(!out.contains("extras"));
    }
}
