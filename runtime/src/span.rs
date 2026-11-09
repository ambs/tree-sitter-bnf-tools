//! Where a node is in the source text: [`Pragma`] (a position), [`Span`] (a
//! node's whole extent), and the two kinds of node that are nothing but text
//! and a place, [`Token`] and [`ErrorRegion`].

/// A node's start position in its source file.
/// 1-indexed (tree-sitter uses 0-index; the conversion from a tree-sitter
/// node does that once).
/// `column` counts characters (Unicode scalar values), not bytes
/// as tree-sitter's own column does, and does not count a leading
/// byte-order mark (U+FEFF), so it matches what an editor shows.
#[derive(Debug)]
pub struct Pragma {
    /// The line, counting from 1.
    pub line: usize,
    /// The column, counting from 1, in characters rather than bytes.
    pub column: usize,
}

/// A node's extent in the source text it was parsed from.
#[derive(Debug)]
pub struct Span {
    /// Byte offsets into the exact text that was parsed, as tree-sitter
    /// reports them: a leading byte-order mark counts as three bytes here,
    /// so these always index straight into the source.
    pub byte_range: std::ops::Range<usize>,
    /// Where the node starts. 1-indexed; see [`Pragma`].
    pub start: Pragma,
    /// Where the node ends: the position just past its last character,
    /// counted the same way as `start`.
    pub end: Pragma,
}

/// A literal in the source — a keyword, punctuation or an operator: an
/// anonymous tree-sitter node. It keeps its position, so it can be located,
/// and (with `--partial-trees`) ordered among the named nodes around it.
#[derive(Debug)]
pub struct Token {
    /// The node's tree-sitter kind; for a literal, normally its own text.
    /// Owned, not `&'static str`: whether `Node::kind` is `'static` or
    /// borrows from the node depends on the tree-sitter version.
    pub kind: String,
    /// The token's source text.
    pub text: String,
    /// Where the token is in the source.
    pub span: Span,
}

/// A region of source text tree-sitter could not fit into the grammar: an
/// `ERROR` node, kept where it sits in the tree so that it can be located,
/// and walked past like any other child. It has no children of its own:
/// whatever tree-sitter did recognise inside it is simply part of its text.
#[derive(Debug)]
pub struct ErrorRegion {
    /// Where the region is in the source.
    pub span: Span,
    /// The region's source text, as written.
    pub text: String,
}
