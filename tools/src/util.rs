use tree_sitter::Node;

use crate::{
    dom::{Diagnostic, directive::loc_col},
    visitors::SourceFile,
};

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

/// Converts a snake_case grammar name to UpperCamelCase for the `camelcase` field in `tree-sitter.json`.
pub fn to_camelcase(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Returns the first line of `text`, char-truncated to 30 characters with a trailing '…'.
fn snippet(text: &str) -> String {
    let mut chars = text.lines().next().unwrap_or("").chars();
    let mut out: String = chars.by_ref().take(30).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// Recursively pushes one [`Diagnostic`] for each ERROR or MISSING node in the subtree.
///
/// ERROR nodes report a snippet of the offending text (first line, char-truncated);
/// MISSING nodes report the kind of the token the parser expected. Children of an
/// ERROR node are not visited, and subtrees without errors are pruned via
/// `has_error()`.
fn collect_syntax_errors(node: &Node<'_>, ctx: &SourceFile<'_>, messages: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        let pos = node.start_position();
        // Stdin has no meaningful filename; fall back to loc()'s bare "line N" form.
        let filename = if ctx.filename == "-" {
            ""
        } else {
            ctx.filename
        };
        let pragma = loc_col(filename, pos.row + 1, pos.column + 1);
        let text = node.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8");
        let message = if node.is_error() {
            format!("syntax error at {pragma} near '{}'", snippet(text))
        } else {
            format!("syntax error at {pragma}: missing '{}'", node.kind())
        };

        messages.push(Diagnostic::error(message));
        return;
    }

    if !node.has_error() {
        return;
    }

    for i in 0..node.child_count() as u32 {
        if let Some(child) = node.child(i) {
            collect_syntax_errors(&child, ctx, messages);
        }
    }
}

/// Maximum number of syntax-error diagnostics reported before summarising.
const MAX_SYNTAX_ERRORS: usize = 10;

/// Walks a parse tree and reports each syntax error as a located [`Diagnostic`].
///
/// Every ERROR or MISSING node yields one error diagnostic carrying file, line,
/// column and context. At most [`MAX_SYNTAX_ERRORS`] are reported; any excess is
/// summarised in a final "… and N more syntax errors" diagnostic.
pub fn syntax_error_diagnostics(root: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<Diagnostic> {
    let mut messages = Vec::new();
    collect_syntax_errors(root, ctx, &mut messages);
    if messages.len() > MAX_SYNTAX_ERRORS {
        let extra = messages.len() - MAX_SYNTAX_ERRORS;
        messages.truncate(MAX_SYNTAX_ERRORS);
        messages.push(Diagnostic::error(format!(
            "… and {extra} more syntax errors"
        )));
    }
    messages
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── to_camelcase ──────────────────────────────────────────────────────────

    #[test]
    fn camelcase_single_word() {
        assert_eq!(to_camelcase("json"), "Json");
    }

    #[test]
    fn camelcase_snake_case() {
        assert_eq!(to_camelcase("my_lang"), "MyLang");
    }

    #[test]
    fn camelcase_multiple_parts() {
        assert_eq!(to_camelcase("tree_sitter_cpp"), "TreeSitterCpp");
    }

    #[test]
    fn camelcase_already_capitalized() {
        assert_eq!(to_camelcase("Json"), "Json");
    }

    #[test]
    fn camelcase_empty_string() {
        assert_eq!(to_camelcase(""), "");
    }

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

    // ── syntax_error_diagnostics ──────────────────────────────────────────────

    /// Parses `src` as BNF, returning the tree (which may contain errors).
    fn parse_bnf(src: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bnf::LANGUAGE.into())
            .unwrap();
        parser.parse(src, None).unwrap()
    }

    /// Runs the collector over `src` under the given `filename`.
    fn syntax_diags(src: &str, filename: &str) -> Vec<Diagnostic> {
        let tree = parse_bnf(src);
        let ctx = SourceFile {
            source: src,
            filename,
            path: None,
        };
        syntax_error_diagnostics(&tree.root_node(), &ctx)
    }

    #[test]
    /// A single ERROR node yields one error diagnostic with file:line:col and a snippet.
    fn syntax_single_error_reports_location_and_snippet() {
        let diags = syntax_diags("root => 'a' ;\n", "g.bnf");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, crate::dom::Severity::Error);
        assert_eq!(
            diags[0].message,
            "syntax error at g.bnf:1:1 near 'root => 'a' ;'"
        );
    }

    #[test]
    /// A MISSING node reports the kind of the token the parser expected.
    fn syntax_missing_node_reports_expected_kind() {
        let diags = syntax_diags("root -> 'a'\n", "g.bnf");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "syntax error at g.bnf:1:12: missing ';'");
    }

    #[test]
    /// Independent errors on separate lines each get their own diagnostic.
    fn syntax_multiple_errors_each_reported() {
        let diags = syntax_diags("root -> ;\nfoo --> bar ;\n", "g.bnf");
        assert_eq!(diags.len(), 2);
        assert_eq!(
            diags[0].message,
            "syntax error at g.bnf:1:8: missing 'pattern'"
        );
        assert_eq!(diags[1].message, "syntax error at g.bnf:2:5 near '-'");
    }

    #[test]
    /// More than MAX_SYNTAX_ERRORS errors are capped, with a trailing summary diagnostic.
    fn syntax_errors_capped_with_summary() {
        let src: String = (0..15).map(|i| format!("r{i} -> ;\n")).collect();
        let diags = syntax_diags(&src, "g.bnf");
        assert_eq!(diags.len(), MAX_SYNTAX_ERRORS + 1);
        assert_eq!(diags.last().unwrap().message, "… and 5 more syntax errors");
        assert!(
            diags[MAX_SYNTAX_ERRORS - 1]
                .message
                .starts_with("syntax error at g.bnf:10:")
        );
    }

    #[test]
    /// Snippets are char-truncated to 30 characters with a trailing ellipsis.
    fn syntax_snippet_truncated_to_thirty_chars() {
        let src = format!("root => '{}' ;\n", "a".repeat(40));
        let diags = syntax_diags(&src, "g.bnf");
        assert_eq!(diags.len(), 1);
        let head: String = src.chars().take(30).collect();
        assert_eq!(
            diags[0].message,
            format!("syntax error at g.bnf:1:1 near '{head}…'")
        );
    }

    #[test]
    /// Stdin input ("-") omits the file part, falling back to loc()'s "line N" form.
    fn syntax_stdin_omits_filename() {
        let diags = syntax_diags("root => 'a' ;\n", "-");
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "syntax error at line 1:1 near 'root => 'a' ;'"
        );
    }
}
