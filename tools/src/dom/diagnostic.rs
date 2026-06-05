use std::fmt;

/// The severity of a [`Diagnostic`] message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A hard error: the grammar cannot be used as-is (e.g. left-recursion).
    Error,
    /// A warning: the grammar is suspicious but can still be converted.
    Warning,
}

/// A structured diagnostic message produced by grammar analysis.
///
/// Diagnostics are emitted by [`Grammar::check`](super::types::Grammar) and carry a
/// [`Severity`] so callers can distinguish hard errors from advisory warnings.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    /// Whether this is a hard error or an advisory warning.
    pub severity: Severity,
    /// The human-readable message body, without a severity prefix.
    pub message: String,
}

impl Diagnostic {
    /// Creates a [`Severity::Warning`] diagnostic with the given message.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    /// Creates a [`Severity::Error`] diagnostic with the given message.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{prefix}: {}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_display() {
        let d = Diagnostic::warning("undefined rule reference 'foo'");
        assert_eq!(d.to_string(), "warning: undefined rule reference 'foo'");
    }

    #[test]
    fn error_display() {
        let d = Diagnostic::error("rule 'expr' is directly left-recursive");
        assert_eq!(
            d.to_string(),
            "error: rule 'expr' is directly left-recursive"
        );
    }

    #[test]
    fn severity_is_preserved() {
        assert_eq!(Diagnostic::warning("x").severity, Severity::Warning);
        assert_eq!(Diagnostic::error("x").severity, Severity::Error);
    }
}
