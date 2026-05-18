mod dom;
mod visitors;

use std::env;
use std::fs::File;
use std::io::Read;

use crate::visitors::visit_grammar;
use tree_sitter::Parser;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        std::process::exit(1);
    }

    let filename = &args[1];

    let mut file =
        File::open(filename).unwrap_or_else(|_| panic!("Error opening file: {}", filename));

    let mut source_code = String::new();
    file.read_to_string(&mut source_code)
        .unwrap_or_else(|_| panic!("Error reading file: {}", filename));

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .expect("Error loading BNF grammar");

    let tree = parser
        .parse(&source_code, None)
        .unwrap_or_else(|| panic!("Error parsing source code: {}", filename));

    let root_node = tree.root_node();
    if root_node.has_error() {
        panic!("There was some parsing error")
    }
    let grammar = visit_grammar(&root_node, &source_code);
    println!("{}", grammar);
}
