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

Pass `--json` to get diagnostics as a JSON object on stdout instead of plain
text on stderr. Exit codes are not affected:

```sh
ts-bnf-tool check --json json.bnf
```

```json
{"diagnostics":[{"severity":"warning","message":"rule 'unused' is never referenced (line 3)"}]}
```

### Left-recursion

Left-recursion is reported as an **error** (exit code 2) because tree-sitter
cannot generate a parser for left-recursive grammars and the resulting
error messages are cryptic.

A directly left-recursive rule references itself as the first symbol of one of
its alternatives:

```bnf
# BAD — directly left-recursive
expr -> expr '+' term | term ;
```

```
error: rule 'expr' is directly left-recursive (line 2)
```

Fix it by rewriting the grammar to use right-recursion or a repetition operator:

```bnf
# OK — right-recursive (or use repeat)
expr -> term ('+' term)* ;
```

Mutual left-recursion arises when two or more rules form a cycle:

```bnf
# BAD — mutually left-recursive
a -> b 'x' | 'a' ;
b -> a 'y' | 'b' ;
```

```
error: rule 'a' is mutually left-recursive (line 1)
error: rule 'b' is mutually left-recursive (line 2)
```

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
| **Undefined refs** | Rule names used in bodies but never defined — `check` flags these as warnings too. |
| **Left-recursive** | Rules involved in left-recursion, split into *direct* (`a → a …`) and *mutual* (`a → b …`, `b → a …`). `check` flags these as errors. |
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

Previous: [End-to-end workflow](05-end-to-end.md) · Next: [Formatting and refactoring](07-refactoring.md)
