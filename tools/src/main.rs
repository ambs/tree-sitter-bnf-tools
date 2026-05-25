//! CLI tool for converting BNF grammars to tree-sitter `grammar.js` notation.

use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};

use ts_bnf_tool::dom::analysis::{first_sets, FirstTerminal};
use ts_bnf_tool::dom::{ParseError, Scaffold};
use ts_bnf_tool::visitors::visit_grammar;

/// Top-level CLI for `ts-bnf-tool`.
#[derive(Parser, Debug)]
#[command(about = "BNF grammar analysis and conversion tool")]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Subcommands,
}

/// The available subcommands.
#[derive(Subcommand, Debug)]
enum Subcommands {
    /// Convert BNF grammar to tree-sitter grammar.js notation (default)
    Convert {
        /// Input BNF file, or `-` to read from stdin
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
    },
    /// Print FIRST sets for each rule in the grammar
    Firsts {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
    },
}

/// Known subcommand names; used by the default-injection logic in `main`.
const SUBCOMMANDS: &[&str] = &["convert", "firsts"];

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
/// Stdin (`-`) has no stem, so it defaults to `"grammar"`.
fn grammar_name(filename: &str, override_name: Option<&str>) -> String {
    override_name.map(str::to_string).unwrap_or_else(|| {
        if filename == "-" {
            return "grammar".to_string();
        }
        Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("grammar")
            .to_string()
    })
}

/// Reads the BNF source from `filename`, parsing it into a grammar.
fn load_grammar_source(filename: &str) -> Result<String, Box<dyn Error>> {
    let mut source = String::new();
    if filename == "-" {
        io::stdin().read_to_string(&mut source)?;
    } else {
        File::open(filename)?.read_to_string(&mut source)?;
    }
    Ok(source)
}

/// Formats a single [`FirstTerminal`] for display: its raw string value as stored.
fn display_terminal<'a>(t: &'a FirstTerminal<'a>) -> &'a str {
    match t {
        FirstTerminal::Literal(s) | FirstTerminal::Pattern(s) => s,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Inject "convert" when the first argument is not a known subcommand or a
    // help/version flag, so that `ts-bnf-tool <file>` keeps working.
    let raw: Vec<String> = std::env::args().collect();
    let args: Vec<String> = if raw.len() >= 2
        && !SUBCOMMANDS.contains(&raw[1].as_str())
        && raw[1] != "--help"
        && raw[1] != "-h"
        && raw[1] != "--version"
        && raw[1] != "-V"
    {
        let mut v = raw;
        v.insert(1, "convert".to_string());
        v
    } else {
        raw
    };

    let cli = Cli::parse_from(args);

    match cli.command {
        Subcommands::Convert {
            filename,
            rules_only,
            generate,
            name,
            output_dir,
        } => {
            let source_code = load_grammar_source(&filename)?;

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
            let name = grammar_name(&filename, name.as_deref());

            if rules_only {
                println!("{}", grammar);
            } else if generate {
                let scaffold = Scaffold {
                    grammar: &grammar,
                    name: &name,
                };
                run_generate(&scaffold, output_dir.as_deref())?;
            } else {
                println!(
                    "{}",
                    Scaffold {
                        grammar: &grammar,
                        name: &name
                    }
                );
            }
        }

        Subcommands::Firsts { filename } => {
            let source_code = load_grammar_source(&filename)?;

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
            let sets = first_sets(&grammar);

            let mut rules: Vec<&str> = sets.keys().copied().collect();
            rules.sort_unstable();

            for rule in rules {
                let mut terminals: Vec<&str> = sets[rule].iter().map(display_terminal).collect();
                terminals.sort_unstable();
                println!("{}: {}", rule, terminals.join(", "));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_convert(args: &[&str]) -> Result<Cli, clap::Error> {
        // Prepend "convert" so tests exercise the subcommand directly.
        let full: Vec<&str> = std::iter::once("ts-bnf-tool")
            .chain(std::iter::once("convert"))
            .chain(args.iter().copied())
            .collect();
        Cli::try_parse_from(full)
    }

    fn convert_fields(cli: Cli) -> (String, bool, bool, Option<String>, Option<String>) {
        match cli.command {
            Subcommands::Convert {
                filename,
                rules_only,
                generate,
                name,
                output_dir,
            } => (filename, rules_only, generate, name, output_dir),
            _ => panic!("expected Convert"),
        }
    }

    #[test]
    fn generate_and_rules_only_conflict() {
        assert!(parse_convert(&["--generate", "--rules-only", "f.bnf"]).is_err());
    }

    #[test]
    fn output_dir_requires_generate() {
        assert!(parse_convert(&["--output-dir", "/tmp", "f.bnf"]).is_err());
    }

    #[test]
    fn generate_alone_is_valid() {
        let (_, rules_only, generate, _, output_dir) =
            convert_fields(parse_convert(&["--generate", "f.bnf"]).unwrap());
        assert!(generate);
        assert!(!rules_only);
        assert!(output_dir.is_none());
    }

    #[test]
    fn generate_with_output_dir_is_valid() {
        let (_, _, generate, _, output_dir) = convert_fields(
            parse_convert(&["--generate", "--output-dir", "/tmp", "f.bnf"]).unwrap(),
        );
        assert!(generate);
        assert_eq!(output_dir.as_deref(), Some("/tmp"));
    }

    #[test]
    fn rules_only_alone_is_valid() {
        let (_, rules_only, generate, _, _) =
            convert_fields(parse_convert(&["--rules-only", "f.bnf"]).unwrap());
        assert!(rules_only);
        assert!(!generate);
    }

    #[test]
    fn help_flag_lists_subcommands() {
        let err = Cli::try_parse_from(["ts-bnf-tool", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let help = err.to_string();
        assert!(help.contains("convert"));
        assert!(help.contains("firsts"));
    }

    #[test]
    fn stdin_dash_is_valid_filename() {
        let (filename, ..) = convert_fields(parse_convert(&["-"]).unwrap());
        assert_eq!(filename, "-");
    }

    #[test]
    fn grammar_name_stdin_defaults_to_grammar() {
        assert_eq!(grammar_name("-", None), "grammar");
    }

    #[test]
    fn grammar_name_stdin_respects_override() {
        assert_eq!(grammar_name("-", Some("mygrammar")), "mygrammar");
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

    #[test]
    fn firsts_subcommand_parses_filename() {
        let cli = Cli::try_parse_from(["ts-bnf-tool", "firsts", "grammar.bnf"]).unwrap();
        match cli.command {
            Subcommands::Firsts { filename } => assert_eq!(filename, "grammar.bnf"),
            _ => panic!("expected Firsts"),
        }
    }

    #[test]
    fn default_injection_inserts_convert() {
        // Simulate the injection logic: a bare filename → "convert" is prepended.
        let raw = vec!["ts-bnf-tool".to_string(), "grammar.bnf".to_string()];
        let injected: Vec<String> = if raw.len() >= 2
            && !SUBCOMMANDS.contains(&raw[1].as_str())
            && raw[1] != "--help"
            && raw[1] != "-h"
        {
            let mut v = raw;
            v.insert(1, "convert".to_string());
            v
        } else {
            raw
        };
        let cli = Cli::try_parse_from(injected).unwrap();
        assert!(matches!(cli.command, Subcommands::Convert { .. }));
    }

    #[test]
    fn default_injection_skips_help() {
        let raw = vec!["ts-bnf-tool".to_string(), "--help".to_string()];
        // --help is excluded from injection
        let should_inject = raw.len() >= 2
            && !SUBCOMMANDS.contains(&raw[1].as_str())
            && raw[1] != "--help"
            && raw[1] != "-h";
        assert!(!should_inject);
    }
}
