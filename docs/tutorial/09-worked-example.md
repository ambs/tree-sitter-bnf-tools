# Worked example: a boolean/arithmetic expression language

The [first end-to-end walkthrough](05-end-to-end.md) uses a JSON grammar — a
great first example because JSON is well-known and conflict-free. But JSON has
no keywords, no operator precedence, and no ambiguity. Most real languages have
at least one of these.

This page walks through a tiny expression language that does. By the end you
will have a working tree-sitter grammar that correctly handles keyword
extraction, operator precedence, and associativity — and you will understand
*why* each directive was needed.

## The language

Our mini-language has:

- Arithmetic: `+`, `-` (lower precedence) and `*`, `/` (higher precedence)
- Unary negation: `-expr`
- Boolean literals `true` and `false`, and boolean negation `not expr`
- Identifiers for variable references
- Parenthesised expressions

A valid expression: `not notable or 2 * x + 1`.

## Step 1 — write the naive grammar

Start with just the rules, no directives except `%extras` for whitespace:

```bnf
%extras /\s/

program    -> expr* ;
expr       -> expr '+' expr
            | expr '-' expr
            | expr '*' expr
            | expr '/' expr
            | 'not' expr
            | '-' expr
            | 'true'
            | 'false'
            | number
            | identifier
            | '(' expr ')'
            ;
identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
number     -> /[0-9]+/ ;
```

Convert it and try to generate a parser:

```sh
ts-bnf-tool convert expr.bnf > grammar.js
tree-sitter generate
```

This fails. Tree-sitter reports conflicts like:

```
Unresolved conflict for symbol sequence:

  'not'  expr  •  '+'  …

Possible interpretations:

  1:  (expr/5  'not'  expr)  '+'  …
  2:  'not'  (expr/1  expr  •  '+'  expr)
```

The parser has reduced `not expr` to a complete `expr` node (interpretation 1)
AND simultaneously wants to shift `+` to extend `expr` into a binary expression
(interpretation 2). With no guidance, it cannot choose.

There are similar conflicts for every pair of binary operators.

## Step 2 — resolve precedence with `%prec`

Operator precedence and associativity fix shift-reduce conflicts. Annotate each
alternative with `%prec.left N` (for binary operators) or `%prec N` (for prefix
operators), using higher `N` for tighter binding:

```bnf
%extras /\s/

program    -> expr* ;
expr       -> expr '+' expr %prec.left 1
            | expr '-' expr %prec.left 1
            | expr '*' expr %prec.left 2
            | expr '/' expr %prec.left 2
            | 'not' expr    %prec 3
            | '-' expr      %prec 3
            | 'true'
            | 'false'
            | number
            | identifier
            | '(' expr ')'
            ;
identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
number     -> /[0-9]+/ ;
```

The annotations encode four decisions:

| Rule | Level | Associativity | Meaning |
|------|-------|---------------|---------|
| `expr '+' expr` | 1 | left | `+` is left-associative, lower than `*` |
| `expr '-' expr` | 1 | left | same as `+` |
| `expr '*' expr` | 2 | left | `*` binds tighter than `+` |
| `expr '/' expr` | 2 | left | same as `*` |
| `'not' expr` | 3 | — | unary `not` binds tighter than any binary op |
| `'-' expr` | 3 | — | same for unary minus |

Convert and generate again:

```sh
ts-bnf-tool convert expr.bnf > grammar.js
tree-sitter generate
```

The parser now generates successfully. Try it:

```sh
echo "1 + 2 * 3" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [0, 10]
  (expr [0, 0] - [0, 10]
    (expr [0, 0] - [0, 1])
    (expr [0, 4] - [0, 10]
      (expr [0, 4] - [0, 5])
      (expr [0, 8] - [0, 9]))))
```

`2 * 3` is nested inside the right operand of `+` — correct.

## Step 3 — fix keyword extraction with `%word`

Now try an expression with identifiers:

```sh
echo "notable" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [0, 7]
  ERROR [0, 0] - [0, 7]
    (expr [0, 0] - [0, 3])
    (identifier [0, 3] - [0, 7]))
```

The lexer split `notable` into the keyword `not` and the identifier `able` —
wrong. Without any guidance, tree-sitter's lexer tries each keyword pattern
independently and matches `not` as soon as it sees those three characters.

The fix is `%word`. It designates one rule as the language's *word token*,
enabling **keyword extraction**: instead of matching keywords independently,
tree-sitter first matches the full word-token pattern (`/[a-zA-Z_][a-zA-Z0-9_]*/`
in our case), then checks whether the result is a known keyword. If yes, it
emits the keyword token; if no, it emits an identifier.

Add `%word` at the top of the file:

```bnf
%word identifier
%extras /\s/

program    -> expr* ;
expr       -> expr '+' expr %prec.left 1
            | expr '-' expr %prec.left 1
            | expr '*' expr %prec.left 2
            | expr '/' expr %prec.left 2
            | 'not' expr    %prec 3
            | '-' expr      %prec 3
            | 'true'
            | 'false'
            | number
            | identifier
            | '(' expr ')'
            ;
identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
number     -> /[0-9]+/ ;
```

Regenerate and test:

```sh
ts-bnf-tool convert expr.bnf > grammar.js
tree-sitter generate
echo "notable" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [0, 7]
  (expr [0, 0] - [0, 7]
    (identifier [0, 0] - [0, 7])))
```

`notable` is now a single identifier node.

Verify that actual keywords still work:

```sh
echo "not true" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [0, 8]
  (expr [0, 0] - [0, 8]
    (expr [0, 4] - [0, 8])))
```

`not` is recognised as the keyword, and `true` as the boolean literal.

## The final grammar

```bnf
%word identifier
%extras /\s/

program    -> expr* ;
expr       -> expr '+' expr %prec.left 1
            | expr '-' expr %prec.left 1
            | expr '*' expr %prec.left 2
            | expr '/' expr %prec.left 2
            | 'not' expr    %prec 3
            | '-' expr      %prec 3
            | 'true'
            | 'false'
            | number
            | identifier
            | '(' expr ')'
            ;
identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
number     -> /[0-9]+/ ;
```

Generate a full tree-sitter project in one step:

```sh
ts-bnf-tool convert --generate --name expr expr.bnf
```

## What to explore next

- **`%conflicts`** — if your language has a structurally ambiguous construct
  (such as a dangling-else), read [GLR conflicts](00-concepts.md#glr-conflicts)
  and add a `%conflicts` directive.
- **`%supertypes`** — if your language has abstract node types that enrich
  generated bindings, see [Hidden nodes and supertypes](00-concepts.md#hidden-supertypes).
- **`%externals`** — if your language is indentation-sensitive or needs tokens
  that a regex cannot describe, see [External scanners](00-concepts.md#external-scanners).

---

Previous: [End-to-end workflow](05-end-to-end.md) · Next: [Analysing a grammar](06-analysing.md)
