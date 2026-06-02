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
use ts_bnf_tool::dom::{Diagnostic, Grammar, ParseError, Scaffold, Severity};
use ts_bnf_tool::visitors::{visit_grammar, SourceFile};

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
        /// Skip static checks; suppress all warnings and convert unconditionally
        #[arg(long, short = 'n')]
        no_check: bool,
        /// Suppress the generated-file header comment at the top of the output
        #[arg(long)]
        no_header: bool,
        /// Treat any warning as a fatal error and exit non-zero (conflicts with --no-check)
        #[arg(long, conflicts_with = "no_check")]
        strict: bool,
    },
    /// Print FIRST sets for each rule in the grammar
    Firsts {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Skip static checks and suppress all warnings
        #[arg(long, short = 'n')]
        no_check: bool,
    },
    /// Run all static checks and exit non-zero on any issue (for CI)
    Check {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
    },
    /// Pretty-print a BNF file in canonical style.
    Format {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Overwrite the file in place (atomic write; cannot be used with `-`)
        #[arg(long, short = 'i', conflicts_with = "check")]
        in_place: bool,
        /// Exit 1 if the file is not already formatted (for CI); do not write output.
        /// When `--strip-comments` is active (the default), comments are excluded from
        /// the comparison so a correctly-formatted file with comments still passes.
        #[arg(long)]
        check: bool,
        /// Strip comments from the output (default behaviour; see issue #148).
        #[arg(long, overrides_with = "no_strip_comments")]
        strip_comments: bool,
        /// Preserve comments; overrides `--strip-comments`.
        /// Reserved for use once issue #148 (comment round-tripping) is implemented.
        #[arg(long, overrides_with = "strip_comments")]
        no_strip_comments: bool,
    },
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

/// Returns the source label for the generated-file header: `<stdin>` for `-`, otherwise the filename.
fn source_label(filename: &str) -> &str {
    if filename == "-" {
        "<stdin>"
    } else {
        filename
    }
}

/// Returns `true` if `name` is a valid JavaScript identifier (excluding `$`).
fn is_valid_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Returns a warning diagnostic if `name` is not a valid JavaScript identifier.
fn check_grammar_name(name: &str) -> Vec<Diagnostic> {
    if is_valid_js_identifier(name) {
        Vec::new()
    } else {
        vec![Diagnostic::warning(format!(
            "grammar name '{name}' is not a valid JavaScript identifier; use --name to override"
        ))]
    }
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

/// Parses `filename` into a grammar DOM.
///
/// When `run_checks` is `true`, the returned [`Vec<Diagnostic>`] contains all diagnostics
/// from cross-reference and static checks.  When `false`, checks are suppressed and the
/// returned vec is always empty.
fn parse_file(
    filename: &str,
    run_checks: bool,
) -> Result<(Grammar, Vec<Diagnostic>), Box<dyn Error>> {
    let source_code = load_grammar_source(filename)?;
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
    let ctx = SourceFile {
        source: &source_code,
        filename,
    };
    let (grammar, diagnostics) = visit_grammar(&root_node, &ctx)?;
    if run_checks {
        Ok((grammar, diagnostics))
    } else {
        Ok((grammar, Vec::new()))
    }
}

/// Formats a single [`FirstTerminal`] for display: its raw string value as stored.
fn display_terminal<'a>(t: &'a FirstTerminal<'a>) -> &'a str {
    match t {
        FirstTerminal::Literal(s) | FirstTerminal::Pattern(s) => s,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match cli.command {
        Subcommands::Convert {
            filename,
            rules_only,
            generate,
            name,
            output_dir,
            no_check,
            no_header,
            strict,
        } => {
            let name = grammar_name(&filename, name.as_deref());
            let (grammar, mut diagnostics) = parse_file(&filename, !no_check)?;
            if !no_check {
                diagnostics.extend(check_grammar_name(&name));
            }
            for d in &diagnostics {
                eprintln!("{d}");
            }
            if diagnostics.iter().any(|d| d.severity == Severity::Error) {
                return Err(
                    "grammar has errors; conversion aborted (use --no-check to convert anyway)"
                        .into(),
                );
            }
            let had_warnings = diagnostics.iter().any(|d| d.severity == Severity::Warning);
            let source = source_label(&filename);

            if rules_only {
                println!("{}", grammar);
            } else if generate {
                let scaffold = Scaffold {
                    grammar: &grammar,
                    name: &name,
                    source,
                    no_header,
                };
                run_generate(&scaffold, output_dir.as_deref())?;
            } else {
                println!(
                    "{}",
                    Scaffold {
                        grammar: &grammar,
                        name: &name,
                        source,
                        no_header,
                    }
                );
            }

            if strict && had_warnings {
                std::process::exit(1);
            }
        }

        Subcommands::Firsts { filename, no_check } => {
            let (grammar, diagnostics) = parse_file(&filename, !no_check)?;
            for d in &diagnostics {
                eprintln!("{d}");
            }
            let sets = first_sets(&grammar);

            let mut rules: Vec<&str> = sets.keys().copied().collect();
            rules.sort_unstable();

            for rule in rules {
                let mut terminals: Vec<&str> = sets[rule].iter().map(display_terminal).collect();
                terminals.sort_unstable();
                println!("{}: {}", rule, terminals.join(", "));
            }
        }

        Subcommands::Format {
            filename,
            in_place,
            check,
            strip_comments,
            no_strip_comments,
        } => {
            if in_place && filename == "-" {
                return Err("--in-place cannot be used with stdin".into());
            }
            let do_strip = strip_comments || !no_strip_comments;
            let (grammar, _) = parse_file(&filename, false)?;
            let formatted = ts_bnf_tool::dom::format_grammar(&grammar);

            if check {
                let original = load_grammar_source(&filename)?;
                let cmp = if do_strip {
                    ts_bnf_tool::util::strip_comments_from_source(&original)
                } else {
                    original
                };
                if cmp != formatted {
                    std::process::exit(1);
                }
            } else if in_place {
                let tmp = format!("{}.tmp", filename);
                fs::write(&tmp, &formatted)?;
                fs::rename(&tmp, &filename)?;
            } else {
                print!("{}", formatted);
            }
        }

        Subcommands::Check { filename } => {
            let (_, diagnostics) = parse_file(&filename, true)?;
            for d in &diagnostics {
                eprintln!("{d}");
            }
            let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);
            let has_warnings = diagnostics.iter().any(|d| d.severity == Severity::Warning);
            if has_errors {
                std::process::exit(2);
            } else if has_warnings {
                std::process::exit(1);
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

    fn parse_format(args: &[&str]) -> Result<Cli, clap::Error> {
        let full: Vec<&str> = std::iter::once("ts-bnf-tool")
            .chain(std::iter::once("format"))
            .chain(args.iter().copied())
            .collect();
        Cli::try_parse_from(full)
    }

    #[test]
    fn format_strip_comments_default_is_true() {
        let cli = parse_format(&["f.bnf"]).unwrap();
        let Subcommands::Format {
            strip_comments,
            no_strip_comments,
            ..
        } = cli.command
        else {
            panic!("wrong subcommand");
        };
        assert!(strip_comments || !no_strip_comments);
    }

    #[test]
    fn format_no_strip_comments_overrides_default() {
        let cli = parse_format(&["--no-strip-comments", "f.bnf"]).unwrap();
        let Subcommands::Format {
            no_strip_comments, ..
        } = cli.command
        else {
            panic!("wrong subcommand");
        };
        assert!(no_strip_comments);
    }

    #[test]
    fn format_strip_comments_and_no_strip_comments_last_wins() {
        let cli = parse_format(&["--strip-comments", "--no-strip-comments", "f.bnf"]).unwrap();
        let Subcommands::Format {
            strip_comments,
            no_strip_comments,
            ..
        } = cli.command
        else {
            panic!("wrong subcommand");
        };
        // --no-strip-comments is last, so strip_comments is cleared by overrides_with
        assert!(!strip_comments && no_strip_comments);
    }

    #[test]
    fn format_in_place_and_check_conflict() {
        assert!(parse_format(&["--in-place", "--check", "f.bnf"]).is_err());
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
    fn strict_and_no_check_conflict() {
        assert!(parse_convert(&["--strict", "--no-check", "f.bnf"]).is_err());
    }

    #[test]
    fn source_label_dash_is_stdin() {
        assert_eq!(source_label("-"), "<stdin>");
    }

    #[test]
    fn source_label_filename_passthrough() {
        assert_eq!(source_label("grammar.bnf"), "grammar.bnf");
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
    fn valid_js_identifier_plain() {
        assert!(is_valid_js_identifier("grammar"));
    }

    #[test]
    fn valid_js_identifier_underscore_start() {
        assert!(is_valid_js_identifier("_grammar"));
    }

    #[test]
    fn valid_js_identifier_with_digits() {
        assert!(is_valid_js_identifier("grammar2"));
    }

    #[test]
    fn invalid_js_identifier_hyphen() {
        assert!(!is_valid_js_identifier("my-grammar"));
    }

    #[test]
    fn invalid_js_identifier_leading_digit() {
        assert!(!is_valid_js_identifier("1grammar"));
    }

    #[test]
    fn invalid_js_identifier_empty() {
        assert!(!is_valid_js_identifier(""));
    }

    #[test]
    fn invalid_js_identifier_dot() {
        assert!(!is_valid_js_identifier("my.grammar"));
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
