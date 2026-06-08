use std::fmt;

/// Statistics about FIRST-set sizes across all rules in a grammar.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FirstSetStats {
    /// Smallest FIRST set (number of distinct leading terminals).
    pub min: usize,
    /// Largest FIRST set.
    pub max: usize,
    /// Mean FIRST set size, rounded to one decimal place.
    pub avg: f64,
}

/// A compact summary of grammar shape and analysis results.
///
/// Produced by [`Grammar::summarise`](super::types::Grammar) and printed by
/// `check --summary`. All counts reflect the grammar after parsing; no
/// additional passes beyond those already run by `check` are repeated here,
/// except FIRST-set computation which is deferred until this struct is built.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GrammarSummary {
    /// Total number of named productions.
    pub rules: usize,
    /// Productions whose body contains no non-terminal references.
    pub leaf_rules: usize,
    /// Productions that are never referenced (directly or transitively) from the root.
    pub unreachable_rules: usize,
    /// Unique string literal terminals across all rule bodies.
    pub unique_literals: usize,
    /// Unique regex pattern terminals across all rule bodies.
    pub unique_patterns: usize,
    /// Number of undefined rule references detected by `check`.
    pub undefined_refs: usize,
    /// Rules that are directly left-recursive (`A → A …`).
    pub left_recursive_direct: usize,
    /// Rules involved in mutual left-recursion but not directly so.
    pub left_recursive_mutual: usize,
    /// FIRST-set statistics, or `None` if the grammar has no productions.
    pub first_sets: Option<FirstSetStats>,
}

impl fmt::Display for GrammarSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let left_recursive = self.left_recursive_direct + self.left_recursive_mutual;
        let terminals = self.unique_literals + self.unique_patterns;
        let first_sets_value = match &self.first_sets {
            Some(fs) => format!("min {}  max {}  avg {}", fs.min, fs.max, fs.avg),
            None => "n/a".to_string(),
        };

        // Each row is (label, count, detail). The FIRST sets row has no count
        // column — its value goes in the detail field directly.
        let rows: &[(&str, Option<usize>, String)] = &[
            (
                "Rules",
                Some(self.rules),
                format!(
                    "(leaf: {}, unreachable: {})",
                    self.leaf_rules, self.unreachable_rules
                ),
            ),
            (
                "Terminals",
                Some(terminals),
                format!(
                    "(literals: {}, patterns: {}, unique values)",
                    self.unique_literals, self.unique_patterns
                ),
            ),
            ("Undefined refs", Some(self.undefined_refs), String::new()),
            (
                "Left-recursive",
                Some(left_recursive),
                format!(
                    "(direct: {}, mutual: {})",
                    self.left_recursive_direct, self.left_recursive_mutual
                ),
            ),
            ("FIRST sets", None, first_sets_value),
        ];

        // Derive column widths from the actual data so adding a row never
        // requires touching this code.
        let label_w = rows.iter().map(|(lbl, ..)| lbl.len()).max().unwrap_or(0);
        let count_w = rows
            .iter()
            .filter_map(|(_, count, _)| *count)
            .map(|n| n.to_string().len())
            .max()
            .unwrap_or(1);

        let last = rows.len() - 1;
        for (i, (label, count, detail)) in rows.iter().enumerate() {
            let line = match count {
                Some(n) if detail.is_empty() => {
                    format!("{:<label_w$}  {:>count_w$}", label, n)
                }
                Some(n) => {
                    format!("{:<label_w$}  {:>count_w$}  {}", label, n, detail)
                }
                None => {
                    format!("{:<label_w$}  {}", label, detail)
                }
            };
            if i < last {
                writeln!(f, "{line}")?;
            } else {
                write!(f, "{line}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> GrammarSummary {
        GrammarSummary {
            rules: 42,
            leaf_rules: 8,
            unreachable_rules: 1,
            unique_literals: 12,
            unique_patterns: 6,
            undefined_refs: 0,
            left_recursive_direct: 0,
            left_recursive_mutual: 0,
            first_sets: Some(FirstSetStats {
                min: 1,
                max: 8,
                avg: 3.2,
            }),
        }
    }

    #[test]
    /// All five rows are present in the output.
    fn display_contains_all_rows() {
        let s = summary().to_string();
        assert!(s.contains("Rules"));
        assert!(s.contains("Terminals"));
        assert!(s.contains("Undefined refs"));
        assert!(s.contains("Left-recursive"));
        assert!(s.contains("FIRST sets"));
    }

    #[test]
    /// Counts and detail values appear correctly.
    fn display_counts_and_details() {
        let s = summary().to_string();
        assert!(s.contains("42"));
        assert!(s.contains("leaf: 8"));
        assert!(s.contains("unreachable: 1"));
        assert!(s.contains("literals: 12"));
        assert!(s.contains("patterns: 6"));
        assert!(s.contains("min 1"));
        assert!(s.contains("max 8"));
        assert!(s.contains("avg 3.2"));
    }

    #[test]
    /// No trailing newline — the caller controls line endings.
    fn display_no_trailing_newline() {
        assert!(!summary().to_string().ends_with('\n'));
    }

    #[test]
    /// When first_sets is None the FIRST sets row shows "n/a".
    fn display_first_sets_none_shows_na() {
        let mut s = summary();
        s.first_sets = None;
        assert!(s.to_string().contains("n/a"));
    }

    #[test]
    /// All label column entries are left-aligned to the same width.
    fn display_labels_aligned() {
        let output = summary().to_string();
        // The longest label is "Undefined refs" / "Left-recursive" (14 chars).
        // Every line must start with enough chars to reach at least that width.
        let label_w = "Undefined refs".len();
        for line in output.lines() {
            assert!(
                line.len() > label_w,
                "line too short to be aligned: {line:?}"
            );
        }
    }
}
