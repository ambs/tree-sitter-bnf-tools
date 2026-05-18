mod dom;
mod visitors;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Read;

use crate::dom::ParseError;
use crate::visitors::visit_grammar;
use tree_sitter::Parser;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        std::process::exit(1);
    }

    let filename = &args[1];

    let mut file = File::open(filename)?;
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
    println!("{}", grammar);
    Ok(())
}
