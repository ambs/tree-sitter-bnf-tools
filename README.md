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
| Axiom directive        | `%axiom ruleName`           | `%axiom entry` |
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

`%axiom ruleName` declares an explicit root (start) rule. Without it, the root
is implicitly the *first rule declared*. Use `%axiom` when you want to debug a
sub-rule in isolation without rearranging the file. The named rule is emitted
first in `grammar.js`'s `rules:` block so tree-sitter treats it as the start
symbol. Declaring `%axiom` more than once, or naming an undefined rule, is an
error.

All five directives (`%axiom`, `%conflicts`, `%inline`, `%supertypes`, `%extras`)
map directly to the same-named fields in the generated `grammar.js` (except
`%axiom`, which controls rule order rather than emitting a field). A warning is
printed to stderr for any referenced rule name that has no definition in the
same file.

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

**Install from crates.io**

```sh
cargo install ts-bnf-tool
```

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
| `convert` | Convert BNF to `grammar.js` |
| `format` | Pretty-print a `.bnf` file in canonical style |
| `highlights` | Generate a skeleton `highlights.scm` |
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
  --no-header            Suppress the generated-file comment at the top of grammar.js
  -n, --no-check         Skip static checks; suppress all warnings and convert unconditionally
  --strict               Treat warnings as errors (conflicts with --no-check)
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
# creates ./expr/grammar.js, ./expr/queries/highlights.scm, runs tree-sitter generate
# producing ./expr/src/parser.c

ts-bnf-tool convert --generate --output-dir ~/parsers/arithmetic --name arithmetic expr.bnf
# creates the project at the specified path with an explicit grammar name
```

### format

Pretty-prints a `.bnf` file in canonical style: consistent spacing, one
alternative per line when a rule exceeds 80 characters, directives first.

```sh
ts-bnf-tool format grammar.bnf          # print to stdout
ts-bnf-tool format -i grammar.bnf       # rewrite in place (atomic)
ts-bnf-tool format --check grammar.bnf  # exit non-zero if not canonical (CI use)
```

Options:

```
  -i, --in-place         Rewrite the file in place
  --check                Exit non-zero if the file is not already formatted
  --strip-comments       Strip # comments from output (default)
  --no-strip-comments    Preserve # comments in output
```

### rename

Renames a rule definition and every reference to it — in rule bodies and in
`%axiom`, `%inline`, `%supertypes`, `%extras`, and `%conflicts` directives —
in one safe, mechanical pass. The result is re-emitted in canonical format.

```sh
ts-bnf-tool rename grammar.bnf expr expression        # print to stdout
ts-bnf-tool rename -i grammar.bnf expr expression     # rewrite in place (atomic)
ts-bnf-tool rename grammar.bnf expr expression -o out.bnf  # write to file
```

Exits non-zero if the source rule is not defined or the target name is already taken.

Options:

```
  -i, --in-place    Rewrite the file in place (cannot be used with stdin)
  -o <FILE>         Write output to this file instead of stdout
```

### highlights

Generates a best-effort skeleton `highlights.scm` — a Tree-sitter query file
that assigns capture names to grammar rules. Solves the blank-page problem when
starting a new tree-sitter grammar.

```sh
ts-bnf-tool highlights grammar.bnf              # print to stdout
ts-bnf-tool highlights grammar.bnf -o highlights.scm  # write to file
```

Rules whose bodies contain no terminals (purely structural rules) are omitted.
Recognised rules get a capture name based on their name; unrecognised rules get
a `; TODO: @???` placeholder for human review.

Options:

```
  -o <FILE>    Write output to this file instead of stdout
  --no-todos   Suppress `; TODO: @???` placeholder entries
```

Example output for a JSON grammar:

```scheme
; Generated by ts-bnf-tool v0.2.0 — edit as needed.
(string) @string
(number) @number
(line_comment) @comment
; (object) TODO: @???
; (pair) TODO: @???
```

The heuristics applied are:

| Rule name pattern | Capture |
|---|---|
| `comment`, `*_comment` | `@comment` |
| `string`, `char`, `*_string`, `string_*` | `@string` |
| `number`, `integer`, `float`, `*_literal` | `@number` |
| `keyword_*`, common keyword names (`if`, `else`, `return`, …) | `@keyword` |
| `operator`, `*_op`, `*_operator` | `@operator` |
| `identifier`, `name`, `*_identifier`, `*_name` | `@variable` |
| `boolean` | `@boolean` |
| `null`, `nil`, `none`, `undefined`, `void` | `@constant.builtin` |

### firsts

Prints the FIRST set of each rule — the set of terminals that can appear as the
first token of any string the rule can produce. Useful for spotting LL(1)
ambiguities: if two alternatives in a `choice(…)` share a terminal in their
FIRST sets, a single token of look-ahead cannot distinguish them.

```sh
ts-bnf-tool firsts expr.bnf
```

```
expr: '(', /[0-9]+/
term: '(', /[0-9]+/
```

Options:

```
  -n, --no-check   Skip static checks and suppress all warnings
  --json           Emit output as JSON instead of plain text
```

With `--json`, output is a JSON object mapping each rule name to a sorted array
of terminal strings:

```sh
ts-bnf-tool firsts --json expr.bnf
```

```json
{"expr": ["'('", "/[0-9]+/"], "term": ["'('", "/[0-9]+/"]}
```

### check

Runs all static checks and exits with a non-zero status if any issue is found.
Designed for CI pipelines.

```sh
ts-bnf-tool check grammar.bnf
```

Checks performed:

| Check | Severity | Example diagnostic |
|-------|----------|--------------------|
| Undefined rule references | warning | `warning: undefined rule reference 'foo'` |
| Undefined `%axiom` rule | **error** | `error: %axiom references undefined rule 'foo' (line 1)` |
| Duplicate `%axiom` | **error** | `error: %axiom declared more than once (line 2)` |
| Undefined `%conflicts` rules | warning | `warning: %conflicts references undefined rule 'foo'` |
| Undefined `%inline` rules | warning | `warning: %inline references undefined rule 'foo'` |
| Undefined `%supertypes` rules | warning | `warning: %supertypes references undefined rule 'foo'` |
| Undefined `%extras` rules | warning | `warning: %extras references undefined rule 'foo'` |
| Unreferenced rule | warning | `warning: rule 'foo' is never referenced (line 4)` |
| Direct left-recursion | **error** | `error: rule 'expr' is directly left-recursive (line 1)` |
| Mutual left-recursion | **error** | `error: rule 'a' is mutually left-recursive (line 1)` |

Options:

```
  --json   Emit diagnostics as JSON instead of plain text
```

With `--json`, diagnostics are written to stdout as a JSON array. Each element
has a `severity` field (`"warning"` or `"error"`) and a `message` field. Exit
codes are unaffected.

```sh
ts-bnf-tool check --json grammar.bnf
```

```json
[{"severity":"warning","message":"rule 'foo' is never referenced (line 3)"}]
```

Exit codes: `0` clean, `1` warnings only, `2` one or more errors.

Left-recursive rules are flagged as errors because tree-sitter cannot handle
them — they cause cryptic failures during parser generation. A rule is *directly*
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
