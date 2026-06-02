# tree-sitter-bnf-tools

A [tree-sitter](https://tree-sitter.github.io/) grammar for BNF, plus a CLI tool
that converts BNF grammars into tree-sitter `grammar.js` notation.

New to the tool? Start with the **[tutorial](docs/tutorial.md)** for a guided
introduction with examples. This README is a reference for the full syntax.
Want syntax highlighting in your editor? See the **[editor setup guide](docs/editors.md)**.

## Repository structure

| Directory | Description |
|-----------|-------------|
| `tree-sitter-bnf/` | Tree-sitter grammar and language bindings (Rust, Node.js, C) |
| `tools/` | `ts-bnf-tool` CLI — converts BNF files to tree-sitter notation |

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
| Token expression | `<< >>` | `<< /[A-Za-z_]/ /[A-Za-z0-9_]*/ >>` |
| Token immediate expression | `<<! >>` | `'-' <<! /[0-9]+/ >>` |
| Field label | `name: symbol` | `lhs: expr` |
| Alias group | `(body => name)` | `(a b => pair)` |
| Precedence (alternative) | `body %prec.TYPE [N]` | `expr '+' expr %prec.left 1` |
| Precedence (sub-expression) | `(body %prec.TYPE [N])` | `(a \| b %prec 1)` |
| Conflicts directive    | `%conflicts [r1, r2, ...]`  | `%conflicts [expr, term]` |
| Inline directive       | `%inline r1, r2, ...`       | `%inline _helper, _wrapper` |
| Supertypes directive   | `%supertypes r1, r2, ...`   | `%supertypes expression, statement` |
| Extras directive       | `%extras item, ...`         | `%extras /\s/, comment` |

See the [tutorial](docs/tutorial.md) for worked examples and an explanation of
each construct. The key points are summarised below.

`<< >>` marks its contents as an atomic lexer terminal (`token()`) — no
whitespace between parts. `<<! >>` additionally requires no whitespace to
precede the token (`token.immediate()`).

Field labels (`label: symbol`) map to `field()`. Kleene operators on a labeled
symbol wrap the whole quantified expression: `items: expr*` →
`field('items', repeat($.expr))`.

Alias groups (`(body => name)`) map to `alias()`. The name is a bare identifier
for a named node or a quoted string for an anonymous node.

Precedence annotations (`%prec`, `%prec.left`, `%prec.right`, `%prec.dynamic`)
wrap an alternative or sub-expression in the corresponding `prec.*()` call. The
level is required for `%prec` and `%prec.dynamic`, optional for `.left`/`.right`.

All four directives (`%conflicts`, `%inline`, `%supertypes`, `%extras`) are
additive across multiple lines and map directly to the same-named fields in the
generated `grammar.js`. A warning is printed to stderr for any referenced rule
name that has no definition in the same file.

### Not supported

The following constructs from other BNF/EBNF variants are **not** recognised:

| Construct | Example | Why it fails |
|-----------|---------|--------------|
| `::=` / `:` / `=` rule separator | `expr ::= term` | Only `->` is accepted |
| Angle-bracket non-terminals | `<expr>` | Only bare identifiers are accepted |
| `[optional]` bracket notation | `['+'?]` | Use `?` instead: `'+'?` |
| `{repetition}` curly-brace notation | `{term}` | Use `*` instead: `term*` |
| Empty (epsilon) alternatives | `a -> b \|` | Trailing `\|` without a body is a parse error |
| ABNF character codes | `%x41` | Not implemented |
| Case-insensitive literals | `%i"text"` | Not implemented |

If your grammar uses any of the unsupported constructs, convert them to the
supported equivalents before running `ts-bnf-tool`.

## ts-bnf-tool

Converts a `.bnf` file to tree-sitter notation.

**Build from source**

```sh
make build
# binary is at target/release/ts-bnf-tool after: make release
```

**Usage**

```sh
ts-bnf-tool <SUBCOMMAND> [OPTIONS] <file.bnf>
```

Pass `-` as the filename to read from stdin.

| Subcommand | Purpose |
|------------|---------|
| `convert` | Convert BNF to `grammar.js` (default) |
| `firsts` | Print FIRST sets for each rule |
| `check` | Run static checks; exit non-zero on any issue |

### convert

**Print a complete `grammar.js` scaffold** (default)

Given `expr.bnf`:

```bnf
# arithmetic expressions
expr -> term ('+' term)* ;
term -> /[0-9]+/ | '(' expr ')' ;
```

Running `ts-bnf-tool convert expr.bnf` outputs:

```js
module.exports = grammar({
  name: "expr",

  rules: {
    expr: $ => seq($.term, repeat(seq('+', $.term))),
    term: $ => choice(/[0-9]+/, seq('(', $.expr, ')')),
  }
});
```

Options:

```
  --name <NAME>          Grammar name (default: filename stem)
  --rules-only           Print rule bodies only, without grammar.js boilerplate
  --generate             Write grammar.js to a directory and run tree-sitter generate
  --output-dir <DIR>     Output directory for --generate (default: ./<name>)
  -n, --no-check         Skip static checks; suppress all warnings and convert unconditionally
```

**Print rule bodies only**

```sh
ts-bnf-tool convert --rules-only expr.bnf
```

```
expr -> seq($.term, repeat(seq('+', $.term)))
term -> choice(/[0-9]+/, seq('(', $.expr, ')'))
```

**Generate a tree-sitter project**

```sh
ts-bnf-tool convert --generate expr.bnf
# creates ./expr/grammar.js and runs tree-sitter generate
# producing ./expr/src/parser.c

ts-bnf-tool convert --generate --output-dir ~/parsers/arithmetic --name arithmetic expr.bnf
# creates the project at the specified path with an explicit grammar name
```

### firsts

Prints the FIRST set of each rule — the set of terminals that can appear as the
first token of any string the rule can produce. Useful for spotting LL(1)
ambiguities: if two alternatives in a `choice(…)` share a terminal in their
FIRST sets, a single token of look-ahead cannot distinguish them.

```sh
ts-bnf-tool firsts expr.bnf
```

```
expr: '+', /[0-9]+/, '('
term: /[0-9]+/, '('
```

### check

Runs all static checks and exits with a non-zero status if any issue is found.
Designed for CI pipelines.

```sh
ts-bnf-tool check grammar.bnf
```

Checks performed:

| Check | Example warning |
|-------|----------------|
| Undefined rule references | `warning: undefined rule reference 'foo'` |
| Undefined `%conflicts` rules | `warning: %conflicts references undefined rule 'foo'` |
| Undefined `%inline` rules | `warning: %inline references undefined rule 'foo'` |
| Undefined `%supertypes` rules | `warning: %supertypes references undefined rule 'foo'` |
| Undefined `%extras` rules | `warning: %extras references undefined rule 'foo'` |
| Direct left-recursion | `warning: rule 'expr' is directly left-recursive` |
| Mutual left-recursion | `warning: rule 'a' is mutually left-recursive` |

Left-recursive rules are flagged because tree-sitter cannot handle them — they
cause cryptic failures during parser generation. A rule is *directly*
left-recursive if its own name can appear as the first symbol of one of its
alternatives (e.g. `expr -> expr '+' term | term`). It is *mutually*
left-recursive if two or more rules form a cycle where each can start with
the next (e.g. `a -> b 'x' | 'a'` and `b -> a 'y' | 'b'`).

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

Requires: Rust (stable), `tree-sitter-cli` ≥ 0.24.4 (`npm install -g tree-sitter-cli`).

## Contributing

Planned improvements and known limitations are tracked as
[GitHub issues](https://github.com/ambs/tree-sitter-bnf-tools/issues). If you
have an idea for a new feature, a construct that is missing, or a bug to report,
please open an issue — all feedback is welcome.

Pull requests are also welcome. Before opening one, please read the checklist
in [CLAUDE.md](CLAUDE.md) and make sure `make check` passes.

## License

MIT
