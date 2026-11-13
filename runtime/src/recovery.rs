//! What recovering a typed tree from input with syntax errors reports:
//! [`Diagnostic`]s, and [`Recovered`], a value together with all of them.

use crate::Span;

/// How many levels of nesting a recovery builds by default. Building is
/// recursive — a few calls per level — so a tree nested deeper than the
/// stack can take does not fail, it *aborts the process*; and `Drop` and
/// `Debug` on the typed tree are recursive too. 256 keeps even a debug
/// build safe on a 2 MiB thread. A caller with a bigger stack can raise it
/// with the generated crate's `recover_with_limit`.
pub const DEFAULT_MAX_DEPTH: usize = 256;

/// What kind of problem a [`Diagnostic`] reports.
#[derive(Debug)]
pub enum DiagnosticKind {
    /// A tree-sitter `ERROR` node: source text the parser could not fit
    /// into the grammar. Reported once per error region, not once per
    /// nested `ERROR` node.
    Error,
    /// A tree-sitter `MISSING` node: a zero-width stand-in for something
    /// the grammar required.
    Missing {
        /// The kind (or token) the `MISSING` node stands in for.
        expected: String,
    },
    /// A field absent from a node whose subtree contains a syntax error.
    MissingField {
        /// The kind of the node the field is missing from.
        kind: &'static str,
        /// The field's label in the grammar.
        field: &'static str,
    },
    /// A child whose kind is outside what `expected` can hold: the variant
    /// set of a multi-shape (or merged) enum, or the kinds a node's
    /// unlabelled children can have.
    UnexpectedKind {
        /// The type that was being built.
        expected: &'static str,
        /// The kind of the child that does not fit it.
        found: String,
    },
    /// A subtree nested deeper than the recovery's limit
    /// ([`DEFAULT_MAX_DEPTH`], or the one given to the generated crate's
    /// `recover_with_limit`): it was not built, so the node at this
    /// diagnostic's span is missing from the tree. Building the typed tree is
    /// recursive, and a tree nested deeper than the stack can take would
    /// otherwise abort the process.
    TooDeep {
        /// The depth limit that was in force.
        limit: usize,
    },
}

/// One problem found while recovering a typed AST from a parse tree.
#[derive(Debug)]
pub struct Diagnostic {
    /// What went wrong.
    pub kind: DiagnosticKind,
    /// Where: the offending node's extent (empty for `MISSING`).
    pub span: Span,
}

/// The result of recovering a typed AST: whatever could be built, and
/// everything that could not.
#[derive(Debug)]
pub struct Recovered<T> {
    /// The recovered value, or `None` if not even the root could be built.
    pub value: Option<T>,
    /// Every problem found, ordered by position in the source.
    pub diagnostics: Vec<Diagnostic>,
}

impl<T> Recovered<T> {
    /// `true` if a value was built and no problem was found.
    pub fn is_complete(&self) -> bool {
        self.value.is_some() && self.diagnostics.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Pragma;

    /// An empty span at the start of a file, for tests that don't care where.
    fn span() -> Span {
        Span {
            byte_range: 0..0,
            start: Pragma { line: 1, column: 1 },
            end: Pragma { line: 1, column: 1 },
        }
    }

    /// A diagnostic of the simplest kind.
    fn diagnostic() -> Diagnostic {
        Diagnostic {
            kind: DiagnosticKind::Error,
            span: span(),
        }
    }

    /// Complete means a value *and* no diagnostics: each half alone is not.
    #[test]
    fn is_complete_needs_a_value_and_no_diagnostics() {
        let complete = Recovered {
            value: Some(1),
            diagnostics: Vec::new(),
        };
        assert!(complete.is_complete());

        let with_problem = Recovered {
            value: Some(1),
            diagnostics: vec![diagnostic()],
        };
        assert!(
            !with_problem.is_complete(),
            "a value with a problem is partial"
        );

        let nothing_built: Recovered<i32> = Recovered {
            value: None,
            diagnostics: Vec::new(),
        };
        assert!(
            !nothing_built.is_complete(),
            "no value is not complete, even without a problem"
        );

        let nothing_and_problem: Recovered<i32> = Recovered {
            value: None,
            diagnostics: vec![diagnostic()],
        };
        assert!(!nothing_and_problem.is_complete());
    }

    /// The default limit is the one the docs promise to be safe on a small stack.
    #[test]
    fn default_depth_limit_is_256() {
        assert_eq!(DEFAULT_MAX_DEPTH, 256);
    }
}
