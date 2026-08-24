use indexmap::IndexSet;

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

/// Whether `name` is safe to emit verbatim as a Rust `struct`/`enum` name: a
/// bare identifier round-trips through `syn::parse_str::<syn::Ident>`
/// unchanged iff it's non-empty, uses only identifier characters, and isn't
/// a Rust keyword — the same technique `rust.rs`'s `rust_field_ident` uses
/// for field labels, just without that function's raw-identifier fallback
/// (not applicable here — see [`check_merge_config`]'s own doc comment for
/// why).
fn is_valid_rust_type_name(name: &str) -> bool {
    syn::parse_str::<syn::Ident>(name).is_ok()
}

/// Parses `source` — the contents of a `--merge-config` TOML file — into a
/// [`MergeConfig`], without checking it against any particular grammar (see
/// [`check_merge_config`] for that).
pub fn parse_merge_config(source: &str) -> Result<MergeConfig, String> {
    toml::from_str(source).map_err(|e| format!("failed to parse merge config: {e}"))
}

/// Validates `config` against `grammar`.
///
/// Every `merge`/`passthrough` entry's own `target` must be a syntactically
/// valid, non-keyword Rust identifier ([`is_valid_rust_type_name`]) — it's
/// emitted verbatim as a `struct`/`enum` name with no escaping available
/// (unlike a field label, a `target` is referenced unqualified at many
/// codegen sites, so raw-identifier escaping isn't a good option here),
/// so an empty string, a name with invalid identifier characters, or a
/// Rust keyword is rejected up front rather than silently producing
/// non-compiling Rust (#361).
///
/// Every kind named in `merge[].from`, `passthrough[].kind`, and `ignore`
/// (aside from the literal wildcard `"*"`) must be one of `grammar`'s own
/// [`visible_kinds`] — a config written against a different grammar, or
/// with a typo'd kind name, is rejected outright rather than silently
/// matching nothing.
///
/// A kind repeated within one `merge` entry's own `from` list is caught and
/// reported by name before the general cross-entry collision check below
/// gets to it — that check's error would otherwise be self-referential and
/// unhelpful for this particular case ("claimed by more than one entry
/// (merge target 'Loop' and merge target 'Loop')", #361).
pub fn check_merge_config(grammar: &Grammar, config: &MergeConfig) -> Result<(), String> {
    for entry in &config.merge {
        if !is_valid_rust_type_name(&entry.target) {
            return Err(format!(
                "merge entry's target '{}' is not a valid Rust identifier; choose a target \
                 that isn't empty, uses only Rust identifier characters, and isn't a Rust \
                 keyword",
                entry.target
            ));
        }
    }
    for entry in &config.passthrough {
        if !is_valid_rust_type_name(&entry.target) {
            return Err(format!(
                "passthrough entry '{}''s target '{}' is not a valid Rust identifier; choose a \
                 target that isn't empty, uses only Rust identifier characters, and isn't a \
                 Rust keyword",
                entry.kind, entry.target
            ));
        }
    }

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

    for entry in &config.merge {
        let mut seen: IndexSet<&str> = IndexSet::new();
        for kind in &entry.from {
            if !seen.insert(kind.as_str()) {
                return Err(format!(
                    "kind '{kind}' is listed twice in merge target '{}''s own `from` list",
                    entry.target
                ));
            }
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

/// Visible kinds not covered by any `merge`/`passthrough`/`ignore` entry.
///
/// Empty whenever `config.ignore` contains the `"*"` wildcard, by
/// definition — a config using it has opted out of coverage tracking.
pub fn uncovered_kinds(grammar: &Grammar, config: &MergeConfig) -> Vec<String> {
    if config.ignore.iter().any(|kind| kind == "*") {
        return Vec::new();
    }

    let mut covered: IndexSet<&str> = IndexSet::new();
    for entry in &config.merge {
        covered.extend(entry.from.iter().map(String::as_str));
    }
    for entry in &config.passthrough {
        covered.insert(&entry.kind);
    }
    covered.extend(config.ignore.iter().map(String::as_str));

    let mut uncovered = Vec::new();
    for kind in visible_kinds(grammar) {
        if !covered.contains(kind.as_str()) {
            uncovered.push(kind);
        }
    }
    uncovered
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

    /// Target names that fail `syn::parse_str::<syn::Ident>`: a Rust
    /// keyword, a string with an invalid identifier character, and an
    /// empty string — the three cases named in #361.
    const INVALID_TARGET_NAMES: [&str; 3] = ["loop", "my-loop", ""];

    /// A `merge` entry whose `target` isn't a valid Rust identifier is
    /// rejected, naming that target, for each of #361's three cases.
    #[test]
    fn check_merge_config_rejects_invalid_merge_target() {
        for target in INVALID_TARGET_NAMES {
            let config = MergeConfig {
                merge: vec![MergeEntry {
                    target: target.to_string(),
                    from: vec!["for_statement".to_string()],
                }],
                passthrough: vec![],
                ignore: vec![],
            };
            let err = check_merge_config(&grammar(), &config).unwrap_err();
            assert!(
                err.contains(&format!("'{target}'")),
                "target '{target}' must be rejected with an error naming it; got: {err}"
            );
        }
    }

    /// A `passthrough` entry whose `target` isn't a valid Rust identifier is
    /// rejected, naming that target, for each of #361's three cases.
    #[test]
    fn check_merge_config_rejects_invalid_passthrough_target() {
        for target in INVALID_TARGET_NAMES {
            let config = MergeConfig {
                merge: vec![],
                passthrough: vec![PassthroughEntry {
                    kind: "comment".to_string(),
                    target: target.to_string(),
                }],
                ignore: vec![],
            };
            let err = check_merge_config(&grammar(), &config).unwrap_err();
            assert!(
                err.contains(&format!("'{target}'")),
                "target '{target}' must be rejected with an error naming it; got: {err}"
            );
        }
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

    /// A kind repeated within one `merge` entry's own `from` list gets a
    /// distinct, non-self-referential error naming the entry and the
    /// repeated kind, not the generic cross-entry collision message (#361).
    #[test]
    fn check_merge_config_detects_same_entry_duplicate_kind() {
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "for_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let err = check_merge_config(&grammar(), &config).unwrap_err();
        assert!(
            err.contains("'for_statement'"),
            "error must name 'for_statement'; got: {err}"
        );
        assert!(
            err.contains("listed twice"),
            "error must describe the same-entry duplicate distinctly, not as a cross-entry \
             collision; got: {err}"
        );
        assert!(
            err.contains("merge target 'Loop'"),
            "error must name the merge entry; got: {err}"
        );
    }

    // ── uncovered_kinds ─────────────────────────────────────────────────

    /// A kind named by neither `merge`, `passthrough`, nor `ignore` is
    /// reported, and only that kind.
    #[test]
    fn uncovered_kinds_reports_kind_missing_from_config() {
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![PassthroughEntry {
                kind: "comment".to_string(),
                target: "Comment".to_string(),
            }],
            ignore: vec![],
        };
        assert_eq!(
            uncovered_kinds(&grammar(), &config),
            vec!["repeat_statement".to_string()]
        );
    }

    /// `ignore = ["*"]` silences the report entirely, even against an
    /// otherwise completely untriaged grammar.
    #[test]
    fn uncovered_kinds_ignore_wildcard_silences_everything() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![],
            ignore: vec!["*".to_string()],
        };
        assert!(uncovered_kinds(&grammar(), &config).is_empty());
    }

    /// A config that explicitly names every visible kind across
    /// merge/passthrough/ignore (no wildcard) reports nothing.
    #[test]
    fn uncovered_kinds_fully_covered_config_reports_nothing() {
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
        assert!(uncovered_kinds(&grammar(), &config).is_empty());
    }
}
