use std::fmt;

/// The severity of a [`Diagnostic`] message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A hard error: the grammar cannot be used as-is (e.g. an undefined
    /// rule reference). Left recursion is *not* an example of this — it's a
    /// supported grammar property, not a defect (see
    /// [`super::analysis::left_recursive_rules`]'s doc comment); no
    /// diagnostic is ever emitted for it.
    Error,
    /// A warning: the grammar is suspicious but can still be converted.
    Warning,
}

/// A structured diagnostic message produced by grammar analysis.
///
/// Diagnostics are emitted by [`Grammar::check`](super::types::Grammar) and carry a
/// [`Severity`] so callers can distinguish hard errors from advisory warnings.
///
/// `file`/`line`/`column` are optional structured source-location fields
/// (issue #319): populated whenever the diagnostic has a concrete source
/// position to point at, `None` for diagnostics that don't (e.g. a
/// grammar-name validity check). They're additive to `check --json`'s
/// existing `{severity, message}` shape — omitted from the serialized
/// output entirely when absent, so existing consumers are unaffected.
/// `message` itself never has a location baked into it: [`Display`](fmt::Display)
/// (and [`located_message`](Self::located_message)) append it as a suffix,
/// computed from these fields, at render time.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    /// Whether this is a hard error or an advisory warning.
    pub severity: Severity,
    /// The human-readable message body, without a severity prefix.
    pub message: String,
    /// Source filename this diagnostic points at, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based source line this diagnostic points at, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// 1-based source column this diagnostic points at, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

impl Diagnostic {
    /// Creates a [`Severity::Warning`] diagnostic with the given message and no location.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            file: None,
            line: None,
            column: None,
        }
    }

    /// Creates a [`Severity::Error`] diagnostic with the given message and no location.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            file: None,
            line: None,
            column: None,
        }
    }

    /// Attaches a source file/line to this diagnostic, builder-style.
    ///
    /// An empty `filename` (meaning "unknown source file", e.g. a grammar
    /// parsed from a string with no path) is recorded as `file: None`.
    pub(crate) fn with_location(mut self, filename: &str, line: usize) -> Self {
        self.file = (!filename.is_empty()).then(|| filename.to_string());
        self.line = Some(line);
        self
    }

    /// Attaches a source column to this diagnostic, builder-style.
    ///
    /// Only meaningful after [`with_location`](Self::with_location); most checks
    /// only have a line to point at, since they're driven off `Production`/
    /// `DirectiveItem` bookkeeping rather than a tree-sitter node.
    pub(crate) fn with_column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    /// Renders the `" (location)"` suffix for this diagnostic's `file`/`line`/`column`
    /// fields, or `""` when there's no location at all. Mirrors the old baked-in-message
    /// text exactly: `" (file:line)"`/`" (file:line:col)"` when `file` is set, falling
    /// back to `" (line N)"`/`" (line N:col)"` when it isn't (an unknown source file,
    /// e.g. a grammar parsed from a string with no path).
    fn location_suffix(&self) -> String {
        match (&self.file, self.line, self.column) {
            (Some(file), Some(line), Some(col)) => format!(" ({file}:{line}:{col})"),
            (Some(file), Some(line), None) => format!(" ({file}:{line})"),
            (None, Some(line), Some(col)) => format!(" (line {line}:{col})"),
            (None, Some(line), None) => format!(" (line {line})"),
            _ => String::new(),
        }
    }

    /// Renders `message` with its location suffix, but no severity prefix.
    ///
    /// For embedding into another type's own `Display` — e.g.
    /// [`ParseError::SyntaxError`](super::ParseError::SyntaxError), which already
    /// supplies its own "error:" framing upstream and would otherwise double it up
    /// if it went through this type's own [`Display`](fmt::Display) impl instead.
    pub(crate) fn located_message(&self) -> String {
        format!("{}{}", self.message, self.location_suffix())
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{prefix}: {}", self.located_message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Severity must serialize lowercase and Diagnostic as `{severity, message}`;
    /// consumers of `check --json` depend on this exact shape.
    fn diagnostic_serializes_with_lowercase_severity() {
        assert_eq!(
            serde_json::to_string(&Diagnostic::warning("undefined rule reference 'foo'")).unwrap(),
            r#"{"severity":"warning","message":"undefined rule reference 'foo'"}"#
        );
        assert_eq!(
            serde_json::to_string(&Diagnostic::error("%axiom references undefined rule 'foo'"))
                .unwrap(),
            r#"{"severity":"error","message":"%axiom references undefined rule 'foo'"}"#
        );
    }

    #[test]
    /// A freshly-constructed diagnostic has no location.
    fn diagnostic_starts_with_no_location() {
        let d = Diagnostic::warning("msg");
        assert_eq!(d.file, None);
        assert_eq!(d.line, None);
        assert_eq!(d.column, None);
    }

    #[test]
    /// `with_location` records a non-empty filename and line as `Some`.
    fn with_location_records_file_and_line() {
        let d = Diagnostic::error("msg").with_location("g.bnf", 3);
        assert_eq!(d.file.as_deref(), Some("g.bnf"));
        assert_eq!(d.line, Some(3));
        assert_eq!(d.column, None);
    }

    #[test]
    /// An empty filename means "unknown source file" and is recorded as `None`,
    /// so the rendered location suffix falls back to bare "line N" text for
    /// the same input.
    fn with_location_empty_filename_is_none() {
        let d = Diagnostic::error("msg").with_location("", 3);
        assert_eq!(d.file, None);
        assert_eq!(d.line, Some(3));
    }

    #[test]
    /// `with_column` layers onto `with_location` without disturbing file/line.
    fn with_column_layers_onto_with_location() {
        let d = Diagnostic::error("msg")
            .with_location("g.bnf", 3)
            .with_column(7);
        assert_eq!(d.file.as_deref(), Some("g.bnf"));
        assert_eq!(d.line, Some(3));
        assert_eq!(d.column, Some(7));
    }

    #[test]
    /// Location fields are additive to `check --json`'s existing shape: present
    /// only when set, so a located diagnostic gains keys but an unlocated one
    /// (covered by `diagnostic_serializes_with_lowercase_severity` above)
    /// serializes exactly as before.
    fn diagnostic_serializes_location_when_present() {
        let d = Diagnostic::warning("msg")
            .with_location("g.bnf", 3)
            .with_column(7);
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"severity":"warning","message":"msg","file":"g.bnf","line":3,"column":7}"#
        );
    }

    // ── Display / located_message: location suffix computed at render time ────

    #[test]
    /// No location at all: no suffix, just "severity: message".
    fn display_no_location() {
        assert_eq!(Diagnostic::error("msg").to_string(), "error: msg");
    }

    #[test]
    /// `file` + `line`, no `column`: `" (file:line)"`.
    fn display_file_and_line() {
        let d = Diagnostic::warning("msg").with_location("g.bnf", 3);
        assert_eq!(d.to_string(), "warning: msg (g.bnf:3)");
    }

    #[test]
    /// `file` + `line` + `column`: `" (file:line:col)"`.
    fn display_file_line_and_column() {
        let d = Diagnostic::error("msg")
            .with_location("g.bnf", 3)
            .with_column(7);
        assert_eq!(d.to_string(), "error: msg (g.bnf:3:7)");
    }

    #[test]
    /// No `file` (unknown source), `line` only: falls back to `" (line N)"`.
    fn display_line_only_no_file() {
        let d = Diagnostic::warning("msg").with_location("", 3);
        assert_eq!(d.to_string(), "warning: msg (line 3)");
    }

    #[test]
    /// No `file`, `line` + `column`: falls back to `" (line N:col)"`.
    fn display_line_and_column_no_file() {
        let d = Diagnostic::error("msg").with_location("", 3).with_column(7);
        assert_eq!(d.to_string(), "error: msg (line 3:7)");
    }

    #[test]
    /// `located_message` is `Display` minus the severity prefix — used by
    /// `ParseError::SyntaxError`, which supplies its own framing.
    fn located_message_omits_severity_prefix() {
        let d = Diagnostic::error("msg").with_location("g.bnf", 3);
        assert_eq!(d.located_message(), "msg (g.bnf:3)");
    }
}
