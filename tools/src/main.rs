mod dom;
mod visitors;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::dom::{ParseError, Scaffold};
use crate::visitors::visit_grammar;
use tree_sitter::Parser;

struct Args {
    filename: String,
    rules_only: bool,
    name: Option<String>,
}

fn parse_args() -> Option<Args> {
    let raw: Vec<String> = env::args().collect();
    let mut rules_only = false;
    let mut name: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--rules-only" => rules_only = true,
            "--name" => {
                i += 1;
                name = raw.get(i).cloned();
            }
            arg if !arg.starts_with('-') => filename = Some(arg.to_string()),
            _ => return None,
        }
        i += 1;
    }
    Some(Args {
        filename: filename?,
        rules_only,
        name,
    })
}

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
    let Some(args) = parse_args() else {
        eprintln!("Usage: bnf-tools [--rules-only] [--name NAME] <filename>");
        std::process::exit(1);
    };

    let mut file = File::open(&args.filename)?;
    let mut source_code = String::new();
    file.read_to_string(&mut source_code)?;

    let mut parser = Parser::new();
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

    if args.rules_only {
        println!("{}", grammar);
    } else {
        let name = grammar_name(&args.filename, args.name.as_deref());
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
