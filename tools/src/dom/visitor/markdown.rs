// Small, dependency-free markdown-rendering helpers, kept separate from the
// rest of the Rust emitter so this is the one place to swap in a real
// markdown-table crate later if hand-rolled column alignment stops being
// enough for what the generated doc comments need.

/// Renders `rows` (row 0 is the header) as a GFM pipe table, with both
/// columns padded to their widest cell so the *source* text lines up, not
/// just the rendered output.
///
/// Returns plain markdown with no `///` doc-comment prefix on each line —
/// this function knows nothing about Rust doc comments, only about
/// markdown tables; callers decide how the result gets embedded.
///
/// For example, `render_table(&[["a", "bb"], ["ccc", "d"]])` returns:
///
/// ```text
/// | a   | bb |
/// |-----|----|
/// | ccc | d  |
/// ```
pub(crate) fn render_table(rows: &[[&str; 2]]) -> String {
    let width = |col: usize| rows.iter().map(|r| r[col].len()).max().unwrap_or(0);
    let (w0, w1) = (width(0), width(1));

    let mut lines = Vec::with_capacity(rows.len() + 1);
    for (i, row) in rows.iter().enumerate() {
        lines.push(format!("| {:w0$} | {:w1$} |", row[0], row[1]));
        if i == 0 {
            lines.push(format!("|{}|{}|", "-".repeat(w0 + 2), "-".repeat(w1 + 2)));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both columns are padded to their own widest cell, independently.
    #[test]
    fn render_table_pads_columns_to_widest_cell() {
        let rows = [["a", "bb"], ["ccc", "d"]];
        assert_eq!(
            render_table(&rows),
            "| a   | bb |\n|-----|----|\n| ccc | d  |"
        );
    }

    /// A header-only table still emits the separator row.
    #[test]
    fn render_table_header_only() {
        assert_eq!(render_table(&[["h1", "h2"]]), "| h1 | h2 |\n|----|----|");
    }

    /// A cell wider than every other cell in its column sets that column's
    /// width; the other column is unaffected.
    #[test]
    fn render_table_widest_cell_in_one_column_does_not_affect_the_other() {
        let rows = [["short", "y"], ["a very long header cell", "z"]];
        let out = render_table(&rows);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "| short                   | y |");
        assert_eq!(lines[2], "| a very long header cell | z |");
    }
}
