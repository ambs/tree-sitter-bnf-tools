use std::fmt;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum ParseError {
    UnexpectedNodeType { expected: String, got: String },
    UnknownNodeKind(String),
    MalformedProduction,
    SyntaxError,
    ParseFailed,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedNodeType { expected, got } => {
                write!(f, "expected node type '{}', got '{}'", expected, got)
            }
            ParseError::UnknownNodeKind(kind) => write!(f, "unknown node kind '{}'", kind),
            ParseError::MalformedProduction => {
                write!(f, "non-terminal expected on left-hand side of production")
            }
            ParseError::SyntaxError => write!(f, "input contains syntax errors"),
            ParseError::ParseFailed => write!(f, "parser returned no tree"),
        }
    }
}

impl std::error::Error for ParseError {}

pub enum GrammarNode {
    Sequence(Vec<GrammarNode>),
    Choice(Vec<GrammarNode>),
    Optional(Box<GrammarNode>),
    TerminalLiteral(String),
    TerminalPattern(String),
    NonTerminal(String),
    ZeroOrMore(Box<GrammarNode>),
    OneOrMore(Box<GrammarNode>),
    Token(Box<GrammarNode>),
    Field(String, Box<GrammarNode>),
    Alias(Box<GrammarNode>, Box<GrammarNode>),
}

impl Display for GrammarNode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GrammarNode::Sequence(s) => {
                write!(
                    f,
                    "seq({})",
                    s.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            GrammarNode::Choice(c) => {
                write!(
                    f,
                    "choice({})",
                    c.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            GrammarNode::TerminalLiteral(l) => {
                write!(f, "{}", l)
            }
            GrammarNode::TerminalPattern(p) => {
                write!(f, "{}", p)
            }
            GrammarNode::NonTerminal(nt) => {
                write!(f, "$.{}", nt)
            }
            GrammarNode::ZeroOrMore(zm) => {
                write!(f, "repeat({})", zm)
            }
            GrammarNode::OneOrMore(om) => {
                write!(f, "repeat1({})", om)
            }
            GrammarNode::Optional(om) => {
                write!(f, "optional({})", om)
            }
            GrammarNode::Token(inner) => {
                write!(f, "token({})", inner)
            }
            GrammarNode::Field(name, inner) => {
                write!(f, "field('{}', {})", name, inner)
            }
            GrammarNode::Alias(body, name) => {
                write!(f, "alias({}, {})", body, name)
            }
        }
    }
}

pub struct Production {
    pub name: String,
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

pub struct Grammar {
    pub productions: Vec<Production>,
}

impl Display for Grammar {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for production in &self.productions {
            write!(fmt, "\n{}", production)?;
        }
        Ok(())
    }
}

pub struct Scaffold<'a> {
    pub grammar: &'a Grammar,
    pub name: &'a str,
}

impl Display for Scaffold<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "module.exports = grammar({{")?;
        writeln!(f, "  name: \"{}\",", self.name)?;
        writeln!(f)?;
        writeln!(f, "  rules: {{")?;
        for production in &self.grammar.productions {
            writeln!(f, "    {}: $ => {},", production.name, production.body)?;
        }
        writeln!(f, "  }}")?;
        write!(f, "}});")
    }
}

#[cfg(test)]
mod tests {
    use super::GrammarNode::*;
    use super::*;

    #[test]
    fn nonterminal_display() {
        assert_eq!(NonTerminal("foo".into()).to_string(), "$.foo");
    }

    #[test]
    fn terminal_literal_display() {
        assert_eq!(TerminalLiteral("'x'".into()).to_string(), "'x'");
    }

    #[test]
    fn terminal_pattern_display() {
        assert_eq!(TerminalPattern("/a+/".into()).to_string(), "/a+/");
    }

    #[test]
    fn zero_or_more_display() {
        assert_eq!(
            ZeroOrMore(Box::new(NonTerminal("a".into()))).to_string(),
            "repeat($.a)"
        );
    }

    #[test]
    fn one_or_more_display() {
        assert_eq!(
            OneOrMore(Box::new(NonTerminal("a".into()))).to_string(),
            "repeat1($.a)"
        );
    }

    #[test]
    fn optional_display() {
        assert_eq!(
            Optional(Box::new(NonTerminal("a".into()))).to_string(),
            "optional($.a)"
        );
    }

    #[test]
    fn token_display() {
        assert_eq!(
            Token(Box::new(TerminalPattern("/[0-9]+/".into()))).to_string(),
            "token(/[0-9]+/)"
        );
    }

    #[test]
    fn field_display() {
        assert_eq!(
            Field("lhs".into(), Box::new(NonTerminal("expr".into()))).to_string(),
            "field('lhs', $.expr)"
        );
    }

    #[test]
    fn alias_display() {
        assert_eq!(
            Alias(
                Box::new(NonTerminal("foo".into())),
                Box::new(NonTerminal("bar".into()))
            )
            .to_string(),
            "alias($.foo, $.bar)"
        );
    }

    #[test]
    fn token_sequence_display() {
        assert_eq!(
            Token(Box::new(Sequence(vec![
                TerminalPattern("/[A-Za-z_]/".into()),
                TerminalPattern("/[A-Za-z0-9_]*/".into()),
            ])))
            .to_string(),
            "token(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/))"
        );
    }

    #[test]
    fn sequence_display() {
        assert_eq!(
            Sequence(vec![
                NonTerminal("a".into()),
                NonTerminal("b".into()),
                NonTerminal("c".into()),
            ])
            .to_string(),
            "seq($.a, $.b, $.c)"
        );
    }

    #[test]
    fn choice_display() {
        assert_eq!(
            Choice(vec![NonTerminal("a".into()), NonTerminal("b".into())]).to_string(),
            "choice($.a, $.b)"
        );
    }

    #[test]
    fn production_display() {
        let p = Production {
            name: "expr".into(),
            body: NonTerminal("a".into()),
        };
        assert_eq!(p.to_string(), "expr -> $.a");
    }

    #[test]
    fn grammar_display() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "b".into(),
                    body: NonTerminal("a".into()),
                },
            ],
        };
        assert_eq!(g.to_string(), "\na -> 'x'\nb -> $.a");
    }

    #[test]
    fn scaffold_single_rule() {
        let g = Grammar {
            productions: vec![Production {
                name: "expr".into(),
                body: TerminalLiteral("'x'".into()),
            }],
        };
        assert_eq!(
            Scaffold { grammar: &g, name: "expr" }.to_string(),
            "module.exports = grammar({\n  name: \"expr\",\n\n  rules: {\n    expr: $ => 'x',\n  }\n});"
        );
    }

    #[test]
    fn scaffold_multi_rule() {
        let g = Grammar {
            productions: vec![
                Production {
                    name: "a".into(),
                    body: TerminalLiteral("'x'".into()),
                },
                Production {
                    name: "b".into(),
                    body: NonTerminal("a".into()),
                },
            ],
        };
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
        let g = Grammar {
            productions: vec![Production {
                name: "r".into(),
                body: TerminalLiteral("'y'".into()),
            }],
        };
        let out = Scaffold {
            grammar: &g,
            name: "mygrammar",
        }
        .to_string();
        assert!(out.contains("name: \"mygrammar\""));
    }
}
