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

A `<< >>` token expression marks its contents as an atomic lexer terminal — no
whitespace or extras are allowed between its parts. It maps directly to
tree-sitter's `token()` DSL function.

A `<<! >>` token immediate expression is like `<< >>` but additionally requires
that no whitespace precedes the token. It maps to tree-sitter's
`token.immediate()` DSL function and is typically used to bind a suffix tightly
to the preceding token:

```bnf
negative -> '-' <<! /[0-9]+/ >> ;
```

generates:

```js
negative: $ => seq('-', token.immediate(/[0-9]+/)),
```

A field label annotates a symbol with a named field in the generated AST,
mapping to tree-sitter's `field()` DSL function.  The colon must be attached
to the label name (no space before it); a space after the colon is optional.

```bnf
assign -> target: name '=' value: expr ;
```

generates:

```js
assign: $ => seq(field('target', $.name), '=', field('value', $.expr)),
```

A Kleene operator applied to a labeled symbol wraps the whole quantified
expression: `items: expr*` generates `field('items', repeat($.expr))`.

An alias group relabels a sequence in the generated AST, mapping to
tree-sitter's `alias()` DSL function.  The `=>` separator divides the body
from the target name.  The name is either a bare identifier (producing a named
node) or a quoted string (producing an anonymous node):

```bnf
param_list -> '(' (name type => parameter)* ')' ;
```

generates:

```js
param_list: $ => seq('(', repeat(alias(seq($.name, $.type), $.parameter)), ')'),
```

With a quoted string name:

```bnf
kw_true -> ('t' 'r' 'u' 'e' => 'true') ;
```

generates:

```js
kw_true: $ => alias(seq('t', 'r', 'u', 'e'), 'true'),
```

Kleene operators and field labels compose with alias groups in the usual way.

### Precedence annotations

Precedence annotations map to tree-sitter's `prec`, `prec.left`, `prec.right`, and
`prec.dynamic` DSL functions.

**Annotating a whole alternative** (no parentheses needed):

```bnf
expr -> expr '+' expr  %prec.left 1
      | expr '*' expr  %prec.left 2
      | expr '^' expr  %prec.right 3
      | '-' expr       %prec 4
      ;
```

generates:

```js
expr: $ => choice(
  prec.left(1, seq($.expr, '+', $.expr)),
  prec.left(2, seq($.expr, '*', $.expr)),
  prec.right(3, seq($.expr, '^', $.expr)),
  prec(4, seq('-', $.expr)),
),
```

**Annotating a sub-expression** using parentheses — `%prec` inside `()` applies to the
whole body, mirroring the `=> alias` syntax:

```bnf
rule -> (a | b %prec 1) c ;    # prec wraps choice(a, b)
rule -> (a | (b %prec 1)) c ;  # prec wraps only b
```

The four annotation kinds follow tree-sitter's naming. The level is optional for
`.left` and `.right` (defaulting to 0), and required for `prec` and `.dynamic`:

| Annotation | Level | Maps to |
|---|---|---|
| `%prec N` | required | `prec(N, ...)` |
| `%prec.left` or `%prec.left N` | optional | `prec.left([N,] ...)` |
| `%prec.right` or `%prec.right N` | optional | `prec.right([N,] ...)` |
| `%prec.dynamic N` | required | `prec.dynamic(N, ...)` |

Precedence annotations compose with field labels, kleene operators, and alias groups:

```bnf
ops: (expr '+' expr %prec.left 1)*          # field + kleene + prec
rule -> (expr '+' expr %prec.left 1 => add) ; # prec + alias in one group
```

### Conflicts directive

The `%conflicts` directive declares groups of rules that the parser is expected
to be ambiguous about, mapping to the `conflicts` field in the generated
`grammar.js`. tree-sitter uses a GLR parser and will abort grammar generation
if it encounters an unexpected ambiguity; listing the conflict explicitly allows
it to resolve the ambiguity at parse time instead.

Each directive declares one or more conflict groups, where each group is a
bracketed list of two or more rule names:

```bnf
%conflicts [expr, term]
%conflicts [foo, bar, baz], [a, b]
```

Multiple `%conflicts` lines are allowed and additive — all groups across all
directives are collected into a single `conflicts` array:

```js
conflicts: $ => [
  [$.expr, $.term],
  [$.foo, $.bar, $.baz],
  [$.a, $.b],
],
```

A warning is printed to stderr for any rule name referenced in a `%conflicts`
group that has no corresponding rule definition in the same file.

### Inline directive

The `%inline` directive lists rules that the parser generator should inline at
every call site, mapping to the `inline` field in the generated `grammar.js`.
Inlined rules are substituted at parse-table generation time — they never become
parser states of their own. This is typically used for hidden helper rules
(prefixed with `_`) that exist purely as structural glue.

```bnf
%inline _helper
%inline _sep, _wrapper
```

Multiple `%inline` lines are allowed and additive — all names are collected into
a single `inline` array:

```js
inline: $ => [$.‌_helper, $._sep, $._wrapper],
```

A warning is printed to stderr for any rule name referenced in `%inline` that
has no corresponding rule definition in the same file.

### Supertypes directive

The `%supertypes` directive lists abstract rule names that act as union types
over a set of concrete subtypes, mapping to the `supertypes` field in the
generated `grammar.js`. Tree-sitter uses this information to produce richer
type annotations in language bindings and in `node-types.json` — consumers
see a named union type (e.g. `Expression`) rather than a flat list of node
kinds.

```bnf
%supertypes expression
%supertypes expression, statement, declaration
```

Multiple `%supertypes` lines are allowed and additive — all names are collected
into a single `supertypes` array:

```js
supertypes: $ => [$.expression, $.statement, $.declaration],
```

A warning is printed to stderr for any rule name referenced in `%supertypes`
that has no corresponding rule definition in the same file.

### Extras directive

The `%extras` directive declares tokens that may appear anywhere in the input,
mapping to the `extras` field in the generated `grammar.js`. Each item is
either a regex pattern (for anonymous extras such as whitespace) or a bare rule
name (for named extras such as a comment rule).

```bnf
%extras /\s/
%extras /\s/, comment
```

Multiple `%extras` lines are allowed and additive — all items are collected into
a single `extras` array:

```js
extras: $ => [/\s/, $.comment],
```

When no `%extras` directive is present, tree-sitter's built-in default applies
(whitespace is skipped everywhere). A warning is printed to stderr for any rule
name referenced in `%extras` that has no corresponding rule definition in the
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
