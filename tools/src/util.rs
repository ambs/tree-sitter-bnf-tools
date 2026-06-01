/// Removes all `comment` tokens from `source` and normalises surrounding whitespace.
///
/// Uses the tree-sitter parse tree to identify comment byte-ranges precisely, so
/// `#` characters inside terminal patterns are never misidentified as comments.
/// After removal, trailing spaces left on lines that had inline comments are
/// stripped, consecutive blank lines are collapsed to at most one, and leading
/// blank lines are removed.  The result ends with exactly one `\n`.
pub fn strip_comments_from_source(source: &str) -> String {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return source.to_string(),
    };

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    collect_comment_bytes(tree.root_node(), &mut ranges);

    let mut stripped = String::with_capacity(source.len());
    let mut pos = 0;
    for (start, end) in &ranges {
        stripped.push_str(&source[pos..*start]);
        pos = *end;
    }
    stripped.push_str(&source[pos..]);

    normalize_stripped(&stripped)
}

/// Recursively collects the byte ranges of every `comment` node in the subtree rooted at `node`.
fn collect_comment_bytes(node: tree_sitter::Node<'_>, ranges: &mut Vec<(usize, usize)>) {
    if node.kind() == "comment" {
        ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            collect_comment_bytes(child, ranges);
        }
    }
}

/// Strips trailing whitespace from each line, collapses consecutive blank lines to at
/// most one, removes leading blank lines, and ensures a single trailing newline.
fn normalize_stripped(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut blank_run = 0usize;
    let mut past_first_content = false;

    for line in s.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
        } else {
            if past_first_content && blank_run > 0 {
                result.push('\n');
            }
            past_first_content = true;
            blank_run = 0;
            result.push_str(trimmed);
            result.push('\n');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn no_comments_unchanged() {
        let src = indoc! {"
            rule -> foo | bar;
        "};
        assert_eq!(strip_comments_from_source(src), src);
    }

    #[test]
    fn leading_comment_line_removed() {
        assert_eq!(
            strip_comments_from_source(indoc! {"
                # header
                rule -> foo;
            "}),
            indoc! {"
                rule -> foo;
            "}
        );
    }

    #[test]
    fn inline_comment_stripped_with_trailing_space() {
        assert_eq!(
            strip_comments_from_source(indoc! {"
                rule -> foo; # inline
            "}),
            indoc! {"
                rule -> foo;
            "}
        );
    }

    #[test]
    fn multiple_comment_lines_collapse_blank() {
        assert_eq!(
            strip_comments_from_source(indoc! {"
                # a
                # b
                rule -> foo;
            "}),
            indoc! {"
                rule -> foo;
            "}
        );
    }

    #[test]
    fn comment_between_rules_preserves_blank_line() {
        // The comment-only line's trailing \n is kept, leaving a blank line between rules.
        assert_eq!(
            strip_comments_from_source(indoc! {"
                rule1 -> foo;
                # between
                rule2 -> bar;
            "}),
            indoc! {"
                rule1 -> foo;

                rule2 -> bar;
            "}
        );
    }

    #[test]
    fn blank_line_plus_comment_between_rules() {
        assert_eq!(
            strip_comments_from_source(indoc! {"
                rule1 -> foo;

                # between
                rule2 -> bar;
            "}),
            indoc! {"
                rule1 -> foo;

                rule2 -> bar;
            "}
        );
    }
}
