# Tree-sitter grammar concepts

This page explains the tree-sitter parser-generator mechanisms that the grammar
directives control. Read it before [Grammar-level directives](03-directives.md)
— the directives will make more sense once you understand what problem each one
solves.

If you are already comfortable with tree-sitter's own [Grammar DSL
guide](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html),
you can skip this page or use it as a quick reference.

---

## How tree-sitter parses

Tree-sitter generates an [LR(1)][wiki-lr] parser from your grammar — a fast,
table-driven parser that reads tokens left to right and looks at most one token
ahead to decide what to do next. When a grammar is locally ambiguous (more than
one parse is possible with one token of lookahead), tree-sitter extends the LR
algorithm to GLR (Generalized LR), which considers multiple interpretations in
parallel and picks one based on rules you provide.

[wiki-lr]: https://en.wikipedia.org/wiki/LR_parser

The key consequence for grammar authors is that LR parsing is sensitive to
**conflicts**: decision points where the parser cannot tell, from the current
state and one lookahead token, which action to take. Most conflicts are
harmless and resolvable with precedence or associativity annotations; a few
require explicit GLR handling.

---

## Shift-reduce conflicts and operator precedence {#conflicts-precedence}

The most common conflict arises with recursive binary-operator rules:

```bnf
expr -> expr '+' expr
      | expr '*' expr
      | /[0-9]+/
      ;
```

When the parser has reduced `1 + 2` to `expr` and the next token is `*`, it
faces a choice:

- **Reduce** now, treating `1 + 2` as the left operand of `*` → gives `(1+2)*3`
- **Shift** the `*`, waiting to collect `2 * 3` first → gives `1+(2*3)`

Without guidance tree-sitter aborts grammar generation with a conflict error.

The fix is **precedence annotations**. Adding `%prec.left` with numeric levels
to each alternative tells tree-sitter two things:

1. **Level**: alternatives with a higher number "bind tighter" (reduce first).
2. **Associativity**: `%prec.left` says "when two operators have the same level,
   the left one reduces first" (left-associative); `%prec.right` says the right
   one does.

```bnf
expr -> expr '+' expr %prec.left 1   # lowest precedence, left-associative
      | expr '*' expr %prec.left 2   # higher precedence, left-associative
      | /[0-9]+/
      ;
```

Now `1 + 2 * 3` unambiguously parses as `1 + (2 * 3)` (the `*` alternative has
a higher level), and `1 + 2 + 3` parses as `(1 + 2) + 3` (left-associative).

For prefix operators, a plain `%prec N` (no associativity) is usually enough:

```bnf
expr -> '-' expr %prec 3   # unary minus: higher than binary ops
      | ...
      ;
```

See [Precedence annotations](02-syntax.md#precedence-annotations) for the full
`%prec`/`%prec.left`/`%prec.right`/`%prec.dynamic` syntax.

---

## GLR conflicts {#glr-conflicts}

Some grammars are **structurally ambiguous** — two different parse trees are
genuinely valid for the same input, and no precedence annotation can pick one
without making the grammar wrong. The classic example is the dangling-else:

```bnf
stmt -> 'if' cond 'then' stmt
      | 'if' cond 'then' stmt 'else' stmt
      | other_stmt
      ;
```

For the input `if a then if b then c else d`, both parses are syntactically
valid:

```
if a then (if b then c else d)   # else belongs to inner if
if a then (if b then c) else d   # else belongs to outer if
```

Precedence cannot resolve this because neither alternative is "higher" than the
other in any useful sense. Tree-sitter handles it with **GLR parsing**: the
`%conflicts` directive declares that this ambiguity is expected and intentional,
and tree-sitter will apply the conflict-resolution heuristic (generally: prefer
the longer, "shift" parse, i.e. bind `else` to the inner `if`).

```bnf
%conflicts [stmt, stmt]
```

Use `%conflicts` only when:

- The grammar is genuinely ambiguous (restructuring cannot resolve it).
- You have verified that tree-sitter's GLR resolution gives the result you want.

For ambiguities that *can* be resolved by restructuring the grammar or by
adding precedence annotations, prefer those approaches — they produce a faster
parser and clearer diagnostics.

---

## Keyword extraction and `word:` {#word-token}

Tree-sitter's lexer is **separate from the parser** and is
**context-sensitive**: in each parser state, only the tokens that are valid
at that point are considered. This causes a problem for languages with infix
keyword operators such as `or`, `and`, or `in`.

Consider a grammar with the infix keyword `or` and an identifier rule
`/[a-zA-Z_][a-zA-Z0-9_]*/`. After a complete expression, the parser is in a
state where the valid next tokens are the binary operators (`or`, `+`, …)
and the statement terminator — identifiers are not expected there. If the
input is `oracle`, the lexer sees the two characters `or`, finds that is the
`or` keyword (valid at this position), and stops — it never reads ahead to
discover that `a`, `c`, `l`, `e` continue the word. So `oracle` is
incorrectly split into the keyword `or` and the identifier `acle`, silently
producing a wrong parse tree.

The `word:` field (controlled by `%word` in the BNF dialect) fixes this. It
designates one rule as the language's *word token*. Tree-sitter then applies
**keyword extraction**: whenever a keyword could match at the current position,
the lexer first reads the complete word-token pattern, then checks whether the
full result is a known keyword. `oracle` matches the identifier pattern in full
(6 characters), and since `"oracle"` is not the keyword `or`, the lexer does
not emit `or` at that position. The mis-parse is prevented.

The same principle applies to any keyword that is a prefix of a valid
identifier: `and` / `android`, `in` / `instanceof`, `is` / `isset`, and so on.

An important constraint: the word rule's body must be a single regex (or literal)
and must be unique — no other rule may use the same body. This is enforced by
`ts-bnf-tool check`.

---

## External scanners {#external-scanners}

Some tokens cannot be described with a regular expression. A common case is
Python's indentation-sensitive layout — `INDENT` and `DEDENT` tokens depend on
the *stack* of previous indentation levels, which a regex cannot capture.

Tree-sitter supports **external scanners**: hand-written C/C++ functions
compiled alongside the generated parser. They implement a `scan()` callback that
reads characters and emits custom tokens.

The `%externals` directive lists the token names the external scanner produces:

```bnf
%externals indent, dedent, newline
```

These names appear in grammar rules just like any other rule reference, but they
have no body in the BNF file — the scanner fills them in at parse time. Writing
the scanner itself (a `.c` file with a specific C API) is documented in
[tree-sitter's external scanner guide][ts-external].

[ts-external]: https://tree-sitter.github.io/tree-sitter/creating-parsers/4-external-scanners.html

---

## Hidden nodes and supertypes {#hidden-supertypes}

Tree-sitter's parse tree contains every node that the grammar produces — even
structural helper rules that are not meaningful to the consumer. **Hidden nodes**
let you suppress these from the visible parse tree.

A rule whose name begins with `_` is automatically hidden:

```bnf
_expr_item -> number | string | identifier ;
```

`_expr_item` is parsed and used internally, but it does not appear as a named
node in the tree — its children are "hoisted" up to its parent.

**Supertypes** go a step further: the `%supertypes` directive marks a rule as a
union type over its alternatives and hides it from the tree. This enriches the
type annotations that tree-sitter generates for language bindings (TypeScript,
Python, etc.):

```bnf
%supertypes expression, statement

expression -> binary_expr | unary_expr | literal ;
statement  -> if_stmt | while_stmt | return_stmt ;
```

Bindings now expose `Expression` and `Statement` as typed interfaces, while
keeping the tree clean.

A supertype rule's body must consist entirely of single-rule alternatives (no
inline sequences or terminals), and the rule cannot be the grammar's start rule.
These constraints are checked by `ts-bnf-tool check`.

---

## Extras: whitespace and comments {#extras}

Tree-sitter has a built-in default: skip any whitespace between tokens. Most
grammars override this with `%extras` to also handle comments, or to allow only
specific kinds of whitespace.

```bnf
%extras /\s/, line_comment
```

This tells tree-sitter: "whitespace (`/\s/`) and `line_comment` tokens may
appear between any two tokens and should be silently consumed." Omitting
`%extras` entirely activates the built-in default (skip `\s` only). Declaring
`%extras` with an *empty* list turns off whitespace skipping entirely, giving
you full control over where whitespace is permitted.

---

Previous: [Syntax walkthrough](02-syntax.md) · Next: [Grammar-level directives](03-directives.md)
