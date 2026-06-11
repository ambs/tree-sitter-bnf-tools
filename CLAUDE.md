# Fixing an issue

Before working on issue `#<n>`, create `plans/<n>.md` (gitignored, never
committed) with: a short task description, the plan, and a todo list whose
items have unique ids `<n>.<k>`, e.g. `- [ ] 187.1 Fix visit_alias_group`.

- Tick each item immediately when done (not in batch), so interrupted work
  can resume from the file.
- Besides implementation steps, always include items for: tests, `README.md`,
  `CHANGELOG.md` (see Changelog below), and docs/tutorial. If one doesn't
  apply, don't omit it — tick it with `N/A — <reason>`.
- Keep finished plan files as a local archive.

# Pull Requests

## Branch naming

| Type | Pattern | Use for |
|------|---------|---------|
| Feature | `feature/<slug>` | New functionality |
| Bug fix | `fix/<slug>` | Defect corrections |
| Chore | `chore/<slug>` | Tooling, config, restructuring |
| Docs | `docs/<slug>` | Documentation only |

Use kebab-case slugs, e.g. `chore/restructure`, `feature/corpus-tests`.

## Target branch

All PRs target `main`.

## Branch workflow

Before creating a branch for a new PR, always sync main with upstream:

```sh
git fetch origin
git merge --ff-only origin/main
```

Then create the branch from the updated main.

## Changelog

Every PR that adds, changes, or removes user-facing behaviour must include an
entry in `CHANGELOG.md` under `## [Unreleased]`.

- **Added** — new subcommands, flags, syntax, or query files
- **Changed** — behaviour changes to existing features (breaking changes noted explicitly)
- **Fixed** — bug fixes visible to users
- **Removed** — deprecated items that are now gone

Internal refactors, test additions, and CI changes that have no effect on the
user-visible interface do not need a changelog entry.

## Checklist before opening

- [ ] `main` is up to date with `origin/main` (`git fetch origin && git merge --ff-only origin/main`)
- [ ] Branch is up to date with `main` (`git rebase main`)
- [ ] `make check` passes
- [ ] `CHANGELOG.md` updated for any user-facing change

## Function documentation (issue #70)

All public and private functions/methods must have a `///` doc comment describing their
purpose. Use standard Rust doc comment style:

```rust
/// Short one-line summary.
fn foo() { ... }
```

The `missing_docs` lint (enabled in `Cargo.toml`) enforces this for public items.
File-level `#![allow(missing_docs)]` suppresses it for modules not yet fully documented;
remove the suppressor once all items in the file are covered.

## Grammar self-description

`grammar/bnf.bnf` is the BNF dialect's own grammar expressed in itself.
It must be kept in sync with `tree-sitter-bnf/grammar.js`: any change to the
grammar rules must be reflected in this file.

## PR description rules

- Never include unticked checklist items in a PR description.
- Before opening the PR, run through the checklist. Tick each item as it passes.
- If a check cannot be run (e.g. requires credentials or a specific environment),
  ask the user to run it before the PR is opened.
- Only open the PR once all items are ticked.
