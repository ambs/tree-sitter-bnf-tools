// Small, language-agnostic text-formatting helpers shared by every
// target-language emitter (see the parent `visitor` module): indentation and
// comment-line prefixing, the parts of "splice this block of text into a
// larger generated file" that have nothing to do with which language is
// being generated. Kept separate from `rust.rs` so a future emitter for
// another language reuses these instead of re-implementing them.

/// Indents every non-empty line of `text` by `spaces` spaces, leaving empty
/// lines untouched so no line ends in trailing whitespace.
///
/// For example, `indent("a\n\nb", 4)` returns `"    a\n\n    b"`.
pub(crate) fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prefixes every line of `text` with a comment marker, so a block of plain
/// text can be spliced into a commented section of a generated file.
///
/// `marker` is the bare marker with no trailing space (`"///"` for a Rust
/// doc comment, `"#"` for a Python one) — this function owns the separating
/// space, so callers never need to remember to type it. A non-empty line
/// gets `marker` followed by a space then the line; a blank line gets the
/// bare `marker` with nothing after it, *not* `marker` plus a dangling
/// space — important for Rust doc comments specifically, where a bare
/// `///` between two paragraphs keeps them in the same doc comment (and
/// rustdoc renders it as a paragraph break), while a *truly* blank,
/// unmarked line would end the doc comment early and silently merge the
/// two paragraphs into one when rendered.
///
/// For example, `prefix_lines("a\n\nb", "///")` returns `"/// a\n///\n/// b"`.
pub(crate) fn prefix_lines(text: &str, marker: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                marker.to_string()
            } else {
                format!("{marker} {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-empty lines get the padding; blank lines are left alone.
    #[test]
    fn indent_pads_non_empty_lines_only() {
        assert_eq!(indent("a\n\nb", 4), "    a\n\n    b");
    }

    /// Zero spaces is a no-op.
    #[test]
    fn indent_zero_spaces_is_a_no_op() {
        assert_eq!(indent("a\nb", 0), "a\nb");
    }

    /// A non-empty line gets the marker plus one separating space.
    #[test]
    fn prefix_lines_adds_one_space_after_the_marker() {
        assert_eq!(prefix_lines("a\nb", "///"), "/// a\n/// b");
    }

    /// A blank line gets the bare marker, with no trailing space.
    #[test]
    fn prefix_lines_blank_line_gets_bare_marker_no_trailing_space() {
        let out = prefix_lines("a\n\nb", "///");
        assert_eq!(out, "/// a\n///\n/// b");
        assert!(!out.lines().any(|l| l.ends_with(' ')));
    }
}
