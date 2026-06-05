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

fn parse_check(args: &[&str]) -> Result<Cli, clap::Error> {
    let full: Vec<&str> = std::iter::once("ts-bnf-tool")
        .chain(std::iter::once("check"))
        .chain(args.iter().copied())
        .collect();
    Cli::try_parse_from(full)
}

fn parse_firsts(args: &[&str]) -> Result<Cli, clap::Error> {
    let full: Vec<&str> = std::iter::once("ts-bnf-tool")
        .chain(std::iter::once("firsts"))
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

#[test]
fn check_json_flag_defaults_false() {
    let cli = parse_check(&["f.bnf"]).unwrap();
    let Subcommands::Check { json, .. } = cli.command else {
        panic!("wrong subcommand");
    };
    assert!(!json);
}

#[test]
fn check_json_flag_parses() {
    let cli = parse_check(&["--json", "f.bnf"]).unwrap();
    let Subcommands::Check { json, .. } = cli.command else {
        panic!("wrong subcommand");
    };
    assert!(json);
}

#[test]
fn firsts_json_flag_defaults_false() {
    let cli = parse_firsts(&["f.bnf"]).unwrap();
    let Subcommands::Firsts { json, .. } = cli.command else {
        panic!("wrong subcommand");
    };
    assert!(!json);
}

#[test]
fn firsts_json_flag_parses() {
    let cli = parse_firsts(&["--json", "f.bnf"]).unwrap();
    let Subcommands::Firsts { json, .. } = cli.command else {
        panic!("wrong subcommand");
    };
    assert!(json);
}
