use std::fmt;
use std::fmt::{Display, Formatter};

pub enum GrammarNode {
    Sequence(Vec<GrammarNode>),
    Choice(Vec<GrammarNode>),
    TerminalLiteral(String),
    TerminalPattern(String),
    NonTerminal(String),
    ZeroOrMore(Box<GrammarNode>),
    OneOrMore(Box<GrammarNode>),
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
}
