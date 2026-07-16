//! CLI tool for converting BNF grammars to tree-sitter `grammar.js` notation.

use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use indoc::formatdoc;

use ts_bnf_tool::dom::analysis::{FirstTerminal, first_sets};
use ts_bnf_tool::dom::rename_grammar;
use ts_bnf_tool::dom::summary::GrammarSummary;
use ts_bnf_tool::dom::{Diagnostic, Grammar, Highlights, ParseError, Scaffold, Severity};
use ts_bnf_tool::util::{syntax_error_diagnostics, to_camelcase};
use ts_bnf_tool::visitors::{SourceFile, visit_grammar};

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
        /// Emit output as JSON instead of plain text
        #[arg(long)]
        json: bool,
    },
    /// Run all static checks and exit non-zero on any issue (for CI)
    Check {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Emit diagnostics as JSON instead of plain text
        #[arg(long)]
        json: bool,
        /// Append a grammar metrics summary after diagnostics
        #[arg(long)]
        summary: bool,
    },
    /// Generate a skeleton highlights.scm from a BNF grammar
    Highlights {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Write output to this file instead of stdout
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Suppress `; TODO: @???` placeholder entries for unclassified rules
        #[arg(long)]
        no_todos: bool,
    },
    /// Rename a rule definition and all its references throughout the grammar
    Rename {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Current rule name
        from: String,
        /// New rule name
        to: String,
        /// Overwrite the file in place (atomic write; cannot be used with `-`)
        #[arg(long, short = 'i', conflicts_with = "output")]
        in_place: bool,
        /// Write output to this file instead of stdout (cannot be combined with `--in-place`)
        #[arg(long, short = 'o', conflicts_with = "in_place")]
        output: Option<String>,
    },
    /// Generate railroad / syntax diagrams from a BNF grammar.
    ///
    /// Produces SVG output directly from Rust — no external binary required
    /// (unlike `graph --format svg`, which shells out to `dot`).
    Railroad {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Write output to file instead of stdout (single-file mode)
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Emit one SVG file per rule; requires `--output-dir`
        #[arg(long, requires = "output_dir", conflicts_with = "rule")]
        split: bool,
        /// Directory for per-rule SVG files (used with `--split`)
        #[arg(long)]
        output_dir: Option<String>,
        /// Render only the named rule; incompatible with `--split`
        #[arg(long)]
        rule: Option<String>,
        /// Draw tree-sitter annotations (field names, token/token.immediate,
        /// alias names, precedence) as labeled boxes instead of rendering them
        /// transparently
        #[arg(long)]
        annotate: bool,
    },
    /// Emit a rule-dependency graph (DOT, Mermaid, or Graphviz-rendered image).
    Graph {
        /// Input BNF file, or `-` to read from stdin
        filename: String,
        /// Output format: dot (default), mermaid, svg, pdf, png
        #[arg(long, default_value = "dot")]
        format: String,
        /// Write output to this file instead of stdout
        #[arg(long, short = 'o')]
        output: Option<String>,
        /// Restrict graph to rules reachable from this rule
        #[arg(long)]
        start: Option<String>,
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

/// Writes `grammar.js` and a skeleton `queries/highlights.scm` to the output directory,
/// then runs `tree-sitter generate` inside it.
fn run_generate(scaffold: &Scaffold<'_>, output_dir: Option<&str>) -> Result<(), Box<dyn Error>> {
    let dir = resolve_output_dir(output_dir, scaffold.name);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("grammar.js"), scaffold.to_string())?;
    let queries_dir = dir.join("queries");
    fs::create_dir_all(&queries_dir)?;
    fs::write(
        queries_dir.join("highlights.scm"),
        Highlights {
            grammar: scaffold.grammar,
            no_todos: false,
        }
        .to_string(),
    )?;
    write_tree_sitter_json(&dir, scaffold.name)?;
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

/// Writes a minimal `tree-sitter.json` to `dir` if one does not already exist.
///
/// Satisfies tree-sitter ≥ 0.25's requirement for ABI 15 generation.
/// An existing file is never overwritten.
fn write_tree_sitter_json(dir: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    let path = dir.join("tree-sitter.json");
    if path.exists() {
        return Ok(());
    }
    let camel = to_camelcase(name);
    fs::write(
        &path,
        formatdoc! {r#"
            {{
              "grammars": [
                {{
                  "name": "{name}",
                  "camelcase": "{camel}",
                  "scope": "source.{name}",
                  "file-types": []
                }}
              ],
              "metadata": {{
                "version": "0.1.0",
                "license": "MIT"
              }}
            }}
        "#},
    )?;
    Ok(())
}

/// Renames rule `from` to `to` in the grammar at `filename` and writes the result.
///
/// Output destination, in priority order: `--in-place` rewrites `filename` atomically,
/// `--output <path>` writes to that path, and the default prints to stdout.
fn run_rename(
    filename: &str,
    from: &str,
    to: &str,
    in_place: bool,
    output: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    if in_place && filename == "-" {
        return Err("--in-place cannot be used with stdin".into());
    }
    let (mut grammar, _) = parse_file(filename, false)?;
    rename_grammar(&mut grammar, from, to)?;
    let formatted = ts_bnf_tool::dom::format_grammar(&grammar);
    if in_place {
        let tmp = format!("{}.tmp", filename);
        fs::write(&tmp, &formatted)?;
        fs::rename(&tmp, filename)?;
    } else if let Some(path) = output {
        fs::write(path, &formatted)?;
    } else {
        print!("{}", formatted);
    }
    Ok(())
}

/// Returns the source label for the generated-file header: `<stdin>` for `-`, otherwise the filename.
fn source_label(filename: &str) -> &str {
    if filename == "-" { "<stdin>" } else { filename }
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

    let path = if filename == "-" {
        None
    } else {
        std::path::Path::new(filename).canonicalize().ok()
    };

    let ctx = SourceFile {
        source: &source_code,
        filename,
        path,
    };

    if root_node.has_error() {
        let diagnostics = syntax_error_diagnostics(&root_node, &ctx);
        return Err(ParseError::SyntaxError(diagnostics).into());
    }
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

/// Parses the command line and dispatches to the selected subcommand.
///
/// Any error is propagated to [`main`], which prints its [`Display`] form.
fn run() -> Result<(), Box<dyn Error>> {
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

        Subcommands::Firsts {
            filename,
            no_check,
            json,
        } => {
            let (grammar, diagnostics) = parse_file(&filename, !no_check)?;
            for d in &diagnostics {
                eprintln!("{d}");
            }
            let sets = first_sets(&grammar);

            let mut rules: Vec<&str> = sets.keys().copied().collect();
            rules.sort_unstable();

            let sorted: std::collections::BTreeMap<&str, Vec<&str>> = rules
                .iter()
                .map(|rule| {
                    let mut terminals: Vec<&str> =
                        sets[rule].iter().map(display_terminal).collect();
                    terminals.sort_unstable();
                    (*rule, terminals)
                })
                .collect();

            if json {
                println!("{}", serde_json::to_string(&sorted)?);
            } else {
                for (rule, terminals) in &sorted {
                    println!("{}: {}", rule, terminals.join(", "));
                }
            }
        }

        Subcommands::Highlights {
            filename,
            output,
            no_todos,
        } => {
            let (grammar, _) = parse_file(&filename, false)?;
            let skeleton = Highlights {
                grammar: &grammar,
                no_todos,
            }
            .to_string();
            match output {
                Some(path) => fs::write(&path, &skeleton)?,
                None => print!("{}", skeleton),
            }
        }

        Subcommands::Railroad {
            filename,
            output,
            split,
            output_dir,
            rule,
            annotate,
        } => {
            let (grammar, _) = parse_file(&filename, false)?;
            let warnings;
            if split {
                let dir = PathBuf::from(
                    output_dir.expect("clap requires --output-dir when --split is given"),
                );
                warnings = ts_bnf_tool::dom::railroad::emit_split(&grammar, &dir, annotate)?;
            } else {
                let svg;
                (svg, warnings) = ts_bnf_tool::dom::railroad::emit_single_file(
                    &grammar,
                    rule.as_deref(),
                    annotate,
                )
                .map_err(|msg| -> Box<dyn Error> { msg.into() })?;
                match output {
                    Some(path) => fs::write(&path, &svg)?,
                    None => print!("{svg}"),
                }
            }
            for w in &warnings {
                eprintln!("{w}");
            }
        }

        Subcommands::Graph {
            filename,
            format,
            output,
            start,
        } => {
            if matches!(format.as_str(), "pdf" | "png") && output.is_none() {
                eprintln!("error: --format {format} requires -o <file>");
                std::process::exit(1);
            }
            let (grammar, _) = parse_file(&filename, false)?;
            let (graph_data, warnings) =
                ts_bnf_tool::dom::graph::build_graph(&grammar, start.as_deref())
                    .map_err(|e| -> Box<dyn Error> { e.into() })?;
            for w in &warnings {
                eprintln!("{w}");
            }
            match format.as_str() {
                "dot" => {
                    let text = ts_bnf_tool::dom::graph::emit_dot(&graph_data);
                    match output {
                        Some(path) => fs::write(&path, &text)?,
                        None => print!("{text}"),
                    }
                }
                "mermaid" => {
                    let text = ts_bnf_tool::dom::graph::emit_mermaid(&graph_data);
                    match output {
                        Some(path) => fs::write(&path, &text)?,
                        None => print!("{text}"),
                    }
                }
                "svg" | "pdf" | "png" => {
                    let dot = ts_bnf_tool::dom::graph::emit_dot(&graph_data);
                    let bytes = ts_bnf_tool::dom::graph::run_graphviz(&dot, &format)?;
                    match output {
                        Some(path) => fs::write(&path, &bytes)?,
                        None => {
                            use std::io::Write;
                            std::io::stdout().write_all(&bytes)?;
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "unknown format '{other}'; expected: dot, mermaid, svg, pdf, png"
                    )
                    .into());
                }
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

        Subcommands::Rename {
            filename,
            from,
            to,
            in_place,
            output,
        } => {
            run_rename(&filename, &from, &to, in_place, output.as_deref())?;
        }

        Subcommands::Check {
            filename,
            json,
            summary,
        } => {
            let (grammar, diagnostics) = match parse_file(&filename, true) {
                Ok((g, d)) => (Some(g), d),
                Err(e) => {
                    let pe = e.downcast::<ParseError>()?;
                    match *pe {
                        ParseError::SyntaxError(diags) => (None, diags),
                        other => return Err(Box::new(other)),
                    }
                }
            };
            // bool::then is lazy — the closure (and first_sets inside it) only
            // runs when --summary was passed. Without it, summarise() would
            // always execute regardless of the flag.
            let grammar_summary = grammar.filter(|_| summary).map(|g| g.summarise());
            if json {
                // Always emit an object so the shape is stable regardless of
                // whether --summary is also passed. The "summary" key is
                // omitted when not requested.
                #[derive(serde::Serialize)]
                struct CheckJsonOutput<'a> {
                    diagnostics: &'a [Diagnostic],
                    #[serde(skip_serializing_if = "Option::is_none")]
                    summary: Option<GrammarSummary>,
                }
                let out = CheckJsonOutput {
                    diagnostics: &diagnostics,
                    summary: grammar_summary,
                };
                println!("{}", serde_json::to_string(&out)?);
            } else {
                for d in &diagnostics {
                    eprintln!("{d}");
                }
                if let Some(s) = grammar_summary {
                    println!("{s}");
                }
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

/// Entry point: runs the CLI and reports any error on stderr with a nonzero exit code.
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
