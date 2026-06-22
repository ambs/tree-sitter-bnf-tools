# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

#### `tree-sitter-bnf`
- `%prec` (and `.left`/`.right`/`.dynamic`) annotations accept a quoted
  string name in place of the integer level, e.g. `%prec 'unary'`,
  generating `prec('unary', …)` in `grammar.js`. The name must match a
  string item declared in some `%precedences` group; `check` reports an
  error for a named level with no matching declaration. (#243)
- `%reserved setName: [r1, r2, 'literal'], otherSet: []` directive for named
  reserved-word sets, and a rule-level `(body %reserved setName)` annotation
  to override the global set for a specific occurrence. Maps to tree-sitter's
  `reserved:` grammar field and `reserved('setName', body)` call respectively.
  The first declared set is the implicit global set; multiple `%reserved`
  lines are additive. Referencing an undefined rule name, or annotating with
  an undeclared set name, is an error; string literals are not checked. (#175)
- `%externals name1, name2, 'literal'` directive for external scanner tokens.
  Maps to tree-sitter's `externals:` grammar field; items may be rule names or
  quoted string literals. Multiple `%externals` lines are additive. Declared
  names are exempt from undefined-reference errors, since they are defined
  by the external C scanner rather than the BNF. (#172)
- `%precedences [g1a, g1b], [g2a, g2b]` directive for named precedence levels.
  Groups are listed in descending priority order; members may be rule names or
  quoted string literals. Maps to tree-sitter's `precedences:` grammar field.
  Multiple `%precedences` lines are additive. Referencing an undefined rule name
  is an error; string literals are not checked. (#174)
- `%word ruleName` directive: declares the identifier token for keyword
  extraction and better error recovery. Maps to tree-sitter's `word:` grammar
  field. Duplicate declarations are an error; naming an undefined rule is an
  error. (#173)
- Patterns accept an optional JS regex flag suffix: `/select/i` is now valid
  syntax. The suffix is carried verbatim through `convert` and `format`, and
  `tree-sitter generate` serializes it as the `flags` field in `grammar.json`
  (flag validity is checked there, not by `check`). (#198)
- Precedence levels accept negative integers: `%prec -1` is now valid syntax.
  The sign is carried verbatim through `convert` (`prec(-1, …)`) and
  `format`. (#196)
- Literal escape semantics are now specified, in the tutorial and in
  `grammar/bnf.bnf`: a literal's content is emitted verbatim into `grammar.js`
  and read as a JavaScript string, so JS escape sequences (`\n`, `\0`, `\xNN`,
  `\\`, escaped quotes, …) apply by passthrough. Escapes are deliberately not
  validated by `check`, so escapes JS adds in the future work without tool
  changes. (#201)

#### `ts-bnf-tool`
- `convert --generate` now writes a minimal `tree-sitter.json` to the output
  directory (if one does not already exist). This satisfies tree-sitter ≥ 0.25's
  requirement for ABI 15 generation, eliminating the fallback-to-ABI-14 warning.
  An existing `tree-sitter.json` is never overwritten. (#199)

### Changed

#### `ts-bnf-tool`
- `check` now reports undefined-rule-reference issues as errors instead of
  warnings: undefined references in rule bodies, `%conflicts`, `%precedences`,
  `%inline`, `%supertypes`, `%extras`, and `%reserved` (both the rule-name and
  set-name forms). These conditions are unconditionally fatal to
  `tree-sitter generate`, so `make check`/CI now catches them instead of
  passing and failing later at generation time — breaking for grammars that
  previously relied on `check` passing despite one of these warnings. Only
  the "rule never referenced" check remains a warning, since it is stylistic
  rather than fatal. (#236)
- Syntax errors now report file, line, column and a source snippet for every
  error in the input (capped at 10), instead of a bare `Error: SyntaxError`.
  `check` routes them through the regular diagnostics output (plain and
  `--json`) and exits 2 — breaking for scripts that relied on exit 1 — while
  the other subcommands abort with the located messages on stderr. (#200)

### Removed

#### `ts-bnf-tool`
- `check` no longer reports left recursion as an error — tree-sitter (GLR)
  supports it. The counts remain in `check --summary`. (#197)

### Fixed

#### `ts-bnf-tool`
- `check` now errors when a `%externals` name is also given a rule
  definition, instead of staying silent until `tree-sitter generate` fails
  with `ExternalTokenNonTerminal`. (#246)
- `check` now errors when the resolved start rule — set via `%axiom`, or
  implicitly the first-declared rule — is hidden (`_`-prefixed). Upstream
  `tree-sitter generate` already rejects such a grammar with "A grammar's
  start rule must be visible"; `check` previously had no equivalent
  diagnostic. (#241)

## [0.3.0] - 2026-06-11

### Added

#### `tree-sitter-bnf`
- `axiomDirective` grammar rule: `%axiom ruleName` is now valid syntax.
  The `%axiom` keyword is highlighted as `@keyword` in `highlights.scm`.
- `includeDirective` grammar rule: `%include "path.bnf"` is now valid syntax.
  The `%include` keyword is highlighted as `@keyword` in `highlights.scm`.

#### `ts-bnf-tool`
- `graph` subcommand: emits a directed rule-dependency graph from a BNF grammar.
  Default output is Graphviz DOT (`--format dot`); Mermaid flowchart is
  available with `--format mermaid`. Rendered formats `svg`, `pdf`, and `png`
  are produced by shelling out to `dot` (Graphviz); `pdf` and `png` require
  `-o <file>`. The start symbol (the `%axiom` rule if declared, otherwise the
  first production) is highlighted with `shape=doublecircle` (DOT) or a `★`
  suffix (Mermaid). Undefined references
  are styled as dashed nodes (DOT) or carry a `⚠` suffix (Mermaid) and emit a
  warning to stderr. DOT node IDs are always quoted and Mermaid node IDs carry
  a trailing underscore (labels show the real rule name), so rule names that
  collide with Graphviz or Mermaid keywords (`node`, `edge`, `end`, …) stay
  valid. `--start <rule>` restricts output to the subgraph reachable
  from the named rule. Grammar files composed with `%include` are supported.
- `railroad` subcommand: generates railroad / syntax diagrams (SVG) from a BNF
  grammar. Supports single-file mode (all rules stacked in one SVG, with
  `#rule-<name>` fragment anchors and cross-rule links), split mode
  (`--split --output-dir <dir>`, one `<rule>.svg` per rule with relative-path
  links between files), and single-rule mode (`--rule <name>`). Non-terminal
  references to undefined rules emit a `warning:` to stderr but still produce a
  valid SVG node; exit code remains 0. No external binary is required.
- `%include "path.bnf"` directive: splits a large grammar across multiple files.
  Paths are resolved relative to the including file. Included files are merged
  in order, as if their contents were inlined at the `%include` site.
  Recursive includes (A→B→C) are supported. Circular includes (A→B→A) are
  detected and reported as an error. Including a file from stdin is an error.
  Duplicate rule names across files produce a warning (last definition wins);
  duplicate `%axiom` declarations are an error (first wins).
  All directives (`%extras`, `%inline`, `%supertypes`, `%conflicts`) from
  included files are merged additively. The `check`, `firsts`, `convert`, and
  `format` subcommands all operate on the fully-merged grammar.
- `check --summary`: appends a compact grammar metrics block to the output after
  diagnostics. Reports rule count (with leaf and unreachable breakdowns), unique
  terminal count (literals and patterns separately), undefined-reference count,
  left-recursive rule count (direct vs. mutual), and FIRST-set size statistics
  (min / max / avg). Summary is printed to stdout; diagnostics remain on stderr.
  Exit code is unaffected.
- `rename` subcommand: safely renames a rule definition and all its references
  (rule bodies, `%axiom`, `%inline`, `%supertypes`, `%extras`, `%conflicts`) in one pass.
  Supports `--in-place` / `-i` for atomic in-place rewrite and `-o <file>` to write to a separate file.
  Exits non-zero if the source rule is not defined or the target name is already taken.
- `highlights` subcommand: generates a skeleton `highlights.scm` from a BNF grammar using naming-convention heuristics. Rules whose bodies contain no terminals are omitted; unrecognised rules get a `; TODO: @???` placeholder. Supports `-o <file>` to write directly to a file and `--no-todos` to suppress placeholder entries.
- `--json` flag on `check`: emits output as a JSON object `{"diagnostics":[…]}` to stdout instead of plain text to stderr. Combined with `--summary`, a `"summary":{…}` key is added to the same object.
- `--json` flag on `firsts`: emits FIRST sets as a JSON object `{"rule": ["terminal", ...]}` to stdout instead of plain text.
- `%axiom ruleName` directive: declares an explicit root (start) rule.
  - `check`: emits an error if the named rule is undefined, and an error if
    `%axiom` is declared more than once in the same file.
  - `check`: the unreachable-rule check now exempts the axiom rule instead of
    the first-declared rule when `%axiom` is present.
  - `format`: `%axiom` is emitted first among directives (before `%extras`).
  - `convert`: the axiom rule is emitted first in `grammar.js`'s `rules:`
    block so tree-sitter treats it as the start symbol.

### Changed

#### Documentation
- README simplified into an overview with links into the documentation; the
  per-subcommand reference moved into the tutorial chapters.
- The tutorial was split into eight chapters under `docs/tutorial/`, with a
  documentation index at `docs/index.md` (also the GitHub Pages home).
- The railroad and graph examples in README and tutorial now use a small
  arithmetic grammar, shown alongside the diagrams. The diagrams for the BNF
  dialect's own grammar are published as `grammar/railroad.svg` and
  `grammar/graph.pdf`.
- The documentation is now published as a website at
  <https://ambs.github.io/tree-sitter-bnf-tools/>.

### Fixed

#### `ts-bnf-tool`
- `check` no longer treats alias display names (`(body => name)`) as rule
  references, matching tree-sitter alias semantics. An undefined alias label
  no longer emits a spurious `undefined rule reference` warning, and a rule
  mentioned *only* as an alias label is now correctly reported as never
  referenced.
- `railroad`: rule-name labels in generated SVG were cropped due to
  `text-anchor:middle` from the railroad crate's stylesheet. Labels now
  carry an explicit `text-anchor:start` override so names are fully visible.

## [0.2.0] - 2026-06-02

### Changed

- CI workflows now pin `tree-sitter-cli` to 0.26.9 for reproducible builds
- `make test-grammar` now requires `tree-sitter-cli` ≥ 0.24.4 and exits with a
  clear error message if the installed version is older

### Added

#### `tree-sitter-bnf`
- `folds.scm`: fold query that marks each `rule` node as foldable, making long
  grammars navigable in editors that support tree-sitter folding
- `docs/editors.md`: step-by-step setup guide for Neovim (nvim-treesitter) and
  Helix, covering parser installation, query file placement, and filetype registration

#### `ts-bnf-tool`
- `check`: warns about rules that are never referenced by any other rule
  (unreachable rule detection, issue #40). The root rule (first in the file)
  is always exempt, as are rules listed in `%extras` (e.g. whitespace handlers
  that are intentionally absent from rule bodies).
- `format --strip-comments` / `--no-strip-comments`: `--strip-comments` is the
  default; it strips all `#` comments from the output and, in `--check` mode,
  excludes comments from the comparison so a correctly-formatted file with comments
  still passes. Use `--no-strip-comments` to suppress stripping once comment
  round-tripping is implemented (see issue #148).
- `format` subcommand: pretty-prints a `.bnf` file in canonical style (consistent
  spacing around `->`, `|`, and `;`; one alternative per line when a rule exceeds
  80 characters; directives emitted first in canonical order). Supports `--in-place`
  (`-i`) for atomic in-place rewriting and `--check` for CI use.
- `convert` now emits a `// <file>:<line>` comment above each rule in the `rules: { … }` block,
  mapping generated JavaScript back to the originating BNF source line; omitted by `--rules-only`
- `convert` and `check` now warn when the same rule name is defined more than once;
  the second definition wins
- `convert` now warns when the derived grammar name is not a valid JavaScript identifier
  (e.g. `my-grammar.bnf` → name `my-grammar` contains a hyphen); suppressible with
  `--no-check`, and treated as a fatal warning by `--strict`. Use `--name` to override.
- `convert --strict`: exits non-zero when warnings are present; output is still written before
  exiting so the file is available for inspection; conflicts with `--no-check`
- `convert` now emits a `// Generated by ts-bnf-tool v<version> from <file> — do not edit by hand.`
  comment at the top of every `grammar.js` output by default; use `--no-header` to suppress it
- `firsts --no-check` (`-n`): skips all static checks and suppresses warnings,
  mirroring the same flag on `convert`
- `visitors::parse_source`: new public library function that parses a BNF source
  string and returns the `Grammar` DOM and diagnostics, eliminating duplicated
  parser setup boilerplate in consumers
- `firsts` subcommand: prints the FIRST set of each rule — the terminals that
  can appear as the first token of any string derived from that rule
- `check` subcommand: runs all static checks and exits non-zero on any issue;
  designed for CI pipelines
- Left-recursion detection in `check`: flags directly and mutually left-recursive
  rules with a diagnostic error; tree-sitter cannot generate a parser for
  left-recursive grammars
- `convert --no-check` (`-n`): skips all static checks and suppresses warnings,
  converting unconditionally; useful when warnings are expected or handled elsewhere

### Fixed

#### `ts-bnf-tool`
- All diagnostic messages now include a source line number, e.g.:
  - `rule 'expr' is directly left-recursive (line 3)`
  - `%inline references undefined rule '_helper' (line 1)`
  - `%conflicts references undefined rule 'ghost' (line 2)`
  - `%supertypes references undefined rule 'expr' (line 4)`
  - `%extras references undefined rule 'ws' (line 1)`

#### `ts-bnf-tool`
- `check` subcommand: diagnostic output is now sorted alphabetically by message,
  giving stable, reproducible warnings regardless of `HashSet` iteration order

### Changed

#### `ts-bnf-tool`
- **Breaking:** subcommand is now required. `ts-bnf-tool <file>` no longer works;
  use `ts-bnf-tool convert <file>` instead.
- Diagnostics now carry a severity level (`error` or `warning`). Left-recursion
  is now an **error** (previously a warning); `convert` aborts on errors unless
  `--no-check` is passed. `check` exits **2** on errors, **1** on warnings-only,
  and **0** when clean.

## [0.1.0] - 2026-05-25

### Added

#### `tree-sitter-bnf`
- Tree-sitter grammar and Rust bindings for a BNF dialect
- Syntax highlight queries (`highlights.scm`, `injections.scm`, `locals.scm`, `tags.scm`, `indents.scm`)
- Self-describing grammar (`grammar/bnf.bnf`)

#### `ts-bnf-tool`
- Convert `.bnf` files to tree-sitter `grammar.js` notation (default output)
- `--rules-only`: print rule bodies without boilerplate
- `--generate`: write `grammar.js` and run `tree-sitter generate` in one step
- `--name`, `--output-dir`: control grammar name and output location
- Read from stdin via `-` filename
- BNF dialect features:
  - Literals (`'text'`, `"text"`), patterns (`/regex/`), non-terminal references
  - Sequences, alternatives (`|`), grouping
  - Kleene operators: `*`, `+`, `?`
  - Token expressions (`<< >>`) and token-immediate expressions (`<<! >>`)
  - Field labels (`name: symbol`)
  - Alias groups (`(body => name)`)
  - Precedence annotations (`%prec`, `%prec.left`, `%prec.right`, `%prec.dynamic`)
  - Grammar directives: `%conflicts`, `%inline`, `%supertypes`, `%extras`
  - Line comments (`#`)
  - Warning on undefined rule references in directive and rule bodies

[Unreleased]: https://github.com/ambs/tree-sitter-bnf-tools/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/ambs/tree-sitter-bnf-tools/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ambs/tree-sitter-bnf-tools/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ambs/tree-sitter-bnf-tools/releases/tag/v0.1.0
