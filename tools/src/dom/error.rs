use std::fmt;
use std::fmt::{Display, Formatter};

/// Errors that can occur while converting a tree-sitter BNF parse tree into the DOM.
#[derive(Debug)]
pub enum ParseError {
    /// A node had a different kind than required; carries the expected and actual kind strings.
    UnexpectedNodeType {
        /// The node kind that was required at this position.
        expected: String,
        /// The node kind that was actually encountered.
        got: String,
    },
    /// A node kind was not recognised by any visitor branch.
    UnknownNodeKind(String),
    /// The left-hand side of a production rule was not a non-terminal.
    MalformedProduction,
    /// The source text contains tree-sitter syntax errors.
    SyntaxError,
    /// The tree-sitter parser returned no tree for the input.
    ParseFailed,
    /// `%include` was used but the source has no associated file path (e.g. stdin).
    IncludeFromStdin,
    /// The path in a `%include` directive could not be read; carries the resolved absolute path.
    IncludeNotFound(String),
    /// A `%include` chain forms a cycle; carries the path that was seen twice.
    IncludeCycle(String),
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
            ParseError::IncludeFromStdin => {
                write!(f, "%include cannot be used when reading from stdin")
            }
            ParseError::IncludeNotFound(path) => {
                write!(f, "included file not found: {}", path)
            }
            ParseError::IncludeCycle(path) => {
                write!(f, "circular %include detected: {}", path)
            }
        }
    }
}

impl std::error::Error for ParseError {}
