# TODO

Impact key: **high** = meaningful to most users or unlocks a category of use; **medium** = useful but narrower audience or easily worked around; **low** = nice-to-have, edge-case, or purely academic.

## Summary

| ID | Task | Category | Impact | Status |
|----|------|----------|--------|--------|
| G-01 | Optional operator (`?`) | Grammar | high | open |
| G-02 | Comments | Grammar | high | done |
| Q-01 | `locals.scm` | Query files | high | open |
| C-01 | Full `grammar.js` output + `--generate` | CLI | high | done |
| C-02 | Undefined-reference check | CLI | high | open |
| C-03 | Left-recursion detection | CLI | high | open |
| B-01 | WASM build | Bindings | high | open |
| D-01 | Editor setup guide | Docs | high | open |
| G-03 | Double-quoted string literals | Grammar | medium | open |
| Q-02 | `injections.scm` | Query files | medium | open |
| C-04 | `--format` flag | CLI | medium | open |
| C-05 | `--check` mode | CLI | medium | open |
| C-06 | Stdin support | CLI | medium | open |
| A-01 | Ambiguity hints | Analysis | medium | open |
| B-02 | Python bindings | Bindings | medium | open |
| B-03 | Node.js bindings | Bindings | medium | open |
| T-01 | Snapshot tests for `bnf-tools` | Testing | medium | open |
| D-02 | BNF dialect reference | Docs | medium | open |
| G-04 | `{...}` grouping | Grammar | low | open |
| G-05 | Epsilon / empty production | Grammar | low | open |
| Q-03 | `folds.scm` | Query files | low | open |
| Q-04 | `indents.scm` | Query files | low | open |
| C-07 | Unreachable-rule check | CLI | low | open |
| A-02 | Nullable detection | Analysis | low | open |
| A-03 | FIRST set computation | Analysis | low | open |
| T-02 | Error-recovery corpus tests | Testing | low | open |
| T-03 | Round-trip property test | Testing | low | open |

## Grammar extensions

- **G-01 — Optional operator (`?`)** *(high)* — add `?` as a Kleene-like operator mapping to `optional()` in tree-sitter output; it is the most glaring gap in the current dialect
- **G-02 — Comments** *(high)* — support `#` single-line comments so real-world grammar files with annotations can be parsed without stripping them first; `#` is unambiguous with all other tokens in the grammar and will be implemented via `extras` in `grammar.js` so comments are valid anywhere whitespace is
- **G-03 — Double-quoted string literals** *(medium)* — many BNF dialects allow `"..."` alongside `'...'`; accept both and normalise on output
- **G-04 — `{...}` grouping** *(low)* — some extended BNF dialects use curly braces for zero-or-more and square brackets for optional; recognising them broadens compatibility
- **G-05 — Epsilon / empty production** *(low)* — support `ε` or an explicit empty alternative (`a -> ;` or `a -> ε ;`) for grammars that name the empty string explicitly

## Tree-sitter query files

- **Q-01 — `locals.scm`** *(high)* — mark rule LHS names as `@definition.function` and bare non-terminal references as `@reference.function`; enables go-to-definition and find-references in editors that support tree-sitter locals
- **Q-02 — `injections.scm`** *(medium)* — inject the `regex` language into `pattern` nodes so `/[a-z]+/` is highlighted as a regex, not a plain string
- **Q-03 — `folds.scm`** *(low)* — fold each `rule` node so long grammars are navigable in editors
- **Q-04 — `indents.scm`** *(low)* — indentation hints for editors that auto-indent continuation lines of a rule body

## `bnf-tools` CLI

- **C-01 — Full `grammar.js` output + `--generate`** *(high)* — default output is now a complete, runnable `grammar.js` scaffold; `--rules-only` restores the old rule-body-only format; `--generate [--output-dir DIR] [--name NAME]` writes the scaffold to a directory and runs `tree-sitter generate` to produce `src/parser.c`; arg parsing uses clap
- **C-02 — Undefined-reference check** *(high)* — after parsing, warn when a non-terminal is referenced in a rule body but never defined as a rule LHS; catches real bugs in grammars before tree-sitter does
- **C-03 — Left-recursion detection** *(high)* — flag directly or mutually left-recursive rules, which tree-sitter cannot handle; saves users from cryptic parser failures
- **C-04 — `--format` flag** *(medium)* — switch the output target; initial candidates: `tree-sitter` (current default), `peg.js`, `lark`, `antlr4`
- **C-05 — `--check` mode** *(medium)* — run all static checks and exit non-zero on any error, suitable for CI
- **C-06 — Stdin support** *(medium)* — accept `-` as the filename to read from stdin, enabling pipeline use (`cat foo.bnf | bnf-tools -`)
- **C-07 — Unreachable-rule check** *(low)* — detect rules that are never referenced from any other rule (and are not the first/root rule)

## Grammar analysis (library)

- **A-01 — Ambiguity hints** *(medium)* — detect trivially ambiguous alternatives (duplicate branches in a `choice`)
- **A-02 — Nullable detection** *(low)* — identify which non-terminals can derive the empty string; prerequisite for correct FIRST/FOLLOW computation
- **A-03 — FIRST set computation** *(low)* — compute the set of leading terminals for each non-terminal; useful for LL-parsing feasibility checks

## Language bindings

- **B-01 — WASM build** *(high)* — `tree-sitter build --wasm`; publish the `.wasm` artifact so browser-based editors (e.g. Zed extensions, web playgrounds) can use the grammar without a native build step
- **B-02 — Python bindings** *(medium)* — enable `"python": true` in `tree-sitter.json` and add the generated binding; makes the grammar usable from Python tooling
- **B-03 — Node.js bindings** *(medium)* — enable `"node": true` for use from JavaScript/TypeScript projects

## Testing

- **T-01 — Snapshot tests for `bnf-tools`** *(medium)* — use `insta` to snapshot the CLI output for a set of representative `.bnf` fixtures; catches regressions in the converter without hand-writing expected strings
- **T-02 — Error-recovery corpus tests** *(low)* — add corpus cases with intentional syntax errors and assert on the resulting error-node structure; exercises tree-sitter's built-in error recovery
- **T-03 — Round-trip property test** *(low)* — generate random valid grammars, convert to tree-sitter notation, check that the output parses without errors

## Documentation

- **D-01 — Editor setup guide** *(high)* — step-by-step instructions for Neovim (nvim-treesitter) and Helix, covering parser installation, query file placement, and filetype registration
- **D-02 — BNF dialect reference** *(medium)* — document exactly which BNF constructs are and are not supported, so users know what to expect before filing issues
