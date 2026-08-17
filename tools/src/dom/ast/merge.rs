use crate::dom::{Grammar, visible_kinds};
use crate::util::find_first_name_collision;

/// A parsed `--merge-config` file: which grammar kinds collapse into a
/// shared Rust `enum`, which are merely renamed, and which are explicitly
/// left as the default baseline struct.
///
/// Deserialized directly from TOML via `serde`, so field names here are the
/// config file's own key names, not chosen for Rust-side convenience.
#[derive(serde::Deserialize)]
pub struct MergeConfig {
    /// Kinds to collapse into one Rust `enum`, one entry per resulting enum.
    #[serde(default)]
    pub merge: Vec<MergeEntry>,
    /// Kinds to rename to a different Rust type name, unchanged otherwise.
    #[serde(default)]
    pub passthrough: Vec<PassthroughEntry>,
    /// Kinds to explicitly leave as the default baseline struct. May be the
    /// single wildcard entry `"*"` instead of a list of kind names.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// One `merge` entry: several source kinds collapsing into one Rust `enum`
/// named `target`, each kind becoming its own variant.
#[derive(serde::Deserialize)]
pub struct MergeEntry {
    /// The generated enum's Rust type name.
    pub target: String,
    /// The grammar kinds becoming this enum's variants.
    pub from: Vec<String>,
}

/// One `passthrough` entry: a single kind emitted under a different Rust
/// type name, its own derived fields unchanged.
#[derive(serde::Deserialize)]
pub struct PassthroughEntry {
    /// The grammar kind being renamed.
    pub kind: String,
    /// The Rust type name to emit it under.
    pub target: String,
}

/// Parses `source` — the contents of a `--merge-config` TOML file — into a
/// [`MergeConfig`], without checking it against any particular grammar (see
/// [`check_merge_config`] for that).
pub fn parse_merge_config(source: &str) -> Result<MergeConfig, String> {
    toml::from_str(source).map_err(|e| format!("failed to parse merge config: {e}"))
}

/// Validates `config` against `grammar`.
///
/// Every kind named in `merge[].from`, `passthrough[].kind`, and `ignore`
/// (aside from the literal wildcard `"*"`) must be one of `grammar`'s own
/// [`visible_kinds`] — a config written against a different grammar, or
/// with a typo'd kind name, is rejected outright rather than silently
/// matching nothing.
pub fn check_merge_config(grammar: &Grammar, config: &MergeConfig) -> Result<(), String> {
    let visible = visible_kinds(grammar);

    for entry in &config.merge {
        for kind in &entry.from {
            if !visible.contains(kind) {
                return Err(format!(
                    "merge target '{}' names '{kind}', which is not a visible kind of this \
                     grammar; check for a typo or a kind that no longer exists",
                    entry.target
                ));
            }
        }
    }

    for entry in &config.passthrough {
        if !visible.contains(&entry.kind) {
            return Err(format!(
                "passthrough entry names '{}', which is not a visible kind of this grammar; \
                 check for a typo or a kind that no longer exists",
                entry.kind
            ));
        }
    }

    if config.ignore.iter().any(|kind| kind == "*") && config.ignore.len() > 1 {
        return Err(
            "ignore mixes the wildcard '*' with named entries; '*' already means \"every \
             kind not otherwise claimed\", so listing specific kinds alongside it is almost \
             certainly a misunderstanding — use '*' alone, or drop it and list kinds \
             explicitly"
                .to_string(),
        );
    }

    for kind in &config.ignore {
        if kind != "*" && !visible.contains(kind) {
            return Err(format!(
                "ignore entry names '{kind}', which is not a visible kind of this grammar; \
                 check for a typo or a kind that no longer exists"
            ));
        }
    }

    let mut claims: Vec<(String, String)> = Vec::new();
    for entry in &config.merge {
        for kind in &entry.from {
            claims.push((format!("merge target '{}'", entry.target), kind.clone()));
        }
    }
    for entry in &config.passthrough {
        claims.push((
            format!("passthrough entry '{}'", entry.kind),
            entry.kind.clone(),
        ));
    }
    for kind in &config.ignore {
        if kind != "*" {
            claims.push(("ignore".to_string(), kind.clone()));
        }
    }

    let items = claims
        .iter()
        .map(|(label, kind)| (label.as_str(), kind.clone()));
    if let Some((other, this, kind)) = find_first_name_collision(items) {
        return Err(format!(
            "kind '{kind}' is claimed by more than one entry ({other} and {this}); a kind \
             may appear in at most one of merge/passthrough/ignore"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::GrammarNode::TerminalLiteral;
    use crate::dom::test_utils::p;

    /// A grammar with a handful of visible kinds to validate merge configs
    /// against: two loop-like statements, one that stays alone, and a
    /// comment kind.
    fn grammar() -> Grammar {
        Grammar::from_rules([
            p("for_statement", TerminalLiteral("'f'".into())),
            p("while_statement", TerminalLiteral("'w'".into())),
            p("repeat_statement", TerminalLiteral("'r'".into())),
            p("comment", TerminalLiteral("'c'".into())),
        ])
    }

    // ── parse_merge_config ──────────────────────────────────────────────

    /// All three sections parse into their corresponding fields.
    #[test]
    fn parse_merge_config_parses_all_three_sections() {
        let source = r#"
            ignore = ["repeat_statement"]

            [[merge]]
            target = "Loop"
            from = ["for_statement", "while_statement"]

            [[passthrough]]
            kind = "comment"
            target = "Comment"
        "#;
        let config = parse_merge_config(source).unwrap();
        assert_eq!(config.merge.len(), 1);
        assert_eq!(config.merge[0].target, "Loop");
        assert_eq!(
            config.merge[0].from,
            vec!["for_statement".to_string(), "while_statement".to_string()]
        );
        assert_eq!(config.passthrough.len(), 1);
        assert_eq!(config.passthrough[0].kind, "comment");
        assert_eq!(config.passthrough[0].target, "Comment");
        assert_eq!(config.ignore, vec!["repeat_statement".to_string()]);
    }

    /// A config omitting every section entirely still parses, defaulting
    /// each `Vec` field to empty rather than failing — the reason every
    /// field carries `#[serde(default)]`.
    #[test]
    fn parse_merge_config_defaults_missing_sections_to_empty() {
        let config = parse_merge_config("").unwrap();
        assert!(config.merge.is_empty());
        assert!(config.passthrough.is_empty());
        assert!(config.ignore.is_empty());
    }

    /// Malformed TOML is a hard error, not a silently-empty config.
    #[test]
    fn parse_merge_config_rejects_malformed_toml() {
        assert!(parse_merge_config("not = [valid").is_err());
    }

    // ── check_merge_config ──────────────────────────────────────────────

    /// A config naming only real kinds, with no overlap between sections,
    /// passes.
    #[test]
    fn check_merge_config_ok_for_valid_config() {
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![PassthroughEntry {
                kind: "comment".to_string(),
                target: "Comment".to_string(),
            }],
            ignore: vec!["repeat_statement".to_string()],
        };
        assert!(check_merge_config(&grammar(), &config).is_ok());
    }

    /// An unknown kind in a `merge` entry's `from` list is rejected, naming it.
    #[test]
    fn check_merge_config_detects_unknown_kind_in_merge_from() {
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["bogus_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains("'bogus_statement'"),
            "error must name 'bogus_statement'; got: {err}"
        );
    }

    /// An unknown kind in a `passthrough` entry is rejected, naming it.
    #[test]
    fn check_merge_config_detects_unknown_kind_in_passthrough() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![PassthroughEntry {
                kind: "bogus_kind".to_string(),
                target: "Bogus".to_string(),
            }],
            ignore: vec![],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains("'bogus_kind'"),
            "error must name 'bogus_kind'; got: {err}"
        );
    }

    /// An unknown kind in `ignore` is rejected, naming it.
    #[test]
    fn check_merge_config_detects_unknown_kind_in_ignore() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![],
            ignore: vec!["bogus_kind".to_string()],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains("'bogus_kind'"),
            "error must name 'bogus_kind'; got: {err}"
        );
    }

    /// `ignore = ["*"]` alone is the documented catch-all and passes.
    #[test]
    fn check_merge_config_ignore_wildcard_alone_is_ok() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![],
            ignore: vec!["*".to_string()],
        };
        assert!(check_merge_config(&grammar(), &config).is_ok());
    }

    /// Mixing the `"*"` wildcard with a named `ignore` entry is rejected.
    #[test]
    fn check_merge_config_ignore_wildcard_mixed_with_named_entry_is_rejected() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![],
            ignore: vec!["comment".to_string(), "*".to_string()],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains('*'),
            "error must mention the wildcard; got: {err}"
        );
    }

    /// A kind claimed by two different entries — here, both a `merge` and a
    /// `passthrough` entry — is rejected, naming the kind and both places
    /// that claim it.
    #[test]
    fn check_merge_config_detects_kind_claimed_by_two_entries() {
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string()],
            }],
            passthrough: vec![PassthroughEntry {
                kind: "for_statement".to_string(),
                target: "ForStatement".to_string(),
            }],
            ignore: vec![],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains("'for_statement'"),
            "error must name 'for_statement'; got: {err}"
        );
        assert!(
            err.contains("merge target 'Loop'"),
            "error must name the merge entry; got: {err}"
        );
        assert!(
            err.contains("passthrough entry 'for_statement'"),
            "error must name the passthrough entry; got: {err}"
        );
    }
}
