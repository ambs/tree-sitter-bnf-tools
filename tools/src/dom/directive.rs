/// A name referenced in a grammar directive (`%inline`, `%supertypes`, `%extras`),
/// together with the 1-based source line of the directive that introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveItem {
    /// The rule name or pattern (for `%extras`).
    pub name: String,
    /// 1-based line number of the directive in the source file.
    pub line: usize,
}

/// A conflict group declared with `%conflicts`, together with its 1-based source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictGroup {
    /// The rule names that form this conflict group.
    pub rules: Vec<String>,
    /// 1-based line number of the `%conflicts` directive in the source file.
    pub line: usize,
}
