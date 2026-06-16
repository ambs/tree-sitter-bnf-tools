/// A name referenced in a grammar directive (`%inline`, `%supertypes`, `%extras`),
/// together with the 1-based source line of the directive that introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveItem {
    /// The rule name or pattern (for `%extras`).
    pub name: String,
    /// 1-based line number of the directive in the source file.
    pub line: usize,
    /// Source filename where this directive appears (empty string if unknown).
    pub filename: String,
}

/// A conflict group declared with `%conflicts`, together with its 1-based source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictGroup {
    /// The rule names that form this conflict group.
    pub rules: Vec<String>,
    /// 1-based line number of the `%conflicts` directive in the source file.
    pub line: usize,
    /// Source filename where this directive appears (empty string if unknown).
    pub filename: String,
}

/// An item that can be a non terminal or a literal, used for instance inside a `%precedences` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameOrLiteral {
    /// a rule-name reference
    Name(String),
    /// a quoted string literal
    Literal(String),
}

/// A precedences group declared with `%precedences`, together with its 1-based source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecedenceGroup {
    /// The groups of rules with same precedences
    pub items: Vec<NameOrLiteral>,
    /// 1-based line number of the `%precedences` directive in the source file.
    pub line: usize,
    /// Source filename where this directive appears (empty string if unknown).
    pub filename: String,
}

/// Formats a source location for use in diagnostic messages.
///
/// Returns `"filename:line"` when `filename` is non-empty, or `"line N"` otherwise.
/// The result is suitable for embedding in parentheses: `format!("... ({loc})")`.
pub(crate) fn loc(filename: &str, line: usize) -> String {
    if filename.is_empty() {
        format!("line {line}")
    } else {
        format!("{filename}:{line}")
    }
}

/// Formats a source location like [`loc`], with a column appended: `"filename:line:col"`.
pub(crate) fn loc_col(filename: &str, line: usize, col: usize) -> String {
    format!("{}:{col}", loc(filename, line))
}
