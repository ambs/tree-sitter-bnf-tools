use std::error::Error;
use std::fs;
use std::path::Path;

use super::super::ast::merge::MergeConfig;

/// Default configuration file for a ts-bnf-tool project
///
/// `grammar` and `name` are language-agnostic. `ast_types` and
/// `merge_config`, though, are concepts specific to the Rust AST-types
/// emitter (`dom::ast::rust`) — see the same note on
/// `main.rs`'s `resolve_scaffold_target`. A second target language will need
/// a different shape for whatever it records here, not more fields bolted
/// on alongside these.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ScaffoldConfig {
    /// The name of the main bnf grammar file
    pub grammar: String,
    /// The name of the project (usually, the grammar file without the .bnf extension)
    pub name: String,
    /// If ast types should be generated
    #[serde(default)]
    pub ast_types: bool,
    /// The merge config data
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub merge_config: Option<MergeConfig>,
}

/// Parses the contents of a scaffolded crate's `ts-bnf-tool.toml`.
pub fn parse_scaffold_config(source: &str) -> Result<ScaffoldConfig, String> {
    toml::from_str(source).map_err(|e| format!("failed to parse ts-bnf-tool.toml: {e}"))
}

/// Renders `config` as the `ts-bnf-tool.toml` file contents.
pub fn render_scaffold_config(config: &ScaffoldConfig) -> String {
    toml::to_string_pretty(config).expect("ScaffoldConfig always serializes")
}

/// Rewrites `dir`'s `ts-bnf-tool.toml` in place so its recorded
/// `name`/`ast_types`/`merge_config` match `effective`, doing nothing when
/// they already do.
///
/// `ts-bnf-tool.toml` is written once with `preserve_existing: true` (like
/// `Cargo.toml`/`lib.rs`) precisely so it stays hand-editable — a rerun's
/// ordinary file-write pass therefore leaves an existing one completely
/// untouched, even when this run's effective settings (a newly added
/// `--ast-types`/`--merge-config`, or a `--name` override) have moved on.
/// This is the patch-in-place step that catches that up afterwards, the
/// same role `ensure_lib_rs_declares_ast_module` plays for `lib.rs`: only
/// the fields that actually changed are rewritten, and only when something
/// did — an unchanged config (including one with a hand-tweaked
/// `[merge_config]` that matches what's already on disk) is left byte-for-
/// byte alone, comments and formatting included. `grammar` is never part of
/// the comparison or the rewrite: it's the crate's identity, guarded
/// separately by the mismatch refusal in `resolve_file_scaffold_target`,
/// not something this function ever changes.
pub fn ensure_scaffold_config_up_to_date(
    dir: &Path,
    effective: &ScaffoldConfig,
) -> Result<(), Box<dyn Error>> {
    let path = dir.join("ts-bnf-tool.toml");
    let existing = parse_scaffold_config(&fs::read_to_string(&path)?)?;
    if existing.name == effective.name
        && existing.ast_types == effective.ast_types
        && existing.merge_config == effective.merge_config
    {
        return Ok(());
    }
    let patched = ScaffoldConfig {
        grammar: existing.grammar,
        name: effective.name.clone(),
        ast_types: effective.ast_types,
        merge_config: effective.merge_config.clone(),
    };
    fs::write(&path, render_scaffold_config(&patched))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An effective config identical to what's already on disk (the
    /// ordinary case: no CLI override this run) leaves the file untouched —
    /// checked by writing content `render_scaffold_config` would never
    /// itself produce (a comment, unusual spacing), so any rewrite at all
    /// would be caught.
    #[test]
    fn ensure_scaffold_config_up_to_date_leaves_unchanged_config_untouched() {
        let dir = std::env::temp_dir().join("ts_bnf_ensure_scaffold_config_unchanged_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ts-bnf-tool.toml");
        let hand_edited = "# hand-added comment\ngrammar = \"decls.bnf\"\nname = \"decls\"\n";
        fs::write(&path, hand_edited).unwrap();

        let effective = ScaffoldConfig {
            grammar: "decls.bnf".to_string(),
            name: "decls".to_string(),
            ast_types: false,
            merge_config: None,
        };
        ensure_scaffold_config_up_to_date(&dir, &effective).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), hand_edited);
    }

    /// A newly added `--ast-types` (recorded as `false` on disk, `true`
    /// this run) is patched in, while `grammar` — never part of the
    /// comparison — stays exactly what was already recorded even if the
    /// caller's `effective.grammar` were to disagree.
    #[test]
    fn ensure_scaffold_config_up_to_date_patches_changed_fields_only() {
        let dir = std::env::temp_dir().join("ts_bnf_ensure_scaffold_config_patch_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ts-bnf-tool.toml");
        fs::write(
            &path,
            render_scaffold_config(&ScaffoldConfig {
                grammar: "decls.bnf".to_string(),
                name: "decls".to_string(),
                ast_types: false,
                merge_config: None,
            }),
        )
        .unwrap();

        let effective = ScaffoldConfig {
            grammar: "ignored.bnf".to_string(),
            name: "decls".to_string(),
            ast_types: true,
            merge_config: None,
        };
        ensure_scaffold_config_up_to_date(&dir, &effective).unwrap();

        let patched = parse_scaffold_config(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(patched.grammar, "decls.bnf");
        assert!(patched.ast_types);
    }
}
