//! CLI tool for converting BNF grammars to tree-sitter `grammar.js` notation.

/// DOM types representing the BNF grammar as a Rust value tree.
mod dom;
/// Visitor functions that walk a tree-sitter parse tree and build the DOM.
mod visitors;

use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;

use crate::dom::{ParseError, Scaffold};
use crate::visitors::visit_grammar;

/// Command-line arguments for the `ts-bnf-tool` binary.
#[derive(Parser, Debug)]
#[command(about = "Convert BNF grammars to tree-sitter notation")]
struct Args {
    /// Input BNF file
    filename: String,

    /// Output rule bodies only, without grammar.js boilerplate
    #[arg(long, conflicts_with = "generate")]
    rules_only: bool,

    /// Generate a full tree-sitter project in the output directory
    #[arg(long)]
    generate: bool,

    /// Grammar name (default: filename stem)
    #[arg(long)]
    name: Option<String>,

    /// Output directory for --generate (default: ./<name>)
    #[arg(long, requires = "generate")]
    output_dir: Option<String>,
}

/// Returns the output directory: the explicit path if given, or `<grammar_name>` as a default.
fn resolve_output_dir(output_dir: Option<&str>, grammar_name: &str) -> PathBuf {
    output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(grammar_name))
}

/// An error produced when an external command (e.g. `tree-sitter generate`) fails.
#[derive(Debug)]
struct CommandError(String);

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CommandError {}

/// Writes `grammar.js` to the output directory and runs `tree-sitter generate` inside it.
fn run_generate(scaffold: &Scaffold<'_>, output_dir: Option<&str>) -> Result<(), Box<dyn Error>> {
    let dir = resolve_output_dir(output_dir, scaffold.name);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("grammar.js"), scaffold.to_string())?;
    let status = Command::new("tree-sitter")
        .arg("generate")
        .current_dir(&dir)
        .status()
        .map_err(|e| CommandError(format!("failed to run tree-sitter: {}", e)))?;
    if !status.success() {
        return Err(CommandError("tree-sitter generate failed".into()).into());
    }
    Ok(())
}

/// Returns the grammar name: the explicit override if provided, or the filename stem.
fn grammar_name(filename: &str, override_name: Option<&str>) -> String {
    override_name.map(str::to_string).unwrap_or_else(|| {
        Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("grammar")
            .to_string()
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut file = File::open(&args.filename)?;
    let mut source_code = String::new();
    file.read_to_string(&mut source_code)?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");

    let tree = parser
        .parse(&source_code, None)
        .ok_or(ParseError::ParseFailed)?;

    let root_node = tree.root_node();
    if root_node.has_error() {
        return Err(ParseError::SyntaxError.into());
    }

    let grammar = visit_grammar(&root_node, &source_code)?;

    let name = grammar_name(&args.filename, args.name.as_deref());

    if args.rules_only {
        println!("{}", grammar);
    } else if args.generate {
        let scaffold = Scaffold {
            grammar: &grammar,
            name: &name,
        };
        run_generate(&scaffold, args.output_dir.as_deref())?;
    } else {
        println!(
            "{}",
            Scaffold {
                grammar: &grammar,
                name: &name
            }
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(args)
    }

    #[test]
    fn generate_and_rules_only_conflict() {
        assert!(parse(&["ts-bnf-tool", "--generate", "--rules-only", "f.bnf"]).is_err());
    }

    #[test]
    fn output_dir_requires_generate() {
        assert!(parse(&["ts-bnf-tool", "--output-dir", "/tmp", "f.bnf"]).is_err());
    }

    #[test]
    fn generate_alone_is_valid() {
        let args = parse(&["ts-bnf-tool", "--generate", "f.bnf"]).unwrap();
        assert!(args.generate);
        assert!(!args.rules_only);
        assert!(args.output_dir.is_none());
    }

    #[test]
    fn generate_with_output_dir_is_valid() {
        let args = parse(&["ts-bnf-tool", "--generate", "--output-dir", "/tmp", "f.bnf"]).unwrap();
        assert!(args.generate);
        assert_eq!(args.output_dir.as_deref(), Some("/tmp"));
    }

    #[test]
    fn rules_only_alone_is_valid() {
        let args = parse(&["ts-bnf-tool", "--rules-only", "f.bnf"]).unwrap();
        assert!(args.rules_only);
        assert!(!args.generate);
    }

    #[test]
    fn help_flag_exits_successfully_and_lists_all_flags() {
        let err = Args::try_parse_from(["ts-bnf-tool", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = err.to_string();
        assert!(help.contains("--rules-only"));
        assert!(help.contains("--generate"));
        assert!(help.contains("--name"));
        assert!(help.contains("--output-dir"));
    }

    #[test]
    fn resolve_output_dir_uses_explicit_path() {
        assert_eq!(
            resolve_output_dir(Some("/my/dir"), "grammar"),
            PathBuf::from("/my/dir")
        );
    }

    #[test]
    fn resolve_output_dir_defaults_to_grammar_name() {
        assert_eq!(
            resolve_output_dir(None, "mygrammar"),
            PathBuf::from("mygrammar")
        );
    }
}
