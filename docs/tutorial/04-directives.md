# Grammar-level directives

Directives appear at the top of the file (before or after rules; order does
not matter) and configure the generated `grammar.js`. All of them map directly
to the same-named fields in `grammar.js`, except `%axiom` (which controls rule
order) and `%include` (which merges files). An error is printed to stderr for
any referenced rule name that has no definition.

If you are new to tree-sitter, read [Tree-sitter grammar concepts](03-concepts.md)
first — it explains the underlying mechanisms that these directives control.

## `%word`

> **Background:** [Keyword extraction and `word:`](03-concepts.md#word-token)
> explains why the lexer can mis-tokenise identifiers like `oracle` as the
> keyword `or` + `acle` (in contexts where only operators are expected), and
> how `word:` fixes it.

Declares the rule that tree-sitter should treat as the language's identifier
token. This enables keyword extraction: literal keyword tokens (e.g. `'if'`,
`'while'`) are matched via the identifier pattern first, so input like `ifx`
is correctly lexed as one identifier rather than being mis-split into the
keyword `if` plus a dangling `x`. It also lets tree-sitter generate a smaller,
faster-compiling lexer:

```bnf
%word identifier

identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;
```

generates:

```js
word: $ => $.identifier,
```

`%word` names a single rule. Declaring it more than once is an **error**, as is
naming a rule that is not defined anywhere in the file.

The named rule's body must reduce to a single bare literal or pattern (e.g.
`identifier -> /[a-zA-Z_][a-zA-Z0-9_]*/ ;`, not a `seq`/`choice` built from
other rules), and it must be the only rule with that exact body — `check`
reports an **error** for either shape, since upstream `tree-sitter generate`
rejects both.

## `%extras`

> **Background:** [Extras: whitespace and comments](03-concepts.md#extras)
> explains what tree-sitter's built-in whitespace default is and when you need
> to override it.

Declares tokens that may appear anywhere in the input — typically whitespace
and comments:

```bnf
%extras /\s/, comment
```

generates:

```js
extras: $ => [/\s/, $.comment],
```

Without this directive, tree-sitter's built-in default (skip whitespace
everywhere) applies. Multiple `%extras` lines are additive — each adds items
to the list.

## `%conflicts`

> **Background:** [GLR conflicts](03-concepts.md#glr-conflicts) explains what
> structural ambiguity is, why precedence cannot resolve it, and how tree-sitter's
> GLR mode handles it.

Declares rule pairs that are expected to be ambiguous, allowing tree-sitter's
GLR parser to resolve them at parse time rather than aborting grammar
generation:

```bnf
%conflicts [expr, term]
%conflicts [foo, bar, baz], [a, b]
```

generates:

```js
conflicts: $ => [
  [$.expr, $.term],
  [$.foo, $.bar, $.baz],
  [$.a, $.b],
],
```

## `%precedences`

> **Background:** [Shift-reduce conflicts and operator precedence](03-concepts.md#conflicts-precedence)
> explains how inline `%prec` annotations (covered in the [syntax walkthrough](02-syntax.md#precedence-annotations))
> and `%precedences` groups work together to resolve operator ambiguity.

Declares named precedence levels in descending priority order. Each bracketed
group contains rule names or quoted string literals that share equal precedence;
groups listed earlier beat groups listed later:

```bnf
%precedences [_unary_expression, _binary_expression],
             [call, member, 'unary', 'binary']
```

generates:

```js
precedences: ($) => [
  [$._unary_expression, $._binary_expression],
  [$.call, $.member, 'unary', 'binary'],
],
```

Multiple `%precedences` lines are additive — each adds groups to the list.
Referencing an undefined rule name is an **error**; string literal items are
never checked against rule definitions, but a string item only has effect
once some rule alternative tags itself with that name via a named
[`%prec`](02-syntax.md#precedence-annotations) annotation:

```bnf
unary_expr  -> ('-' expr %prec 'unary') ;
binary_expr -> (expr '+' expr %prec 'binary') ;
```

Tagging `%prec` with a name that has no matching string item in any
`%precedences` group is itself an **error**.

## `%reserved`

Declares named reserved-word sets — groups of keywords that should be
preferred over a generic identifier rule wherever they apply. The first set
declared is the implicit **global** set, applied everywhere by default; later
sets (often empty) are swapped in for specific occurrences via the
[rule-level `%reserved` annotation](02-syntax.md#reserved-word-annotation).
Each set name is followed by a bracketed list of rule names or quoted string
literals — empty brackets declare a set with no reserved words:

```bnf
%reserved keywords: [if, else, 'while'],
          propertyName: []
```

generates:

```js
reserved: ($) => ({
  keywords: ($) => [$.if, $.else, 'while'],
  propertyName: ($) => [],
}),
```

Multiple `%reserved` directives are additive — each adds sets to the list;
the *first* set declared overall stays the implicit global set even if a
later line declares more sets. Referencing an undefined rule name is an
**error**; string literal items are never checked. A rule-level `%reserved`
annotation naming a set that was never declared here is also an **error**.

## `%inline`

Lists rules to substitute at every call site during parser-table generation.
Typically used for hidden helper rules that exist as structural glue:

```bnf
%inline _helper, _wrapper
```

generates:

```js
inline: $ => [$._helper, $._wrapper],
```

An inlined rule cannot be the grammar's resolved start rule, cannot also be
declared via `%externals`, and its body cannot reduce to a pure token (e.g.
`ident -> /[a-zA-Z_]+/ ;`) — `check` reports an **error** for any of these,
since upstream `tree-sitter generate` rejects them.

## `%supertypes`

> **Background:** [Hidden nodes and supertypes](03-concepts.md#hidden-supertypes)
> explains what hidden nodes and supertype rules are, and why they matter for
> language bindings.

Lists abstract rule names that act as union types over concrete subtypes. This
enriches the type annotations in language bindings. Declaring a rule as a
supertype also unconditionally hides it from the parse tree, even if its name
doesn't start with `_`:

```bnf
%supertypes expression, statement, declaration
```

generates:

```js
supertypes: $ => [$.expression, $.statement, $.declaration],
```

A supertype rule cannot also be the grammar's resolved start rule (see
`%axiom` below) — `check` reports an **error** if it is, since the start rule
must be visible.

A supertype rule's body also can't reduce to a pure token (e.g.
`ident -> /[a-zA-Z_]+/ ;`), and every one of its alternatives must be exactly
one step (`expr -> term | unary ;` is fine, `expr -> term | term '+' term ;`
is not) — `check` reports an **error** for either shape, since upstream
`tree-sitter generate` rejects both.

## `%externals`

> **Background:** [External scanners](03-concepts.md#external-scanners)
> explains what an external scanner is, when a regex is not enough (e.g.
> indentation-sensitive layout), and how the scanner integrates with the
> generated parser.

Declares tokens produced by an external scanner (a hand-written C lexer)
rather than the grammar itself. Items may be rule names or quoted string
literals:

```bnf
%externals indent, dedent, 'string_content'
```

generates:

```js
externals: $ => [$.indent, $.dedent, 'string_content'],
```

Multiple `%externals` lines are additive — each adds items to the list.
Declared names are exempt from undefined-reference errors: they are defined
by the external scanner, not by any rule in the BNF file. A name cannot be
declared in `%externals` *and* given a rule definition — `check` reports an
error if it is.

## `%axiom`

Declares an explicit root (start) rule. Without `%axiom`, tree-sitter treats
the *first rule declared* as the start symbol. Use `%axiom` when you want to
debug a sub-rule in isolation — temporarily redirect the entry point without
rearranging the file:

```bnf
%axiom expr

top_level -> stmt+ ;
expr      -> term ('+' term)* ;
term      -> /[0-9]+/ ;
```

`convert` silently emits `expr` first in `grammar.js`'s `rules:` block so
tree-sitter uses it as the start symbol, while the BNF file keeps its original
declaration order.

Declaring `%axiom` more than once is an **error**, as is naming a rule that is
not defined anywhere in the file. The resolved start rule — whether set by
`%axiom` or, absent `%axiom`, the implicit first-declared rule — also cannot
be hidden: tree-sitter requires the start rule to be visible, and `check`
reports an **error** if it is not, whether the rule is hidden by a
`_`-prefixed name or because it's also listed in `%supertypes`.

## `%include`

Merges another BNF file into the current grammar at the point of the
directive:

```bnf
%include "expressions.bnf"
```

Paths are relative to the including file. Includes may be nested (A includes B
includes C); circular includes (A includes B includes A) are detected and
reported as an error. All directives from included files are merged
additively. Duplicate rule names produce a warning (last definition wins);
duplicate `%axiom` declarations across files are an error. `%include` cannot
be used when reading from stdin.

---

Previous: [Tree-sitter concepts](03-concepts.md) · Next: [Cheat sheet](05-cheatsheet.md)
