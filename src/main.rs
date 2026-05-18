mod dom;
mod visitors;

use std::env;
use std::fs::File;
use std::io::Read;

use crate::visitors::visit_grammar;
use tree_sitter::Parser;

fn main() {
    // Get command-line arguments
    let args: Vec<String> = env::args().collect();

    // Check if a filename was provided
    if args.len() != 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        std::process::exit(1);
    }

    // Get the filename from the first argument (args[0] is the program name)
    let filename = &args[1];

    // Open the file
    let mut file = File::open(filename).expect(&format!("Error opening file: {}", filename));

    // Create a string to store the file contents
    let mut source_code = String::new();

    // Read the file contents into the string
    file.read_to_string(&mut source_code).expect(&format!("Error reading file: {}", filename));
        
    let mut parser : Parser = Parser::new();
    // Set the language to BNF
    parser.set_language(&tree_sitter_bnf::LANGUAGE.into()).expect("Error loading BNF grammar");
            
    // Call the parser
    let tree = parser.parse(&source_code, None).expect(&format!("Error parsing source code: {}", filename));
    
    let root_node = tree.root_node();
    if root_node.has_error() {
        panic!("There was some parsing error")
    }
    let grammar = visit_grammar(&root_node, &source_code);
    println!("{}", grammar);
}
