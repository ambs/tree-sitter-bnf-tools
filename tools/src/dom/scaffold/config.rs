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
    /// If the generated ast types should build partial trees from parse trees
    /// containing syntax errors (`--partial-trees`). Only meaningful with
    /// `ast_types`; absent from configs written before the flag existed,
    /// which therefore keep building strict trees. Omitted from the written
    /// file when `false`, so a scaffold that never uses the flag produces
    /// exactly the config it always did.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub partial_trees: bool,
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
/// `name`/`ast_types`/`partial_trees`/`merge_config` match `effective`, doing
/// nothing when they already do.
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
        && existing.partial_trees == effective.partial_trees
        && existing.merge_config == effective.merge_config
    {
        return Ok(());
    }
    let patched = ScaffoldConfig {
        grammar: existing.grammar,
        name: effective.name.clone(),
        ast_types: effective.ast_types,
        partial_trees: effective.partial_trees,
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
            partial_trees: false,
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
                partial_trees: false,
                merge_config: None,
            }),
        )
        .unwrap();

        let effective = ScaffoldConfig {
            grammar: "ignored.bnf".to_string(),
            name: "decls".to_string(),
            ast_types: true,
            partial_trees: false,
            merge_config: None,
        };
        ensure_scaffold_config_up_to_date(&dir, &effective).unwrap();

        let patched = parse_scaffold_config(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(patched.grammar, "decls.bnf");
        assert!(patched.ast_types);
    }

    /// A config written before `--partial-trees` existed has no such key and
    /// must keep meaning strict trees.
    #[test]
    fn parse_scaffold_config_without_partial_trees_defaults_to_false() {
        let config =
            parse_scaffold_config("grammar = \"decls.bnf\"\nname = \"decls\"\nast_types = true\n")
                .unwrap();
        assert!(!config.partial_trees);
    }

    /// `partial_trees` is only written when set, so a scaffold that never
    /// uses the flag produces exactly the config it always did; when set it
    /// round-trips.
    #[test]
    fn render_scaffold_config_writes_partial_trees_only_when_set() {
        let mut config = ScaffoldConfig {
            grammar: "decls.bnf".to_string(),
            name: "decls".to_string(),
            ast_types: true,
            partial_trees: false,
            merge_config: None,
        };
        assert!(!render_scaffold_config(&config).contains("partial_trees"));

        config.partial_trees = true;
        let rendered = render_scaffold_config(&config);
        assert!(rendered.contains("partial_trees = true"));
        assert!(parse_scaffold_config(&rendered).unwrap().partial_trees);
    }

    /// Switching `--partial-trees` on or off is patched into an existing
    /// config like any other recorded setting.
    #[test]
    fn ensure_scaffold_config_up_to_date_patches_partial_trees() {
        let dir = std::env::temp_dir().join("ts_bnf_ensure_scaffold_config_partial_trees_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ts-bnf-tool.toml");
        let mut config = ScaffoldConfig {
            grammar: "decls.bnf".to_string(),
            name: "decls".to_string(),
            ast_types: true,
            partial_trees: false,
            merge_config: None,
        };
        fs::write(&path, render_scaffold_config(&config)).unwrap();

        config.partial_trees = true;
        ensure_scaffold_config_up_to_date(&dir, &config).unwrap();
        assert!(
            parse_scaffold_config(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .partial_trees
        );

        config.partial_trees = false;
        ensure_scaffold_config_up_to_date(&dir, &config).unwrap();
        assert!(
            !parse_scaffold_config(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .partial_trees
        );
    }
}
