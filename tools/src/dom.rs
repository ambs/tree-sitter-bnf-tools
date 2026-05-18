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
                write!(f, "seq({})", s.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
            }
            GrammarNode::Choice(c) => {
                write!(f, "choice({})", c.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
            }
            GrammarNode::TerminalLiteral(l) => { write!(f, "{}", l)}
            GrammarNode::TerminalPattern(p) => { write!(f, "{}", p)}
            GrammarNode::NonTerminal(nt) => { write!(f, "$.{}", nt)}
            GrammarNode::ZeroOrMore(zm) => {write!(f, "repeat({})", zm)}
            GrammarNode::OneOrMore(om) => {write!(f, "repeat1({})", om)}
        }
    }
}

pub struct Production {
    pub name: String,
    pub body: GrammarNode
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
    pub productions: Vec<Production>
}

impl Display for Grammar {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for production in &self.productions {
            write!(fmt, "\n{}", production)?;
        }
        Ok(())
    }
}
