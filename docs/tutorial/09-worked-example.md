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
- Boolean keywords: `true`, `false`, `not`, `and`, `or`
- Identifiers for variable references
- Parenthesised expressions
- Each expression terminated by `;`

A valid expression: `not oracle or android and 2 * x + 1 ;`.

## Step 1 — write the naive grammar

Start with just the rules, no directives except `%extras` for whitespace. Save
this to `expr.bnf`:

```bnf
%extras /\s/

program    -> (expr ';')* ;
expr       -> expr 'or' expr
            | expr 'and' expr
            | expr '+' expr
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
ts-bnf-tool convert --name expr expr.bnf > grammar.js
tree-sitter generate
```

This fails. Tree-sitter reports a conflict like:

```
Error: Unresolved conflict for symbol sequence:

  '-'  expr  •  'or'  …

Possible interpretations:

  1:  '-'  (expr  expr  •  'or'  expr)
  2:  (expr  '-'  expr)  •  'or'  …

Possible resolutions:

  1:  Specify a left or right associativity in `expr`
  2:  Add a conflict for these rules: `expr`
```

The parser has reached a point where it sees `- expr` followed by `or` and
cannot decide:

1. **Interpretation 1**: treat `expr` as the left operand of `or`, making
   `- expr` one side of a binary expression → gives `(-x) or y`
2. **Interpretation 2**: reduce `- expr` first, giving `(-x)`, then combine
   with `or` → same result but reached differently

Without guidance on which operators bind tighter, every pair of operators
generates a conflict like this.

## Step 2 — resolve precedence with `%prec`

Operator precedence and associativity fix shift-reduce conflicts. Annotate each
alternative with `%prec.left N` (for binary operators) or `%prec N` (for prefix
operators), using higher `N` for tighter binding:

```bnf
%extras /\s/

program    -> (expr ';')* ;
expr       -> expr 'or' expr  %prec.left 1
            | expr 'and' expr %prec.left 2
            | expr '+' expr   %prec.left 3
            | expr '-' expr   %prec.left 3
            | expr '*' expr   %prec.left 4
            | expr '/' expr   %prec.left 4
            | 'not' expr      %prec 5
            | '-' expr        %prec 5
            | 'true'
            | 'false'
            | number
            | identifier
            | '(' expr ')'
            ;
identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
number     -> /[0-9]+/ ;
```

The annotations encode the precedence hierarchy:

| Rule | Level | Assoc | Meaning |
|------|-------|-------|---------|
| `expr 'or' expr` | 1 | left | loosest: `or` binds last |
| `expr 'and' expr` | 2 | left | tighter than `or` |
| `expr '+' expr` | 3 | left | arithmetic add/sub |
| `expr '-' expr` | 3 | left | same level as `+` |
| `expr '*' expr` | 4 | left | tighter than `+` |
| `expr '/' expr` | 4 | left | same level as `*` |
| `'not' expr` | 5 | — | tightest: unary `not` |
| `'-' expr` | 5 | — | same for unary minus |

Convert and generate again:

```sh
ts-bnf-tool convert --name expr expr.bnf > grammar.js
tree-sitter generate
```

The parser now generates successfully. Try it:

```sh
echo "1 + 2 * 3 ;" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [1, 0]
  (expr [0, 0] - [0, 9]
    (expr [0, 0] - [0, 1]
      (number [0, 0] - [0, 1]))
    (expr [0, 4] - [0, 9]
      (expr [0, 4] - [0, 5]
        (number [0, 4] - [0, 5]))
      (expr [0, 8] - [0, 9]
        (number [0, 8] - [0, 9])))))
```

`2 * 3` is nested as the right operand of `+` — correct. The operators are
anonymous and not shown as named nodes; each `(number …)` leaf holds the digit.

## Step 3 — fix keyword extraction with `%word`

The language has infix keyword operators: `or` and `and`. Now try a variable
named `oracle` without explicitly writing `or`:

```sh
echo "true oracle ;" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [1, 0]
  (expr [0, 0] - [0, 11]
    (expr [0, 0] - [0, 4])
    (expr [0, 7] - [0, 11]
      (identifier [0, 7] - [0, 11]))))
```

The input was silently mis-parsed. `oracle` (cols 5–10) was split into the
keyword `or` (cols 5–6, anonymous) and the identifier `acle` (cols 7–10),
producing `(true) or (acle)`. Same for `android`:

```sh
echo "true android ;" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [1, 0]
  (expr [0, 0] - [0, 12]
    (expr [0, 0] - [0, 4])
    (expr [0, 8] - [0, 12]
      (identifier [0, 8] - [0, 12]))))
```

`android` (cols 5–11) was split into `and` (cols 5–7) and `roid` (cols 8–11).

This happens because after a complete expression, the only valid next tokens are
binary operators (`or`, `and`, `+`, …) and `;` — identifiers are not expected
there. The lexer sees `or` at the start of `oracle`, finds it is a valid token
at that position, and stops after two characters. It never considers whether the
full string is an identifier.

The fix is `%word`. It designates one rule as the language's *word token*,
enabling **keyword extraction**: instead of matching keywords greedily by
position, tree-sitter first matches the full word-token pattern
(`/[a-zA-Z_][a-zA-Z0-9_]*/`), then checks whether the result is a known
keyword. If the whole word is a keyword it emits that token; if not, no keyword
matches at this position, and the lexer looks for other options.

Add `%word` at the top of the file:

```bnf
%word identifier
%extras /\s/

program    -> (expr ';')* ;
expr       -> expr 'or' expr  %prec.left 1
            | expr 'and' expr %prec.left 2
            | expr '+' expr   %prec.left 3
            | expr '-' expr   %prec.left 3
            | expr '*' expr   %prec.left 4
            | expr '/' expr   %prec.left 4
            | 'not' expr      %prec 5
            | '-' expr        %prec 5
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
ts-bnf-tool convert --name expr expr.bnf > grammar.js
tree-sitter generate
echo "true oracle ;" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [1, 0]
  (ERROR [0, 0] - [0, 4]
    (expr [0, 0] - [0, 4]))
  (expr [0, 5] - [0, 11]
    (identifier [0, 5] - [0, 11])))
```

The silent mis-parse is gone. With `%word`, the lexer tries the full identifier
pattern at `oracle`, matches 6 characters, and finds that `"oracle"` is not a
keyword. `or` is not emitted. The parser now correctly sees `oracle` as an
identifier — and since an identifier cannot appear in operator position after
`true`, it surfaces an error rather than silently accepting the wrong parse.

Verify that identifiers starting with keyword prefixes work correctly in atom
position:

```sh
echo "oracle or android and false ;" | tree-sitter parse /dev/stdin
```

```
(program [0, 0] - [1, 0]
  (expr [0, 0] - [0, 27]
    (expr [0, 0] - [0, 6]
      (identifier [0, 0] - [0, 6]))
    (expr [0, 10] - [0, 27]
      (expr [0, 10] - [0, 17]
        (identifier [0, 10] - [0, 17]))
      (expr [0, 22] - [0, 27]))))
```

`oracle` (cols 0–5) and `android` (cols 10–16) are each a single identifier.
The tree groups `android and false` first (level 2), then `oracle or …` (level
1) — reflecting the `and`-over-`or` precedence.

## The final grammar

```bnf
%word identifier
%extras /\s/

program    -> (expr ';')* ;
expr       -> expr 'or' expr  %prec.left 1
            | expr 'and' expr %prec.left 2
            | expr '+' expr   %prec.left 3
            | expr '-' expr   %prec.left 3
            | expr '*' expr   %prec.left 4
            | expr '/' expr   %prec.left 4
            | 'not' expr      %prec 5
            | '-' expr        %prec 5
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
