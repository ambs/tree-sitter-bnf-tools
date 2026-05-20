# tree-sitter-bnf-tools

A [tree-sitter](https://tree-sitter.github.io/) grammar for BNF, plus a CLI tool
that converts BNF grammars into tree-sitter `grammar.js` notation.

## Repository structure

| Directory | Description |
|-----------|-------------|
| `tree-sitter-bnf/` | Tree-sitter grammar and language bindings (Rust, Node.js, C) |
| `tools/` | `bnf-tools` CLI — converts BNF files to tree-sitter notation |

Both modules are independent and can be split into separate repositories in the future.

## BNF dialect

The grammar supports the following syntax:

```bnf
expr    -> term ('+' term)*  ;
term    -> factor ('*' factor)* ;
factor  -> /[0-9]+/ | '(' expr ')' ;
```

| Construct | Syntax | Example |
|-----------|--------|---------|
| Rule | `name -> body ;` | `expr -> term ;` |
| Comment | `# text` | `# this is a comment` |
| Literal terminal | `'text'` | `'+'` |
| Pattern terminal | `/regex/` | `/[0-9]+/` |
| Non-terminal reference | bare identifier | `term` |
| Sequence | juxtaposition | `'(' expr ')'` |
| Alternative | `\|` | `'a' \| 'b'` |
| Zero or more | `*` | `term*` |
| One or more | `+` | `term+` |
| Zero or one (optional) | `?` | `','?` |
| Grouping | `( )` | `('a' \| 'b')*` |

### Not supported

The following constructs from other BNF/EBNF variants are **not** recognised:

| Construct | Example | Why it fails |
|-----------|---------|--------------|
| `::=` / `:` / `=` rule separator | `expr ::= term` | Only `->` is accepted |
| Double-quoted literals | `"text"` | Only single-quoted `'text'` is accepted |
| Angle-bracket non-terminals | `<expr>` | Only bare identifiers are accepted |
| `[optional]` bracket notation | `['+'?]` | Use `?` instead: `'+'?` |
| `{repetition}` curly-brace notation | `{term}` | Use `*` instead: `term*` |
| Empty (epsilon) alternatives | `a -> b \|` | Trailing `\|` without a body is a parse error |
| ABNF character codes | `%x41` | Not implemented |
| Case-insensitive literals | `%i"text"` | Not implemented |

If your grammar uses any of the unsupported constructs, convert them to the
supported equivalents before running `bnf-tools`.

## bnf-tools

Converts a `.bnf` file to tree-sitter notation.

**Build from source**

```sh
make build
# binary is at target/release/bnf-tools after: make release
```

**Usage**

```sh
bnf-tools [OPTIONS] <file.bnf>

Options:
  --name <NAME>          Grammar name (default: filename stem)
  --rules-only           Print rule bodies only, without grammar.js boilerplate
  --generate             Write grammar.js to a directory and run tree-sitter generate
  --output-dir <DIR>     Output directory for --generate (default: ./<name>)
  -h, --help             Print help
```

**Print a complete `grammar.js` scaffold** (default)

Given `expr.bnf`:

```bnf
# arithmetic expressions
expr -> term ('+' term)* ;
term -> /[0-9]+/ | '(' expr ')' ;
```

Running `bnf-tools expr.bnf` outputs:

```js
module.exports = grammar({
  name: "expr",

  rules: {
    expr: $ => seq($.term, repeat(seq('+', $.term))),
    term: $ => choice(/[0-9]+/, seq('(', $.expr, ')')),
  }
});
```

**Print rule bodies only**

```sh
bnf-tools --rules-only expr.bnf
```

```
expr -> seq($.term, repeat(seq('+', $.term)))
term -> choice(/[0-9]+/, seq('(', $.expr, ')'))
```

**Generate a tree-sitter project**

```sh
bnf-tools --generate expr.bnf
# creates ./expr/grammar.js and runs tree-sitter generate
# producing ./expr/src/parser.c

bnf-tools --generate --output-dir ~/parsers/arithmetic --name arithmetic expr.bnf
# creates the project at the specified path with an explicit grammar name
```

## Development

```sh
make            # show all available targets
make build      # build both crates
make test       # run Rust tests (generates parser.c if needed)
make test-grammar  # run tree-sitter corpus tests
make lint       # clippy
make fmt        # rustfmt
make clean      # remove build artifacts
```

Requires: Rust (stable), `tree-sitter-cli` (`npm install -g tree-sitter-cli`).

## License

MIT
