---
title: Generating a Scaffold
nav_order: 12
---

# Generating a processing scaffold

## What `scaffold` is for

Writing a tree-sitter-backed language tool by hand takes several steps. You
run `tree-sitter generate`. You wire up a Rust crate around the generated C
parser. You write a traversal that walks the tree without missing a node
kind.

`ts-bnf-tool scaffold` does all of that for you. Point it at a `.bnf`
grammar, and it produces a complete, self-contained Rust crate for parsing
and traversing the described language. The crate includes:

- the tree-sitter parser
- an ANTLR-style `Visitor<'tree>` trait — one `visit_*` method per node
  kind, a `visit()` dispatcher, and a `combine`-based fold so you only
  write the bodies you care about
- a runnable example

`cd` into the output directory and run `cargo run --example walk -- <file>`.
It works immediately — no edits needed.

This is a Rust-only feature for now. The subcommand's name is deliberately
target-language-neutral: a future target might scaffold a module or package
instead of a crate.

## Prerequisites

`scaffold` shells out to the `tree-sitter` CLI to generate the C parser.
The generated crate's `build.rs` then compiles `src/parser.c` with a C
compiler, the first time you `cargo build`/`cargo run` it.

Beyond `ts-bnf-tool` itself, you need:

- **`tree-sitter-cli` >= 0.25 on `PATH`** (`npm install -g tree-sitter-cli`).
  The generated crate targets ABI 15, which requires that version. Without
  it, `scaffold` fails with `` `tree-sitter` not found on PATH ``.
- **A working C compiler** (`cc`/`gcc`/`clang`). Without it, `cargo build`/
  `cargo run` fails compiling `src/parser.c`.
- **`ts-bnf-tool` itself, installed and on `PATH`.** The generated
  `Makefile`'s `generate` target calls it directly (`$(BNF_TOOL) scaffold
  .`), not through `cargo run`. If it isn't installed globally, override
  `BNF_TOOL`: `make BNF_TOOL=/path/to/ts-bnf-tool generate`.

## Basic usage

The best way to see what `scaffold` does is to run it. The recommended
workflow is **in-place**: create the crate's folder first, put the grammar
inside it, then point `scaffold` at that same folder. Source and
destination end up being the same file, so there's only ever one copy of
the grammar to keep in sync.

```sh
mkdir decls && cd decls
```

Save this tiny declaration language as `decls.bnf` (you're now inside
`decls/`, so this is `decls/decls.bnf` from outside it):

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

It describes programs made of `name = value;` declarations. A `program` is
zero or more `decl`s. Each `decl` names a `target` identifier and gives it
a `value`, which is either another identifier or a number.

Now scaffold it, in place — from inside `decls/`:

```sh
ts-bnf-tool scaffold -o . decls.bnf
```

This fills in the `decls/` directory around the grammar you just wrote:

```
decls/
├── .gitignore
├── Cargo.toml
├── Makefile
├── ts-bnf-tool.toml
├── decls.bnf
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

### What `scaffold` creates

The table below lists every file, what it's for, and whether `scaffold`
ever touches it again after the first run — which answers a question that
comes up immediately: **is it safe to hand-edit this file?**

| File / directory | What it is | Can you edit it? |
|---|---|---|
| `decls.bnf` | The grammar you wrote. Always the source of truth. | **Yes** — edit this, then rerun `scaffold`. |
| `grammar.js`, `src/` | The tree-sitter grammar and C parser. Files `grammar.js` and folder `src/` are exactly what `convert --generate` already produces — the real `tree-sitter generate` output, unchanged. | No — regenerated on every rerun. |
| `tree-sitter.json` | Tree-sitter's own package metadata file. | Written once, then left alone. Edit freely. |
| `queries/highlights.scm` | A starter syntax-highlighting query. | Written once, then left alone. Edit/extend freely (see [Keeping the grammar in sync](#keeping-the-grammar-in-sync) below). |
| `ts-bnf-tool.toml` | Records how the crate was scaffolded: the bundled grammar's filename, the crate name, and which flags were used. | Written once; a rerun updates only the fields matching a flag you actually pass. |
| `Makefile` | A `generate` target that reruns `scaffold` for you. | Written once. Add your own targets. |
| `Cargo.toml` | The crate manifest. | Written once. Edit freely. |
| `bindings/rust/build.rs` | Compiles `src/parser.c` (and `src/scanner.c`, if present). | No — regenerated on every rerun. |
| `bindings/rust/lib.rs` | Parser bindings (`LANGUAGE`, `NODE_TYPES`), plus a `parse` convenience function. | Written once. **This is where you add `pub mod` for your own `Visitor` implementations.** |
| `bindings/rust/visitor.rs` | The generated `Visitor<'tree>` trait (described below). | **No — always regenerated. Never hand-edit this file.** Write your own visitor in a different file instead, and register it in `lib.rs`. |
| `examples/walk.rs` | A small program that parses a file and counts its nodes, implementing nothing but the trait's one required method (described below). | Written once. Edit it, or drop a new file into `examples/` — Cargo picks those up automatically. |
| `.gitignore` | Ignores `/target`. | Written once. Extend freely. |

`lib.rs`'s `parse` function looks like this:

```rust
pub fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&LANGUAGE.into())?;
    parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse the given source".into())
}
```

One exception to `lib.rs`'s "written once" rule: if you scaffolded without
`--ast-types` and later rerun with it added, the rerun still inserts the
one line it needs (`pub mod ast;`) into your existing `lib.rs` — otherwise
the newly generated `examples/ast.rs` couldn't even compile. It never
removes anything you've added yourself.

### Other ways to invoke it

You aren't limited to the defaults used above:

```sh
ts-bnf-tool scaffold grammar.bnf                # crate in ./<name>, name from the filename
ts-bnf-tool scaffold -o out/decls grammar.bnf   # crate in out/decls
ts-bnf-tool scaffold --name decls grammar.bnf   # override the crate/grammar name
ts-bnf-tool scaffold --no-header grammar.bnf    # suppress generated-file comments
```

`--name` also names the grammar in the generated trait's own doc comment.
It defaults to the input filename's stem — that's why the `decls.bnf`
example above needed no `--name` at all; its stem is already `decls`.

A hyphenated name (`my-lang`) is fine. `Cargo.toml`'s `[package] name`
keeps the hyphen, since that's Cargo's own convention. Everywhere else —
the tree-sitter grammar name, the generated C parser symbol, the module
path `examples/*.rs` imports — the hyphen becomes an underscore
(`my_lang`) instead, because tree-sitter's own `grammar()` call rejects a
hyphenated name outright.

A name still invalid after that substitution (a leading digit, whitespace,
…) is rejected up front, before anything is written to disk.

`scaffold` runs no static checks on the grammar before generating, same as
`railroad` and `graph` — diagnostics never gate its output. One exception:
it does check that no two rules would produce the same `visit_*` method
name (see below). A grammar that fails this check is rejected with a clear
diagnostic, again before anything is written to disk.

## The generated `Visitor` trait

Open `bindings/rust/visitor.rs` to see the generated trait. Remember: this
file is always regenerated, so don't edit it — see the table above.

### `Output`, `Error`, and `combine`

Before anything compiles, every `Visitor` implementation must set two
associated types and implement one method:

```rust
impl<'t> Visitor<'t> for MyVisitor {
    type Output = /* what one visit_* call produces */;
    type Error = /* what can go wrong */;

    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        /* fold `results` — one child's Output each — into this node's own Output */
    }
}
```

Rust has no way to give `type Output` a default that an implementor can
skip. You must set it yourself, every time. There's no single right
choice — it depends on what your visitor computes:

| If your visitor... | use `Output = ` |
|---|---|
| only has a side effect (counting, printing, filling in a field on `self`) | `()` |
| collects a value from every node it visits | `Vec<T>` |
| looks for the first matching node and stops | `Option<T>` |
| builds something else | a custom type |

If your visitor can't fail, use `Error = std::convert::Infallible`.

`combine` is the one method every implementation must write. tree-sitter
hands you a node's children one at a time; `combine` is where their
`Output`s get folded into that node's own `Output`. Every other method in
the trait already has a sensible default body, so a new visitor can start
with just `combine`.

### `combine` in practice

`combine`'s job is to fold each child's `Output` into this node's own
`Output`. That's easiest to see with a visitor whose `Output` is real
data flowing bottom-up, not a side effect. Here's `CollectText`, which
gathers every leaf's own source text into a `Vec<String>`:

```rust
struct CollectText;

impl<'t> Visitor<'t> for CollectText {
    type Output = Vec<String>;
    type Error = std::convert::Infallible;

    // Every child already produced its own Vec<String>; concatenate them
    // into this node's.
    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        Ok(results.into_iter().flatten().collect())
    }

    // A leaf has no children to fold through `combine`; it *is* the data.
    fn visit_ident(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        Ok(vec![node.text().to_string()])
    }

    fn visit_num(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        Ok(vec![node.text().to_string()])
    }
}
```

Trace it over `x = 1;`, following the actual `Vec<String>` values, not
just how many times `combine` happens to run:

1. `target` is an `ident`. `visit_ident` is overridden, so it returns
   `vec!["x".into()]` directly, with no `combine` call at all.
2. `value` is an `expr` node, which isn't overridden. Its default body,
   `children_visitor`, visits `expr`'s one child, the `num` leaf.
   `visit_num` is overridden too, and returns `vec!["1".into()]` directly,
   the same way.
3. Back in `children_visitor(expr_node)`, that one child result gets
   folded: `combine(vec![vec!["1".into()]])` → `vec!["1".into()]`. That's
   `expr`'s own `Output`.
4. `visit_decl` isn't overridden either, so `children_visitor(decl_node)`
   now holds two child results: `target`'s `vec!["x".into()]` and
   `value`'s `vec!["1".into()]`. It folds them:
   `combine(vec![vec!["x".into()], vec!["1".into()]])` →
   `vec!["x".into(), "1".into()]`. That's `decl`'s own `Output`.
5. `program` holds two `decl`s. Its own default `children_visitor` folds
   their two `Output`s the same way, giving `vec!["x", "1", "y", "x"]` as
   the whole program's `Output`, for the two-line source
   `x = 1;\ny = x;`.

Every one of those `combine` calls is doing real, inspectable work:
concatenating its children's data into its own. Compare that to
`examples/walk.rs`'s `Counter`, which ignores `results` entirely and
increments a field instead:

```rust
struct Counter {
    total: usize,
}

impl<'t> Visitor<'t> for Counter {
    type Output = ();
    type Error = std::convert::Infallible;

    fn combine(&mut self, _results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        self.total += 1;
        Ok(())
    }
}
```

`Counter` still ends up counting every visited node correctly (see
["Running it"](#running-it) below), but only as a byproduct of *when*
`combine` is called: once per visited node, by construction of
`children_visitor`/`default_result`, not of what it computes. If
`children_visitor`'s fold strategy ever changed, that byproduct could
change with it. `CollectText` above is what `combine` is actually for:
folding children's data into a parent's, on purpose.

### One method per grammar rule

For `decls.bnf` the trait has one method per kind (`visit_program`,
`visit_decl`, `visit_expr`, `visit_ident`, `visit_num`), the `visit()`
dispatcher, and the five ANTLR-mirroring helper methods above.

A grammar rule can label its children with **fields**.
`decl -> target: ident '=' value: expr ';'` names its `ident` child
`target` and its `expr` child `value`. Fields let you refer to "the
`target` of a `decl`" by name instead of by position, both from Rust
(via `field_visitor`, described below) and from tree-sitter's own tooling
(queries, `children_by_field_name`). `decl` has two fields, so its
generated doc comment lists both, alongside the method its *default* body
actually calls for each:

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

That comment can be misread as saying the default body calls
`field_visitor`. It doesn't: `children_visitor` visits every named child
in one pass, regardless of field. The "via ..." line instead tells you
what visiting *just* that field would look like, because that's exactly
what you can do yourself, by overriding `visit_decl`:

```rust
fn visit_decl(&mut self, node: SourceNode<'tree>) -> Result<Self::Output, Self::Error> {
    self.field_visitor(node, "target") // visit only `target`; `value` is skipped entirely
}
```

`field_visitor(node, "target")` visits only the children in the `target`
field (here, one `ident`) and folds their `Output`s through `combine`,
the same way `children_visitor` does for *every* named child. Overriding
`visit_decl` like this, instead of the default `children_visitor`, is
exactly how the `DeclExtractor` example further down collects only the
name being *declared* from each `decl`, ignoring the value to the right
of `=`.

`ident` and `num` have no visible children of their own — each is a
single token, with no substructure at all. So they're leaves, and there's
no "Anonymous children" section either:

```rust
/// Visits a `ident` node.
///
/// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
fn visit_ident(&mut self, node: SourceNode<'tree>) -> Result<Self::Output, Self::Error> {
    let _ = node;
    self.default_result()
}
```

### ANTLR correspondence

If you've used ANTLR's `AbstractParseTreeVisitor`, this should feel
familiar. The trait's own doc comment includes this table:

| ANTLR (`AbstractParseTreeVisitor`) | Here                                                            |
|-------------------------------------|-----------------------------------------------------------------|
| `visit(tree)`                        | `Visitor::visit`                                                 |
| `visitChildren(node)`                | `Visitor::children_visitor`                                      |
| `aggregateResult(agg, next)`         | `Visitor::combine` (`Vec`-based, not pairwise)                   |
| `defaultResult()`                    | `Visitor::default_result` (`= combine(vec![])`)                  |
| `visitErrorNode(node)`               | `Visitor::error_visitor`                                          |
| (no ANTLR analogue)                  | `Visitor::missing_visitor`, for tree-sitter's `MISSING` nodes    |

One thing has no ANTLR equivalent: `missing_visitor`. tree-sitter's error
recovery can insert a zero-width `MISSING` node, standing in for a token
the parser expected but never found. `Node::kind()` reports the
*expected* kind on that node — not a distinct "missing" kind. So `visit()`
checks `Node::is_missing()` first, before matching on kind, and routes
there instead.

## Running it

Try it — no edits needed (still inside `decls/`):

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

Say you want every name declared by a `decls.bnf` program — but not a name
that only appears on the right-hand side of `=`.

Two overrides do it:

- `visit_ident` captures a leaf's own text.
- `visit_decl` visits *only* its `target` field, skipping `value`
  entirely. That's what stops a name used inside an expression from being
  collected as if it were a declaration.

Save this as `examples/decl_extractor.rs` inside `decls/`. Cargo picks up
any file dropped into `examples/` automatically, so this needs no
`Cargo.toml` change — it lives alongside `walk.rs` as a second example.

Every `visit_*` method receives a `SourceNode`, not a bare
`tree_sitter::Node`. `SourceNode` bundles the node with the source text it
was parsed from, so `node.text()` — a method only `SourceNode` has —
returns that node's own text directly, with no source-slicing to do
yourself:

```rust
use decls::visitor::{SourceNode, Visitor};

// No fields: DeclExtractor holds no state of its own. Everything it needs
// comes from the tree it's visiting, not from `self`.
struct DeclExtractor;

impl<'t> Visitor<'t> for DeclExtractor {
    // Each visit_* call returns the names it found, so far, as a Vec.
    type Output = Vec<String>;
    // This visitor can't fail, so Error is the "can't happen" type.
    type Error = std::convert::Infallible;

    // A node's own names are its children's names, concatenated. `results`
    // holds one Vec<String> per visited child; flatten them into one.
    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        Ok(results.into_iter().flatten().collect())
    }

    // `ident` is a leaf: its own text *is* the name, so just return it.
    fn visit_ident(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        Ok(vec![node.text().to_string()])
    }

    // Visit `target` only — never `value` — so a name used as a value
    // (the right-hand side of `=`) is never collected as a declaration.
    fn visit_decl(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        self.field_visitor(node, "target")
    }
}

fn main() {
    let source = "x = 1;\ny = x;\n";
    let tree = decls::parse(source).expect("parse must succeed");
    let mut extractor = DeclExtractor;
    // `parse` only returns a `Tree`. Bundle its root `Node` with the
    // source text — `Visitor::visit` needs a `SourceNode`, not a bare `Node`.
    let root = SourceNode { node: tree.root_node(), source };
    let names = extractor.visit(root).unwrap();
    assert_eq!(names, vec!["x", "y"]);
}
```

`visit_program`'s default body (`children_visitor`) already does the right
thing: it visits every `decl`, and `combine`'s `flatten` concatenates their
results. Given `x = 1;` then `y = x;`, `DeclExtractor` returns
`["x", "y"]` — the declared names, not `x`'s later use as a value.

Run it from inside `decls/`:

```sh
cargo run --example decl_extractor
```

It exits silently if the extracted names match, and panics on its own
`assert_eq!` otherwise.

## A visitor that can fail

Every visitor so far has used `Error = std::convert::Infallible`: a
visitor that can't fail. Real ones often can. `decls.bnf`'s
`num -> /[0-9]+/` puts no upper bound on how many digits a number literal
has, so a visitor that turns that text into an `i64` has a genuine
failure case: a literal with more digits than an `i64` can hold.

`SumValues` sums every number literal in a program, and fails, naming
the offending literal, the moment one doesn't fit:

Save this as `examples/sum_values.rs` inside `decls/`:

```rust
use decls::visitor::{SourceNode, Visitor};

// The one way this visitor can fail: a `num` literal too big for `i64`.
#[derive(Debug)]
struct TooBig(String);

struct SumValues;

impl<'t> Visitor<'t> for SumValues {
    type Output = i64;
    type Error = TooBig;

    // Every visited node contributes a partial sum. A node with no `num`
    // among its descendants contributes 0: `results` is either empty (a
    // leaf other than `num`) or holds children whose own sums were 0.
    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        Ok(results.into_iter().sum())
    }

    // The only place a nonzero value, or a failure, can originate.
    fn visit_num(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        node.text()
            .parse()
            .map_err(|_| TooBig(node.text().to_string()))
    }
}

fn main() {
    for source in ["x = 1;\ny = 2;\n", "x = 99999999999999999999;\n"] {
        let tree = decls::parse(source).expect("parse must succeed");
        let root = SourceNode { node: tree.root_node(), source };
        match (SumValues).visit(root) {
            Ok(sum) => println!("{source:?}: sum = {sum}"),
            Err(TooBig(text)) => println!("{source:?}: `{text}` doesn't fit in an i64"),
        }
    }
}
```

`visit_num` is the only method that ever returns a nonzero `Output` or an
`Err`. Every other kind falls back to the default `children_visitor`/
`default_result` chain, and `combine`'s `sum()` just propagates whatever
its children produced. There's no explicit error-checking in `visit_decl`
or `visit_program` because none is needed: `children_visitor`'s own loop
(see the generated `visitor.rs`) uses `?` on each child's result, so the
first `num` that fails to parse stops the walk right there and returns
that `Err`; `combine` for any of its ancestors never runs.

Run it (still inside `decls/`):

```sh
$ cargo run --example sum_values
"x = 1;\ny = 2;\n": sum = 3
"x = 99999999999999999999;\n": `99999999999999999999` doesn't fit in an i64
```

## Keeping the grammar in sync

You've now hand-edited the scaffolded crate, by adding
`examples/decl_extractor.rs` and `examples/sum_values.rs`. You'll likely
want to register a hand-written `Visitor` in `lib.rs` too, eventually. So
it's worth knowing what a rerun does.

Editing `decls.bnf` and rerunning `scaffold` is safe: the
["What `scaffold` creates"](#what-scaffold-creates) table above already
says which files get regenerated and which don't. Your hand-written code
is never touched.

One exception worth calling out on its own: `queries/highlights.scm` is
written once, then frozen. It won't pick up new grammar rules by itself.
Regenerate it explicitly when the grammar gains rules you want
highlighted:

```sh
ts-bnf-tool highlights -o queries/highlights.scm decls.bnf
```

(Any hand-written refinements you've made survive a manual `highlights`
rerun the same way — see
[Refine the highlights skeleton](06-end-to-end.md#step-5--refine-the-highlights-skeleton).)

### `make generate`

The scaffolded `Makefile` wraps a rerun in one target. From inside
`decls/`:

```sh
$ make generate
```

This reruns `ts-bnf-tool scaffold .` whenever `decls.bnf` or
`ts-bnf-tool.toml` is newer than `bindings/rust/visitor.rs`. Otherwise
it's a no-op. From here on, the whole workflow is: edit `decls.bnf`, run
`make generate`.

### Rerunning without repeating anything

`ts-bnf-tool scaffold .` — what `make generate` runs — is worth knowing on
its own. Point `scaffold` at the crate's own *directory*, instead of its
grammar file, and it reruns using whatever `ts-bnf-tool.toml` already
recorded: the bundled grammar's filename, `--name`, `--ast-types`,
`--merge-config`. No flags needed.

Pass a flag anyway, and it overrides what's recorded — `ts-bnf-tool.toml`
is updated to match. One exception: `--ast-types` is a one-way switch. You
can add it on a later rerun, but never remove it.

### The mismatch guard

A crate remembers which grammar file it was bundled from. Point `scaffold`
at a *different* `.bnf` file for that same crate (rather than the
directory), and it refuses — it won't silently swap the bundled grammar or
ignore the new file:

```
$ ts-bnf-tool scaffold --output-dir decls other.bnf
error: a different grammar file ('decls.bnf') is already bundled in decls; scaffold again with that file, or remove/rename it first if you mean to replace it
```

This guard doesn't cover files pulled in via `%include` — those are freely
re-bundled as the include graph changes (see below).

### Bundling `%include`d files

If `decls.bnf` `%include`s another file, `scaffold` bundles the *whole*
include closure — not just the root file. Each included file lands at the
same path, relative to the crate root, that it had relative to the root
grammar's own directory. The `%include` directive itself is copied
unchanged; it already resolves correctly from the new location, since
`%include` paths are always relative to the file that names them.

Unlike the root grammar, included files aren't covered by the mismatch
guard above. The include graph is freely re-bundled every time it changes.

An `%include` that resolves *outside* the root grammar's own directory
tree is left external — not bundled, not rewritten. `scaffold` still
succeeds, but warns:

```
note: /path/outside/child.bnf is outside /path/to/decls; leaving it unbundled
```

### Non-in-place scaffolding

You can also point `scaffold` at an external `.bnf` file and a *fresh*
output directory — the original workflow, still fully supported. It
bundles a copy on that first run, and prints a note telling you which copy
is live from here on:

```
$ ts-bnf-tool scaffold --name decls --output-dir decls path/to/decls.bnf
bundled a copy of decls.bnf into decls; edit that copy from now on — path/to/decls.bnf is no longer read
```

From here on there are *two* copies of the grammar on disk: the original
external file, and the bundled one inside `decls/`. Only the bundled copy
is ever read again. The in-place workflow from the start of this chapter
avoids that split — source and destination are the same file.

## Typed node structs (`--ast-types`)

The generated `Visitor` trait works with `SourceNode`: a thin wrapper
around `tree_sitter::Node` plus the source text. You get traversal and
dispatch through it (`node.kind()`, `children_by_field_name`, and
`node.text()`), but a node's payload stays stringly-typed — there's no
Rust struct matching your grammar's shape.

`--ast-types` adds that, as a second, independent layer:
`bindings/rust/ast.rs`, one owned Rust struct per grammar rule. "Owned"
means no `'tree` lifetime survives construction — once built, a value
doesn't borrow from the parse tree anymore. Each struct gets a
`TryFrom<super::visitor::SourceNode<'tree>>` impl, and a `_pragma:
runtime::Pragma` field recording its start line/column. A leaf kind (one
with no visible children of its own) also gets a `_text: String` field.

`Pragma` and `BuildError` (the shared `TryFrom` error type) live inside an
inner `runtime` module, not at `ast.rs`'s own top level. And the two
injected fields start with an underscore. Both choices exist for the same
reason: so a grammar rule or field genuinely named `pragma`, `text`,
`build_error`, or `source_node` can never collide with these fixed,
tool-injected names — see "A note on vocabulary" below.

Try it — still inside `decls/`:

```sh
ts-bnf-tool scaffold --ast-types -o . decls.bnf
```

This adds two files on top of the tree shown earlier:

```
decls/
├── …
├── bindings/rust/
│   ├── ast.rs        (new)
│   ├── build.rs
│   ├── lib.rs
│   └── visitor.rs
└── examples/
    ├── ast.rs         (new)
    └── walk.rs
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

Like `visitor.rs`, `ast.rs` is always regenerated from the current grammar
— there's no hand-edit support for it. If you need more fields on a
generated type, wrap it in your own struct rather than editing the
generated file. A rerun of `scaffold --ast-types` overwrites `ast.rs` every
time, same as `visitor.rs`, while `examples/ast.rs` itself follows the
same write-once-then-leave-alone rule as `examples/walk.rs`.

**A note on vocabulary.** Grammar rule and field names are otherwise
completely unrestricted. `pragma`, `text`, `build_error`, `source_node` —
all plausible names a real grammar might want (a `pragma` directive rule, a
`text` leaf, a field called `text`) — are all fine. A kind named any of
these generates an ordinary top-level `struct Pragma`/`Text`/`BuildError`/
`SourceNode`, distinct from the tool's own fixed `runtime::Pragma`,
`runtime::BuildError`, and `super::visitor::SourceNode`.

The only real restriction is on field labels: one starting with `_` is
rejected at generation time. That whole leading-underscore namespace is
reserved for the fields this tool injects itself (`_pragma`, `_text`).

### Collapsing related kinds (`--merge-config`)

A grammar sometimes has several kinds that are really one construct, from
the caller's point of view. `for_statement`, `while_statement`, and
`repeat_statement` might all just be "a loop." Left alone, `--ast-types`
gives each its own, unrelated struct.

`--merge-config <path>`, passed alongside `--ast-types`, collapses a group
of kinds like that into one Rust `enum`. The enum's own variants
discriminate on the Rust type — no stringly-typed `kind` field needed.

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

- **`merge`** collapses every kind in `from` into one `enum` named
  `target`, one variant per source kind.
- **`passthrough`** renames a single kind's generated struct without
  otherwise changing it — here, `comment`'s struct is emitted as
  `DocComment` instead of the `Comment` its kind name would otherwise
  derive.
- **`ignore`** (not used above) explicitly marks a kind as "leave it as
  the default baseline struct" — see the coverage report below for why
  you'd write this out loud instead of just doing nothing.

Every `merge`/`passthrough` entry's `target` is emitted verbatim as a Rust
`struct`/`enum` name. It must be a valid, non-keyword Rust identifier —
`Loop` is fine; `loop`, `my-loop`, and an empty string aren't. `scaffold`
rejects an invalid config up front, rather than emit Rust that won't
compile.

One kind can't be merged away: the grammar's own root rule. The scaffolded
`examples/ast.rs` needs one concrete, nameable root type to construct
(`use {crate}::ast::{RootKind};`). A kind claimed by a `merge` entry
becomes a private variant of that entry's enum instead — not a usable
substitute. So a `merge` entry naming the root rule in its `from` list is
rejected up front, the same as an invalid `target`. To rename the root
rule's struct, use `passthrough` instead.

Suppose your grammar has a `program -> items: (for_statement | while_statement
| repeat_statement)* doc: comment ;` rule. Running
`ts-bnf-tool scaffold --ast-types --merge-config ast-merge.toml grammar.bnf`
with the config above generates:

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

`ForStatement`, `WhileStatement`, and `RepeatStatement` still exist. Each
still gets the ordinary `TryFrom` impl `--ast-types` always generates —
they're just no longer `pub`. `Loop`'s three variants are the only way
code outside the generated crate ever sees them. `Comment`'s struct
doesn't exist at all under that name; it's emitted as `DocComment`, per
the `passthrough` entry, `pub` like any ordinary kind.

**Coverage report.** A grammar can drift out of sync with its merge
config: a new rule gets added, and nobody's decided yet whether it should
merge, passthrough, or stay alone. So whenever `--merge-config` is passed,
`scaffold` prints one stderr line for every visible kind not named by any
`merge`, `passthrough`, or `ignore` entry:

```
warning: kind 'program' is not covered by --merge-config (no merge/passthrough/ignore entry); it will be emitted as an ordinary baseline struct
```

This is advisory only. It never affects the exit code, and the uncovered
kind still generates normally, as an ordinary `pub` struct — exactly as if
`--merge-config` hadn't been passed for that kind at all. Once you've
reviewed a grammar's kinds, and you're happy leaving the rest as ordinary
structs, silence the report with the wildcard `ignore` entry:

```toml
ignore = ["*"]

[[merge]]
target = "Loop"
from = ["for_statement", "while_statement", "repeat_statement"]
```

`ignore = ["*"]` must be the config's only `ignore` entry. It means "every
kind not otherwise claimed" — listing specific kinds alongside it would be
redundant at best, a likely typo at worst. `check_merge_config` rejects
that combination outright.

## Typed field accessors without ownership

Want typed field accessors directly over borrowed `tree_sitter::Node`s —
no owned copy, no `'tree`-free struct, no merge/collapse? See
[type-sitter](https://github.com/Jakobeha/type-sitter). It generates those
from the same `node-types.json` the generated crate's `NODE_TYPES`
constant also embeds.

It composes with the `Visitor` trait the same way it always has,
independently of `--ast-types`: a `Visitor` implementation can construct
type-sitter's typed wrappers from the `Node` inside the `SourceNode` it's
handed.

---

Previous: [Visualising a grammar](10-visualising.md) · Back to the [index](../index.md)
