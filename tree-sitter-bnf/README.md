# tree-sitter-bnf

A [tree-sitter](https://tree-sitter.github.io/) grammar for the BNF dialect used by
[`ts-bnf-tool`](https://github.com/ambs/tree-sitter-bnf-tools).

## Usage

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
tree-sitter-bnf = "0.1"
tree-sitter = "0.26"
```

Then parse BNF source with the `LANGUAGE` constant:

```rust
let code = r#"
expr -> term ('+' term)* ;
term -> /[0-9]+/ | '(' expr ')' ;
"#;

let mut parser = tree_sitter::Parser::new();
parser
    .set_language(&tree_sitter_bnf::LANGUAGE.into())
    .expect("Error loading BNF grammar");

let tree = parser.parse(code, None).unwrap();
assert!(!tree.root_node().has_error());
```

## BNF dialect

This grammar recognises a BNF variant with tree-sitter extensions:

| Construct | Syntax |
|-----------|--------|
| Rule | `name -> body ;` |
| Literal terminal | `'text'` |
| Pattern terminal | `/regex/` |
| Sequence | juxtaposition |
| Alternative | `\|` |
| Repetition | `*`, `+`, `?` |
| Token expression | `<< >>` |
| Field label | `name: symbol` |
| Precedence | `%prec.left N` |

See the [repository README](https://github.com/ambs/tree-sitter-bnf-tools#bnf-dialect)
for the full syntax reference.

## License

MIT
