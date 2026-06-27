# Analysing a grammar

## Checking for issues

`ts-bnf-tool check` runs all static checks on a grammar file and exits with a
non-zero status if any issue is found. This makes it easy to wire into a CI
pipeline:

```sh
ts-bnf-tool check json.bnf
echo $?   # 0 if clean, 1 if warnings only, 2 if any errors
```

Checks performed:

| Check | Severity | Example diagnostic |
|-------|----------|--------------------|
| Undefined rule references | **error** | `error: undefined rule reference 'foo'` |
| Undefined `%axiom` rule | **error** | `error: %axiom references undefined rule 'foo' (line 1)` |
| Duplicate `%axiom` | **error** | `error: %axiom declared more than once (line 2)` |
| Undefined `%conflicts` rules | **error** | `error: %conflicts references undefined rule 'foo'` |
| Undefined `%inline` rules | **error** | `error: %inline references undefined rule 'foo'` |
| Undefined `%supertypes` rules | **error** | `error: %supertypes references undefined rule 'foo'` |
| Undefined `%extras` rules | **error** | `error: %extras references undefined rule 'foo'` |
| Unreferenced rule | warning | `warning: rule 'foo' is never referenced (line 4)` |

Pass `--json` to get diagnostics as a JSON object on stdout instead of plain
text on stderr. Exit codes are not affected:

```sh
ts-bnf-tool check --json json.bnf
```

```json
{"diagnostics":[{"severity":"warning","message":"rule 'unused' is never referenced (line 3)"}]}
```

### Syntax errors

If the file does not parse at all, `check` reports each syntax error with its
file, line, column and a snippet of the offending source, then exits 2:

```bnf
root => 'a' ;
value -> 'b'
```

```
error: syntax error at broken.bnf:1:6 near '=> 'a' ;'
error: syntax error at broken.bnf:2:13: missing ';'
```

At most 10 syntax errors are listed; any excess is summarised in a final
`… and N more syntax errors` line. With `--json`, syntax errors appear as
regular entries in the `"diagnostics"` array. Every other subcommand
(`convert`, `format`, `graph`, …) aborts with the same located messages on
stderr and exits 1.

### Left-recursion

Left-recursive rules are **not** flagged by `check`. Tree-sitter is a GLR
parser generator: left recursion is fully supported and is the idiomatic
style for binary and postfix expression rules.

```bnf
# OK — directly left-recursive, idiomatic for binary operators
expr -> expr '+' term | term ;
```

Left recursion is still a grammar property worth knowing about — for
instance, a left-recursive rule may need a `%prec` annotation or a
`%conflicts` entry to resolve ambiguity. The `check --summary` block
reports how many rules are directly or mutually left-recursive (see
[Summarising grammar shape](#summarising-grammar-shape) below).

What actually makes `tree-sitter generate` fail is *unresolved ambiguity*
— for example `expr -> expr '+' expr | 'n'` with no precedence annotation.
See [Shift-reduce conflicts and operator precedence](00-concepts.md#conflicts-precedence)
for how to resolve these with `%prec` annotations.
Ahead-of-time detection of such conflicts is planned separately
([#31](https://github.com/ambs/tree-sitter-bnf-tools/issues/31)).

### Unreferenced rules

A rule that is defined but never referenced by any other rule (and is not the
root rule) is reported as a warning. The root is either the rule named by
`%axiom`, or — when `%axiom` is absent — the first-declared rule:

```bnf
root   -> item+ ;
item   -> /[a-z]+/ ;
unused -> 'x' ;   # never referenced
```

```
warning: rule 'unused' is never referenced (line 3)
```

## Summarising grammar shape

`check --summary` appends a compact metrics block to stdout after the run.
Diagnostics still go to stderr, so the two streams can be captured independently
in shell pipelines.

```sh
ts-bnf-tool check --summary json.bnf
```

```
Rules            6  (leaf: 2, unreachable: 0)
Terminals       12  (literals: 10, patterns: 2, unique values)
Undefined refs   0
Left-recursive   0  (direct: 0, mutual: 0)
FIRST sets      min 1  max 7  avg 2
```

Each row measures a different aspect of the grammar:

| Row | What it tells you |
|-----|-------------------|
| **Rules** | Total named productions. *leaf* = rules whose body contains no rule references (only terminals). *unreachable* = rules never reached from the root, which `check` also flags as warnings. |
| **Terminals** | Unique terminal values across all rule bodies, split into string literals and regex patterns. See the note on uniqueness below. |
| **Undefined refs** | Rule names used in bodies but never defined — `check` flags these as errors too. |
| **Left-recursive** | Rules involved in left-recursion, split into *direct* (`a → a …`) and *mutual* (`a → b …`, `b → a …`). Informational only — left recursion is idiomatic tree-sitter style, not a defect. |
| **FIRST sets** | Size statistics (min / max / average) of the FIRST set of each rule — the set of terminals that can open a derivation. A large max or high average suggests the grammar may have ambiguous alternatives. |

> **Terminal uniqueness** is measured by raw source text, not by what the lexer
> matches. `'x'` and `"x"` are counted as two distinct literals even though they
> match the same character. The count reflects how many distinct token patterns
> the grammar author wrote, which is a useful proxy for lexer complexity.

### Using `--summary` with `--json`

Combining `--json` and `--summary` adds a `"summary"` key to the JSON output
alongside `"diagnostics"`, making both machine-readable in a single pass:

```sh
ts-bnf-tool check --json --summary json.bnf | jq .summary.rules
```

The full `"summary"` object shape:

```json
{
  "rules": 6,
  "leaf_rules": 2,
  "unreachable_rules": 0,
  "unique_literals": 8,
  "unique_patterns": 6,
  "undefined_refs": 0,
  "left_recursive_direct": 0,
  "left_recursive_mutual": 0,
  "first_sets": { "min": 1, "max": 7, "avg": 3.3 }
}
```

`first_sets` is `null` when the grammar has no productions.

### `check` options

```
  --json      Emit output as a JSON object instead of plain text
  --summary   Append a grammar metrics block after diagnostics
```

## Inspecting FIRST sets

`ts-bnf-tool firsts` prints the FIRST set of each rule — the set of terminals
that can appear as the very first token of any string the rule can derive. This
is useful for understanding LL(1) feasibility: if two alternatives in a
`choice(…)` share a terminal, a single token of look-ahead cannot tell them
apart.

```sh
ts-bnf-tool firsts json.bnf
```

```
array: '['
number: /\-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?/
object: '{'
pair: '"'
string: '"'
value: '"', '[', 'false', 'null', 'true', '{', /\-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?/
```

Pass `--json` to get a JSON object instead, suitable for editor plugins or
other tooling that consumes structured output:

```sh
ts-bnf-tool firsts --json json.bnf
```

```json
{
  "array":  ["'['"],
  "number": ["/\\-?[0-9]+(\\.[0-9]+)?([eE][+-]?[0-9]+)?/"],
  "object": ["'{'"],
  "pair":   ["'\"'"],
  "string": ["'\"'"],
  "value":  ["'\"'", "'['", "'false'", "'null'", "'true'", "'{'", "/\\-?[0-9]+(\\.[0-9]+)?([eE][+-]?[0-9]+)?/"]
}
```

### `firsts` options

```
  -n, --no-check   Skip static checks and suppress all warnings
  --json           Emit output as JSON instead of plain text
```

---

Previous: [Worked example](09-worked-example.md) · Next: [Formatting and refactoring](07-refactoring.md)
