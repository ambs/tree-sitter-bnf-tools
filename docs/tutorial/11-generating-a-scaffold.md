# Generating a processing scaffold

`ts-bnf-tool scaffold` scaffolds a complete, self-contained Rust crate for
parsing and traversing a BNF-described language: the tree-sitter parser, an
ANTLR-style `Visitor<'tree>` trait — one `visit_*` method per node kind, a
central `visit()` dispatcher, and a `combine`-based fold so you only write
the bodies you care about — and a runnable example. `cd` into the output
directory and `cargo run --example walk -- <file>` works immediately, with
no edits. This is a Rust-only feature for now — the subcommand's name is
deliberately target-language-neutral, since a future target might scaffold
a module or package instead of a crate.

```sh
ts-bnf-tool scaffold grammar.bnf                # crate in ./<name>
ts-bnf-tool scaffold -o out/decls grammar.bnf   # crate in out/decls
ts-bnf-tool scaffold --name decls grammar.bnf   # override the crate/grammar name
ts-bnf-tool scaffold --no-header grammar.bnf    # suppress generated-file comments
```

`--name` also affects wording in the generated trait's own doc comment; it
defaults to the input filename's stem. Like `railroad` and `graph`,
`scaffold` runs no static checks before generating — diagnostics never gate
output — but it does check that no two rules would generate the same
`visit_*` method (see below): a grammar that fails this check is rejected
with a clear diagnostic before anything is written to disk.

Re-running `scaffold` after editing the grammar is safe: `grammar.js`,
`src/*`, `bindings/rust/build.rs`, and `bindings/rust/visitor.rs` are
regenerated every time so they always track the current grammar, but
`Cargo.toml`, `bindings/rust/lib.rs`, `examples/walk.rs`,
`queries/highlights.scm`, and `.gitignore` are only ever written once — if
they already exist, `scaffold` leaves them alone, so hand-written code (and
any highlighting refinements — see
[Refine the highlights skeleton](06-end-to-end.md#step-5--refine-the-highlights-skeleton))
survives a grammar change. Since `queries/highlights.scm` is frozen after
its first write, it won't pick up new rules on its own; regenerate it
explicitly with `ts-bnf-tool highlights -o queries/highlights.scm` when the
grammar gains rules you want highlighted.

## What gets generated

Save this tiny declaration language as `decls.bnf`:

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

`ts-bnf-tool scaffold --name decls decls.bnf` creates a `decls/` directory:

```
decls/
├── Cargo.toml
├── grammar.js
├── tree-sitter.json
├── queries/highlights.scm
├── src/
│   ├── parser.c
│   ├── node-types.json
│   └── tree_sitter/...
├── bindings/rust/
│   ├── build.rs
│   ├── lib.rs
│   └── visitor.rs
└── examples/
    └── walk.rs
```

The `grammar.js`/`queries/`/`tree-sitter.json`/`src/` files are exactly what
`convert --generate` already produces — the real `tree-sitter generate`
output, unchanged. `scaffold` adds the `bindings/rust/` and `examples/`
directories on top:

- **`bindings/rust/lib.rs`** — the parser bindings (`LANGUAGE`, `NODE_TYPES`,
  same shape as any `tree-sitter generate`-produced crate), a `pub mod
  visitor;`, and a `parse` convenience function. This file is only ever
  scaffolded once — it's where to add `pub mod` declarations for your own
  hand-written `Visitor` implementations, and a rerun won't touch them. One
  exception: if you scaffolded without `--ast-types` and later rerun with
  it added, the rerun still inserts the one line it needs
  (`pub mod ast;`) into your existing `lib.rs` — otherwise the newly
  generated `examples/ast.rs` couldn't even compile — but never removes
  anything you've added yourself.

  ```rust
  pub fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
      let mut parser = tree_sitter::Parser::new();
      parser.set_language(&LANGUAGE.into())?;
      parser
          .parse(source, None)
          .ok_or_else(|| "tree-sitter failed to parse the given source".into())
  }
  ```

- **`bindings/rust/visitor.rs`** — the generated `Visitor<'tree>` trait,
  described below.

- **`examples/walk.rs`** — a small program that parses a file and counts its
  nodes, implementing nothing but the trait's one required method (also
  described below).

## The generated `Visitor` trait

For `decls.bnf`, `bindings/rust/visitor.rs` has one method per kind
(`visit_program`, `visit_decl`, `visit_expr`, `visit_ident`, `visit_num`), a
`visit()` dispatcher matching on `node.kind()`, and five ANTLR-mirroring
helper methods with sensible default bodies. The `decl` kind has two fields,
so its doc comment lists both:

```rust
/// Visits a `decl` node.
///
/// **Fields:**
/// - `target` -> `ident` ([`Visitor::visit_ident`]), via `self.field_visitor(node, "target")`
/// - `value` -> `expr` ([`Visitor::visit_expr`]), via `self.field_visitor(node, "value")`
///
/// **Anonymous children** (not visited by default): `'='`, `';'`
fn visit_decl(&mut self, node: SourceNode<'tree>) -> Result<Self::Output, Self::Error> {
    self.children_visitor(node)
}
```

`ident` and `num` have no visible children of their own, so they're leaves —
each is a single token with no substructure at all, so there's no "Anonymous
children" section either:

```rust
/// Visits a `ident` node.
///
/// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
fn visit_ident(&mut self, node: SourceNode<'tree>) -> Result<Self::Output, Self::Error> {
    let _ = node;
    self.default_result()
}
```

If you've used ANTLR's `AbstractParseTreeVisitor`, the shape should feel
familiar — the trait's own header doc comment includes this table:

| ANTLR (`AbstractParseTreeVisitor`) | Here                                                            |
|-------------------------------------|-----------------------------------------------------------------|
| `visit(tree)`                        | `Visitor::visit`                                                 |
| `visitChildren(node)`                | `Visitor::children_visitor`                                      |
| `aggregateResult(agg, next)`         | `Visitor::combine` (`Vec`-based, not pairwise)                   |
| `defaultResult()`                    | `Visitor::default_result` (`= combine(vec![])`)                  |
| `visitErrorNode(node)`               | `Visitor::error_visitor`                                          |
| (no ANTLR analogue)                  | `Visitor::missing_visitor`, for tree-sitter's `MISSING` nodes    |

The one thing without an ANTLR equivalent is `missing_visitor`: tree-sitter's
error recovery can insert a zero-width `MISSING` node standing in for a token
the parser expected but never found. `Node::kind()` reports the *expected*
kind on such a node, not a distinct "missing" kind, so `visit()` checks
`Node::is_missing()` before ever matching on the kind, and routes there
instead.

`combine` is the one method you must implement; everything else has a
default body. Which shape to give it depends on what you're computing:

| Output type              | Typical `combine` body                        |
|---------------------------|-------------------------------------------------|
| `()` (side-effect)        | `Ok(())`                                        |
| `Vec<T>` (collect)        | `Ok(results.into_iter().flatten().collect())`   |
| `Option<T>` (find-first)  | `Ok(results.into_iter().flatten().next())`      |

`examples/walk.rs`'s `Counter` uses the first pattern, implementing only
`combine`:

```rust
struct Counter {
    total: usize,
}

impl<'t> Visitor<'t> for Counter {
    type Output = ();
    type Error = std::convert::Infallible;

    fn combine(&mut self, _results: Vec<()>) -> Result<(), Self::Error> {
        self.total += 1;
        Ok(())
    }
}
```

`combine` runs exactly once per node visited — every default `visit_*`
method eventually calls it, whether through `children_visitor`'s fold or
`default_result`'s `combine(vec![])` — so counting `combine` calls counts
every named node in the tree without touching a single per-kind method.
(`children_visitor` iterates `named_children`, so anonymous tokens like
`'='`/`';'` are never visited and never counted.)

## Running it

No edits needed:

```sh
$ echo 'x = 1;
y = x;' > sample.decls
$ cargo run --example walk -- sample.decls
sample.decls: 9 node(s)
```

(the root `program` node, plus two `decl`s each contributing itself, its
`target` `ident`, its `value`'s `expr` wrapper, and the `ident`/`num` inside
that — 4 nodes per `decl`, 8 total, plus `program` itself — `9` in total.)

## A real use: extracting declared names

Say you want the list of names declared by a `decls.bnf` program, ignoring
any identifiers that appear only on the right-hand side of `=`. Override
`visit_ident` to capture a leaf's own text, and override `visit_decl` to
visit *only* its `target` field — skipping `value` entirely, so a name used
inside an expression never gets collected as if it were a declaration. This
can replace `examples/walk.rs`, or live alongside it as a second example:

Every `visit_*` method receives a `SourceNode`, which bundles the
`tree_sitter::Node` with the source text it was parsed from — so
`DeclExtractor` doesn't need to store `source` itself the way a bare
`Node<'tree>`-based visitor would; `node.source` is already there, and
`node.utf8_text(...)` reaches `Node`'s own method through `SourceNode`'s
`Deref` impl.

```rust
use decls::visitor::{SourceNode, Visitor};

struct DeclExtractor;

impl<'t> Visitor<'t> for DeclExtractor {
    type Output = Vec<String>;
    type Error = std::convert::Infallible;

    fn combine(&mut self, results: Vec<Vec<String>>) -> Result<Vec<String>, Self::Error> {
        Ok(results.into_iter().flatten().collect())
    }

    fn visit_ident(&mut self, node: SourceNode<'t>) -> Result<Vec<String>, Self::Error> {
        Ok(vec![node.utf8_text(node.source.as_bytes()).unwrap().to_string()])
    }

    fn visit_decl(&mut self, node: SourceNode<'t>) -> Result<Vec<String>, Self::Error> {
        self.field_visitor(node, "target")
    }
}

fn main() {
    let source = "x = 1;\ny = x;\n";
    let tree = decls::parse(source).expect("parse must succeed");
    let mut extractor = DeclExtractor;
    let root = SourceNode { node: tree.root_node(), source };
    let names = extractor.visit(root).unwrap();
    assert_eq!(names, vec!["x", "y"]);
}
```

`visit_program`'s default body (`children_visitor`) already does the right
thing: it visits every `decl`, and `combine`'s `flatten` concatenates their
results. Given `x = 1; y = x;`, `DeclExtractor` returns `["x", "y"]` — the
declared names, not `x`'s later use as a value.

## Typed node structs (`--ast-types`)

The generated `Visitor` trait works with `SourceNode` — a thin wrapper
around `tree_sitter::Node` plus the source text — and
`node.kind()`/`children_by_field_name` reached through it — you get
traversal and dispatch, but node payloads stay stringly-typed. Passing
`--ast-types` alongside `scaffold` adds a second, independent layer on top:
`bindings/rust/ast.rs`, one owned Rust struct per grammar rule (no `'tree`
lifetime survives construction), each with a
`TryFrom<super::visitor::SourceNode<'tree>>` impl and a `_pragma:
runtime::Pragma` field recording its start line/column. A leaf kind — one
with no visible children of its own — additionally carries `_text: String`.
`Pragma` and `BuildError` (the shared `TryFrom` error type) live inside an
inner `runtime` module rather than at `ast.rs`'s own top level, and the two
injected fields are spelled with a leading underscore — both so that a
grammar rule or field genuinely named `pragma`, `text`, `build_error`, or
`source_node` can never collide with these fixed, tool-injected names; see
"A note on vocabulary" below.

```sh
ts-bnf-tool scaffold --name decls --ast-types decls.bnf
```

For `decls.bnf` (the running example from earlier in this tutorial),
`bindings/rust/ast.rs` gets one struct per kind. `decl` has two fields, so
its `TryFrom` impl builds both from the parsed node:

```rust
/// `decl` node.
#[derive(Debug)]
pub struct Decl {
    pub _pragma: runtime::Pragma,
    pub target: Ident,
    pub value: Expr,
}

impl<'tree> TryFrom<super::visitor::SourceNode<'tree>> for Decl {
    type Error = runtime::BuildError;

    fn try_from(node: super::visitor::SourceNode<'tree>) -> Result<Self, Self::Error> {
        let _pragma = runtime::Pragma::from(node);
        let target = node
            .child_by_field("target")
            .ok_or(runtime::BuildError::MissingField { kind: "decl", field: "target" })?
            .try_into()?;
        let value = node
            .child_by_field("value")
            .ok_or(runtime::BuildError::MissingField { kind: "decl", field: "value" })?
            .try_into()?;
        Ok(Decl { _pragma, target, value })
    }
}
```

`ident`, being a leaf, gets a `_text: String` field instead of any nested
struct:

```rust
/// `ident` node.
#[derive(Debug)]
pub struct Ident {
    pub _pragma: runtime::Pragma,
    pub _text: String,
}
```

Every generated struct/enum derives `Debug`, so `{:#?}` recursively
pretty-prints an entire typed tree with no per-kind code — exactly what the
scaffolded `examples/ast.rs` does, the same bar `examples/walk.rs` already
meets:

```sh
$ printf 'x = 1;\ny = x;\n' > sample.decls
$ cargo run --example ast -- sample.decls
sample.decls:
Program {
    _pragma: Pragma {
        line: 1,
        column: 1,
    },
}
```

(`Program` has only `_pragma` here — `decl*` in `program -> decl* ;` has no
field label of its own, and only labeled fields become struct fields; give
it one, e.g. `program -> items: decl* ;`, to get a `pub items: Vec<Decl>`
field instead.)

Like `visitor.rs`, `ast.rs` is always regenerated from the current grammar —
there's no hand-edit support for it. If you need more fields on a generated
type, wrap it in your own struct rather than editing the generated file; a
rerun of `scaffold --ast-types` overwrites it every time, same as
`visitor.rs`, while `examples/ast.rs` itself follows the same
write-once-then-leave-alone rule as `examples/walk.rs`.

**A note on vocabulary.** Grammar rule and field names are otherwise
completely unrestricted — including `pragma`, `text`, `build_error`, and
`source_node`, all plausible names a real grammar might want (a `pragma`
directive rule, a `text` leaf, a field called `text`). A kind named any of
these generates an ordinary top-level `struct Pragma`/`Text`/`BuildError`/
`SourceNode`, distinct from the tool's own fixed `runtime::Pragma`,
`runtime::BuildError`, and `super::visitor::SourceNode`. The only
restriction is on field labels: one starting with `_` is rejected at
generation time, since that whole leading-underscore namespace is reserved
for the fields this tool injects itself (`_pragma`, `_text`).

### Collapsing related kinds (`--merge-config`)

A grammar sometimes has several kinds that are really one construct from the
caller's point of view — `for_statement`/`while_statement`/`repeat_statement`
all being "a loop", say. Left alone, `--ast-types` gives each its own
unrelated struct. Passing `--merge-config <path>` alongside `--ast-types`
collapses a group of kinds like that into a single Rust `enum`, self-
discriminating on the Rust type tag rather than a stringly `kind` field.

The config is TOML with up to three kinds of entry:

```toml
# ast-merge.toml
[[merge]]
target = "Loop"
from = ["for_statement", "while_statement", "repeat_statement"]

[[passthrough]]
kind = "comment"
target = "DocComment"
```

- **`merge`** collapses every kind in `from` into one `enum` named `target`,
  one variant per source kind.
- **`passthrough`** renames a single kind's generated struct without
  otherwise changing it — here, `comment`'s struct is emitted as
  `DocComment` instead of the `Comment` its kind name would otherwise
  derive.
- **`ignore`** (not used above) explicitly marks a kind as "leave it as the
  default baseline struct" — see the coverage report below for why you'd
  write this out loud instead of just doing nothing.

Every `merge`/`passthrough` entry's own `target` is emitted verbatim as a
Rust `struct`/`enum` name, so it must be a valid, non-keyword Rust
identifier (e.g. `Loop`, not `loop`, `my-loop`, or an empty string) —
`scaffold` rejects the config up front otherwise, rather than emitting
non-compiling Rust.

Given a grammar with a `program -> items: (for_statement | while_statement |
repeat_statement)* doc: comment ;` rule and the config above,
`ts-bnf-tool scaffold --ast-types --merge-config ast-merge.toml grammar.bnf`
generates:

```rust
pub struct Program {
    pub _pragma: runtime::Pragma,
    pub items: Vec<Loop>,
    pub doc: DocComment,
}
```

```rust
#[allow(private_interfaces)]
#[derive(Debug)]
pub enum Loop {
    ForStatement(ForStatement),
    WhileStatement(WhileStatement),
    RepeatStatement(RepeatStatement),
}

impl<'tree> TryFrom<super::visitor::SourceNode<'tree>> for Loop {
    type Error = runtime::BuildError;

    fn try_from(node: super::visitor::SourceNode<'tree>) -> Result<Self, Self::Error> {
        match node.node.kind() {
            "for_statement" => Ok(Loop::ForStatement(node.try_into()?)),
            "while_statement" => Ok(Loop::WhileStatement(node.try_into()?)),
            "repeat_statement" => Ok(Loop::RepeatStatement(node.try_into()?)),
            found => Err(runtime::BuildError::UnexpectedKind {
                expected: "Loop",
                found: found.to_string(),
            }),
        }
    }
}
```

`ForStatement`/`WhileStatement`/`RepeatStatement` themselves still exist and
still each get the ordinary `TryFrom` impl `--ast-types` always generates —
they're just no longer `pub`, so `Loop`'s three variants are the only way
code outside the generated crate ever sees them. `Comment`'s struct doesn't
exist at all under that name; it's emitted as `DocComment` per the
`passthrough` entry, `pub` like any ordinary kind.

**Coverage report.** A grammar can drift out of sync with its merge config —
a new rule gets added, and nobody's decided yet whether it should merge,
passthrough, or be left alone. Whenever `--merge-config` is passed,
`scaffold` prints one line to stderr for every visible kind not named by any
`merge`, `passthrough`, or `ignore` entry:

```
warning: kind 'program' is not covered by --merge-config (no merge/passthrough/ignore entry); it will be emitted as an ordinary baseline struct
```

This is advisory only — it never affects the exit code, and the uncovered
kind still generates normally as an ordinary `pub` struct, exactly as if
`--merge-config` hadn't been passed for that kind at all. Once you've
reviewed a grammar's kinds and are happy leaving the rest as ordinary
structs, silence the report with the wildcard `ignore` entry:

```toml
ignore = ["*"]

[[merge]]
target = "Loop"
from = ["for_statement", "while_statement", "repeat_statement"]
```

`ignore = ["*"]` must be the config's only `ignore` entry — it means "every
kind not otherwise claimed", so listing specific kinds alongside it would be
redundant at best and a likely typo at worst; `check_merge_config` rejects
the combination outright.

## Typed field accessors without ownership

If you'd rather have typed field accessors directly over borrowed
`tree_sitter::Node`s — no owned copy, no `'tree`-free struct, no
merge/collapse — see [type-sitter](https://github.com/Jakobeha/type-sitter),
which generates those from the same `node-types.json` the generated crate's
`NODE_TYPES` constant also embeds. It composes with the `Visitor` trait the
same way it always has, independently of `--ast-types`: a `Visitor`
implementation can construct type-sitter's typed wrappers from the `Node`
inside the `SourceNode` it's handed.

---

Previous: [Visualising a grammar](10-visualising.md) · Back to the [index](../index.md)
