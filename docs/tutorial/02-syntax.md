# Syntax walkthrough

Every construct of the BNF dialect, one at a time, with the tree-sitter
JavaScript it generates.

## Rules and alternatives

A rule is written as:

```
name -> body ;
```

The body is a sequence of symbols separated by `|` for alternatives. The
semicolon is required at the end of every rule.

```bnf
color -> 'red' | 'green' | 'blue' ;
```

generates:

```js
color: $ => choice('red', 'green', 'blue'),
```

## Terminals: literals and patterns

A **literal** is a quoted string — single or double quotes both work:

```bnf
arrow -> '->' ;
kw_if -> "if" ;
```

### Escape sequences in literals

A literal's content is copied verbatim into `grammar.js`, where it is read as
a **JavaScript string literal** — so escape sequences have JavaScript
semantics. `\n`, `\t`, `\0`, `\\`, `\xNN` and `\u{…}` all mean what they mean
in JS. A quote of the same kind as the delimiter is escaped with a backslash;
the other kind needs no escape:

```bnf
newline           -> '\n' ;
line_continuation -> '\\' '\n' ;
nul               -> '\0' ;
letter_a          -> '\x41' ;
single_quote      -> '\'' ;
double_quote      -> "\"" ;
```

`ts-bnf-tool` does not validate escapes: whatever follows the backslash is
passed through unchanged, and JavaScript decides what it means. This keeps
the dialect automatically in step with any escape JS supports — but it also
means a typo such as `'\q'` is not caught here; JS silently reads it as `q`.

Control characters must be written as escapes. Raw line breaks (LF or CR —
both line terminators in JS, where a string literal cannot span lines) and
raw NUL bytes inside the quotes are syntax errors:

```bnf
broken -> 'a
b' ;          # syntax error — write 'a\nb'
```

A **pattern** is a regex delimited by slashes:

```bnf
ident  -> /[A-Za-z_][A-Za-z0-9_]*/ ;
number -> /[0-9]+/ ;
```

Patterns follow JavaScript regex syntax (tree-sitter uses a JS regex engine).
A pattern may carry JS regex flags after the closing slash — tree-sitter
honours `i` (case-insensitive):

```bnf
kw_select -> /select/i ;
```

The flag suffix is passed through to `grammar.js` verbatim; flag validity is
checked by `tree-sitter generate`.

## Sequences and grouping

Juxtaposition means sequence. Parentheses group without creating a named rule:

```bnf
pair    -> '(' expr ',' expr ')' ;
sep_seq -> item (',' item)* ;
```

## Quantifiers

| Syntax | Meaning | Maps to |
|--------|---------|---------|
| `x*`   | zero or more | `repeat(x)` |
| `x+`   | one or more  | `repeat1(x)` |
| `x?`   | optional     | `optional(x)` |

Quantifiers bind to the immediately preceding symbol or group:

```bnf
args -> '(' (expr (',' expr)*)? ')' ;
```

## Token expressions: `<< >>` and `<<! >>`

By default, tree-sitter allows whitespace and extras between any two tokens.
`<< >>` forces the enclosed expression to be treated as a single atomic lexer
token — no whitespace is allowed inside:

```bnf
identifier -> << /[A-Za-z_]/ /[A-Za-z0-9_]*/ >> ;
```

generates:

```js
identifier: $ => token(seq(/[A-Za-z_]/, /[A-Za-z0-9_]*/)),
```

`<<! >>` goes further: it also requires that no whitespace precedes the token.
This is useful for suffixes that must be attached to the preceding token:

```bnf
negative -> '-' <<! /[0-9]+/ >> ;
```

generates:

```js
negative: $ => seq('-', token.immediate(/[0-9]+/)),
```

Use `<< >>` when you want a multi-part terminal (e.g. a number followed by a
unit). Use `<<! >>` when the token must be glued to whatever comes before it
(e.g. a postfix operator, a type suffix like `u32`).

## Field labels

A field label annotates a symbol with a named field in the AST, using
tree-sitter's `field()`:

```bnf
assign -> target: ident '=' value: expr ;
```

generates:

```js
assign: $ => seq(field('target', $.ident), '=', field('value', $.expr)),
```

The colon must be attached to the label name (no space before it). A space
after the colon is optional. Field labels compose with quantifiers:

```bnf
call -> func: ident '(' args: expr* ')' ;
```

generates `field('args', repeat($.expr))`.

## Alias groups

An alias group relabels a sequence in the generated AST using `alias()`. The
`=>` separator divides the body from the target name:

```bnf
param_list -> '(' (name type => parameter)* ')' ;
```

generates:

```js
param_list: $ => seq('(', repeat(alias(seq($.name, $.type), $.parameter)), ')'),
```

The target name is either a bare identifier (a named node) or a quoted string
(an anonymous node):

```bnf
true_kw -> ('t' 'r' 'u' 'e' => 'true') ;
```

### How aliases behave

The alias name is a **display label** in the resulting syntax tree, not a
rule reference: the body does all the parsing, and the resulting node is
merely renamed. The name does not need to exist as a rule — and if a rule
with the same name *does* exist, the alias neither invokes nor references
it. Consequently, `check` does not count alias names as rule references: an
undefined alias label is not an `undefined rule reference`, and a rule
mentioned *only* as an alias label is still reported as never referenced
(it can never produce a node).

What the parse tree looks like, for each alias form:

- **Bare identifier → named node.** Given

  ```bnf
  member -> object: identifier '.' ( identifier => property_name ) ;
  ```

  parsing `foo.bar` yields

  ```
  (member
    object: (identifier)    ; "foo"
    (property_name))        ; "bar" — parsed by identifier, displayed as property_name
  ```

- **Quoted string → anonymous node.** Given

  ```bnf
  true_kw -> ( 't' 'r' 'u' 'e' => 'true' ) ;
  ```

  parsing `true` yields an *anonymous* node, exactly as if the rule body had
  been the plain string `'true'`: it does not appear as a named node in the
  tree and is only visible to queries via the `"true"` anonymous-node
  syntax.

## Precedence annotations

Precedence annotations wrap an alternative in a `prec()` call, resolving
ambiguities in the grammar. For an explanation of *why* these conflicts arise
(shift-reduce conflicts, LR parsing), see
[Shift-reduce conflicts and operator precedence](03-concepts.md#conflicts-precedence).

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

The four annotation kinds:

| Annotation | Level | Maps to |
|---|---|---|
| `%prec N` or `%prec 'name'` | required | `prec(N, ...)` or `prec('name', ...)` |
| `%prec.left` or `%prec.left N` or `%prec.left 'name'` | optional | `prec.left([N or 'name',] ...)` |
| `%prec.right` or `%prec.right N` or `%prec.right 'name'` | optional | `prec.right([N or 'name',] ...)` |
| `%prec.dynamic N` or `%prec.dynamic 'name'` | required | `prec.dynamic(N, ...)` or `prec.dynamic('name', ...)` |

The level is either a signed integer or a quoted name, never both. An
integer level such as `%prec -1` allows negative values and maps to
`prec(-1, ...)`; tree-sitter grammars commonly use negative precedence to
disfavour an interpretation. A named level such as `%prec 'unary'` must
match a string item declared in some [`%precedences`](04-directives.md#precedences)
group — referencing an undeclared name is an **error**.

To annotate a sub-expression rather than a whole alternative, wrap it in
parentheses with `%prec` inside:

```bnf
rule -> (a | b %prec 1) c ;
```

## Reserved-word annotation

A `(body %reserved setName)` annotation opts a symbol into a named
[reserved-word set](04-directives.md#reserved), overriding the implicit
global set for that occurrence only:

```bnf
member -> obj '.' (identifier %reserved propertyName) ;
```

generates:

```js
member: $ => seq($.obj, '.', reserved('propertyName', $.identifier)),
```

`setName` must match a set name declared with `%reserved`; referencing an
undeclared set name is an **error**.

## What is not supported

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

---

Previous: [Getting started](01-getting-started.md) · Next: [Tree-sitter concepts](03-concepts.md)
