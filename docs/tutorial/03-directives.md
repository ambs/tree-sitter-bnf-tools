# Grammar-level directives

Directives appear at the top of the file (before or after rules; order does
not matter) and configure the generated `grammar.js`. All of them map directly
to the same-named fields in `grammar.js`, except `%axiom` (which controls rule
order) and `%include` (which merges files). A warning is printed to stderr for
any referenced rule name that has no definition.

## `%extras`

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
everywhere) applies.

## `%conflicts`

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

## `%inline`

Lists rules to substitute at every call site during parser-table generation.
Typically used for hidden helper rules that exist as structural glue:

```bnf
%inline _helper, _wrapper
```

## `%supertypes`

Lists abstract rule names that act as union types over concrete subtypes. This
enriches the type annotations in language bindings:

```bnf
%supertypes expression, statement, declaration
```

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
not defined anywhere in the file.

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

Previous: [Syntax walkthrough](02-syntax.md) · Next: [Cheat sheet](04-cheatsheet.md)
