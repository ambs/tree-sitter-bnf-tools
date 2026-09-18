---
title: Generating a Scaffold
nav_order: 12
---

# Generating a processing scaffold

## What `scaffold` is for

Writing a tree-sitter-backed language tool by hand takes several
steps, from running `tree-sitter generate`, creating the library (in
the case of Rust, the crate) around the generated C parser, and
finally, writing the traversal code, that walks the tree to produce
your desired result.

`ts-bnf-tool scaffold` does all of that for you. Point it at a `.bnf`
grammar, and it produces a complete, self-contained Rust crate for parsing
and traversing the described language. The crate includes:

- the tree-sitter parser;
- an ANTLR-style `Visitor<'tree>` trait: one `visit_*` method per node
  kind, a `visit()` dispatcher, and a `combine`-based fold so you only
  write the bodies you care about;
- a runnable example (just `cd` into the output directory and run
  `cargo run --example walk -- <file>`);
- optionally, one typed Rust struct per grammar rule (`--ast-types`), so
  you can traverse an owned syntax tree instead of tree-sitter nodes.

This is a Rust-only feature for now. The subcommand's name is deliberately
target-language-neutral: a future target might scaffold a module or package
instead of a crate.

This chapter is a walkthrough: seven steps that take you from a `.bnf`
file to a crate with five working programs in it. Everything it skips
over (the full file-by-file breakdown, every flag, `--merge-config`,
rerun semantics) is in [Scaffold reference](12-scaffold-reference.md).

## Prerequisites

`scaffold` calls directly the `tree-sitter` CLI to generate the C parser.
The generated crate's `build.rs` then compiles `src/parser.c` with a C
compiler, the first time you `cargo build`/`cargo run` it.

Thus, beyond `ts-bnf-tool` itself, you need:

- **`tree-sitter-cli` >= 0.27 on `PATH`** (`npm install -g tree-sitter-cli`).
  The generated crate targets ABI 15, which requires that version. Without
  it, `scaffold` fails with `` `tree-sitter` not found on PATH ``.
- **A working C compiler** (`cc`, `gcc` or `clang`). Without it,
  `cargo build` and `cargo run` will fail compiling `src/parser.c`.
- **`ts-bnf-tool` itself, installed and on `PATH`.** The generated
  `Makefile`'s `generate` target calls it directly (`$(BNF_TOOL)
  scaffold .`), not through `cargo run`. If it isn't installed
  globally, override `BNF_TOOL`: `make BNF_TOOL=/path/to/ts-bnf-tool
  generate`.

## Step 1 — scaffold a crate

The recommended workflow is **in-place**: create the crate's folder first,
put the grammar inside it, then point `scaffold` at that same folder.
Source and destination end up being the same file, so there's only one
copy of the grammar to keep in sync.

```sh
mkdir decls && cd decls
```

Save this tiny declaration language as `decls.bnf` inside the folder
you just created:

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

This grammar describes programs made of `name = value;`
declarations. A `program` is zero or more `decl`. Each `decl` names a
`target` identifier and gives it a `value`, which is either another
identifier or a number.

Now scaffold it, in place. Still inside `decls/`:

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

For the rest of this chapter you only need to know three of those:

- **`decls.bnf`** is the source of truth. Edit it, rerun `scaffold`.
- **`bindings/rust/visitor.rs`** holds the generated `Visitor` trait. It
  is regenerated on every run, so never hand-edit it.
- **`examples/`** is yours. Drop a `.rs` file in and Cargo picks it up
  with no `Cargo.toml` change.

The other files, and exactly which ones a rerun overwrites, are covered in
[What `scaffold` creates](12-scaffold-reference.md#what-scaffold-creates).

## Step 2 — run the bundled example

Before writing any code of your own, check that the crate works.
`examples/walk.rs` came with it, so this step needs no edits at all
(still inside `decls/`):

```sh
$ printf 'x = 1;\ny = x;\n' > sample.decls
$ cargo run --example walk -- sample.decls
sample.decls: 9 node(s)
```

The first run takes a moment: that's `build.rs` compiling `src/parser.c`.

That count is the root `program` node plus two `decl`s, each contributing
itself, its `target` `ident`, its `value`'s `expr` wrapper, and the
`ident`/`num` inside that. Four nodes per `decl` makes 8, and `program`
itself brings the total to 9.

Keep `sample.decls` around; Step 7 reuses it.

## Step 3 — write your first visitor

Open `bindings/rust/visitor.rs` to see the generated trait. Before
anything compiles, every `Visitor` implementation must set two associated
types and implement one method:

```rust
impl<'t> Visitor<'t> for MyVisitor {
    type Output = /* what one visit_* call produces */;
    type Error = /* what can go wrong */;

    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error> {
        /* fold `results`, one child's Output each, into this node's own Output */
    }
}
```

Rust has no way to give `type Output` a default that an implementor can
skip. You must set it yourself, every time. There's no single right
choice: it depends on what your visitor computes.

Some examples:

| If your visitor... | use `Output = ` |
|---|---|
| only has a side effect (counting, printing, filling in a field on `self`) | `()` |
| collects a value from every node it visits | `Vec<T>` |
| looks for the first matching node and stops | `Option<T>` |
| builds something else | a custom type |

If your visitor can't fail, use `Error =
std::convert::Infallible`. Step 6 shows one that can.

`combine` is the one method every implementation must write. Tree-sitter
hands you a node's children one at a time; `combine` is where their
`Output`s get folded into that node's own `Output`. Every other method in
the trait already has a sensible default body, so a new visitor can start
by just defining `combine`.

That's easiest to see with a visitor whose `Output` is real data flowing
bottom-up, not a side effect. Here's `CollectText`, which gathers every
leaf's own source text into a `Vec<String>`:

```rust
use decls::visitor::{SourceNode, Visitor};

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

fn main() {
    let source = "x = 1;\ny = x;\n";
    let tree = decls::parse(source).expect("parse must succeed");
    let root = SourceNode { node: tree.root_node(), source };
    println!("{:?}", (CollectText).visit(root).unwrap());
}
```

Save that as `examples/collect_text.rs`, then run it:

```sh
$ cargo run --example collect_text
["x", "1", "y", "x"]
```

`visit_ident` and `visit_num` are overridden because `ident` and `num`
are **leaves**: single tokens with nothing underneath. A leaf's generated
default contributes nothing, so overriding it is how a token's text gets
into a result at all. (The generated file marks leaves explicitly; see
[Leaves](12-scaffold-reference.md#leaves).)

Here is why that's the output, step by step, for the two-line source
`x = 1;\ny = x;`:

1. First `decl` (`x = 1;`):
   - `target` is an `ident`. `visit_ident` is overridden, so it returns
     `vec!["x".to_string()]` directly, with no `combine` call at all.
   - `value` is an `expr` wrapping a `num`. `expr` isn't overridden, so
     its default `children_visitor` visits that one child; `visit_num`
     is overridden too, returning `vec!["1".to_string()]`.
     `children_visitor` folds that single result:
     `combine(vec![vec!["1".to_string()]])` gives `vec!["1".to_string()]`.
     That's `expr`'s own `Output`.
   - `decl` isn't overridden either. Its `children_visitor` now holds
     `target`'s `vec!["x".to_string()]` and `value`'s
     `vec!["1".to_string()]`, and folds them:
     `combine(vec![vec!["x".to_string()], vec!["1".to_string()]])` gives
     `vec!["x".to_string(), "1".to_string()]`. That's the first `decl`'s
     `Output`.
2. Second `decl` (`y = x;`) goes through the same steps. Its `value`
   wraps an `ident` this time instead of a `num`, but the fold is
   identical: `target` gives `vec!["y".to_string()]` directly, `expr`
   folds down to `vec!["x".to_string()]`, and `decl` folds both into
   `vec!["y".to_string(), "x".to_string()]`.
3. `program` holds both `decl`s. Its own default `children_visitor`
   folds their two `Output`s the same way, concatenating
   `vec!["x".to_string(), "1".to_string()]` with
   `vec!["y".to_string(), "x".to_string()]`. That last `Vec<String>` is
   the whole program's `Output`, and it is what
   `cargo run --example collect_text` printed above as
   `["x", "1", "y", "x"]`.

Every one of those `combine` calls is doing real, inspectable work:
concatenating its children's data into its own. Compare that to the
`Counter` inside the `examples/walk.rs` you ran in Step 2, which ignores
`results` entirely and increments a field instead:

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

`Counter` still counts every visited node correctly, but only as a
byproduct of *when* `combine` is called: once per visited node, by
construction of `children_visitor`/`default_result`, not of what it
computes. If `children_visitor`'s fold strategy ever changed, that
byproduct could change with it. `CollectText` is what `combine` is
actually for: folding children's data into a parent's, on purpose.

## Step 4 — visit one field only

A grammar rule can label its children with **fields**.
`decl -> target: ident '=' value: expr ';'` names its `ident` child
`target` and its `expr` child `value`. Fields let you refer to "the
`target` of a `decl`" by name instead of by position, both from Rust and
from tree-sitter's own tooling (queries, `children_by_field_name`).

`decl` has two fields, so its generated doc comment lists both:

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

Read that comment carefully, because it is easy to misread. The `via ...`
part of each line does **not** describe what the default body does. The
default body is the last line of the block: `self.children_visitor(node)`,
which visits *every* named child in one pass, paying no attention to
fields at all. What the `via ...` part tells you is how to visit **just
that one field**, if you ever want to. It is a recipe you can copy into an
override of your own.

You already have the "default" half of this comparison running:
`CollectText` never overrode `visit_decl`, so every `decl` used
`children_visitor` and the output kept both fields, `["x", "1", "y", "x"]`.
All you need to see the contrast, then, is that same visitor plus the one
override. Start from a copy:

```sh
cp examples/collect_text.rs examples/fields.rs
```

Then, in the copy, add one method inside the existing `impl` block, just
after `visit_num`:

```rust
    // The one addition: visit the `target` field only, never `value`.
    fn visit_decl(&mut self, node: SourceNode<'t>) -> Result<Self::Output, Self::Error> {
        self.field_visitor(node, "target")
    }
```

That is the whole change. Nothing else in the file moves. Run it:

```sh
$ cargo run --example fields
["x", "y"]
```

`1` is gone, and so is the `x` on the right of `y = x;`. Compare that to
the `["x", "1", "y", "x"]` that `cargo run --example collect_text` still
prints: same grammar, same input, same `combine`, same leaf methods. The
single added method is the entire difference.

Here's what it did. `field_visitor(node, "target")` walks only the
children sitting in the `target` field, which for a `decl` is exactly one
`ident`, and folds their `Output`s through `combine`, just as
`children_visitor` does for every named child. Because the override
replaced the default body, `value` is never visited at all.

Two details worth keeping in mind for your own visitors:

- Skipping a field skips its **whole subtree**. `visit_expr` and
  `visit_num` are now never called at all, so `visit_num` sits in your
  copy as dead code, and the `x` in `y = x;` never reaches `visit_ident`.
  If a visitor ignores a node, suspect an override *above* it.
- If you ask for a field the node doesn't have, `field_visitor` doesn't
  fail. It returns `default_result()`, which is `combine(vec![])`, so for
  a `Vec<String>` output that's simply an empty vector.

## Step 5 — extract declared names

Step 4's two overrides already compute something useful: every name
declared by a `decls.bnf` program, and not a name that only appears on the
right-hand side of `=`. Here they get a purpose-built name, a comment on
every part, and an assertion pinning the result down, so the file stands
on its own as a starting point for your own visitors.

The two overrides, restated:

- `visit_ident` captures a leaf's own text.
- `visit_decl` visits *only* its `target` field, skipping `value`
  entirely. That's what stops a name used inside an expression from being
  collected as if it were a declaration.

One thing worth noticing before you read it. Every `visit_*` method
receives a `SourceNode`, not a bare `tree_sitter::Node`. `SourceNode`
bundles the node with the source text it was parsed from, so `node.text()`
(a method only `SourceNode` has) returns that node's own text directly,
with no source-slicing to do yourself.

Save this as `examples/decl_extractor.rs`:

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

    // Visit `target` only, never `value`, so a name used as a value
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
    // source text: `Visitor::visit` needs a `SourceNode`, not a bare `Node`.
    let root = SourceNode { node: tree.root_node(), source };
    let names = extractor.visit(root).unwrap();
    assert_eq!(names, vec!["x", "y"]);
}
```

`visit_program`'s default body (`children_visitor`) already does the right
thing: it visits every `decl`, and `combine`'s `flatten` concatenates their
results. Given `x = 1;` then `y = x;`, `DeclExtractor` returns
`["x", "y"]`: the declared names, not `x`'s later use as a value.

Run it:

```sh
cargo run --example decl_extractor
```

It exits silently if the extracted names match, and panics on its own
`assert_eq!` otherwise.

## Step 6 — a visitor that can fail

Every visitor so far has used `Error = std::convert::Infallible`: a
visitor that can't fail. Real ones often can. `decls.bnf`'s
`num -> /[0-9]+/` puts no upper bound on how many digits a number literal
has, so a visitor that turns that text into an `i64` has a genuine
failure case: a literal with more digits than an `i64` can hold.

`SumValues` sums every number literal in a program, and fails, naming
the offending literal, the moment one doesn't fit. Save it as
`examples/sum_values.rs`:

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

Run it:

```sh
$ cargo run --example sum_values
"x = 1;\ny = 2;\n": sum = 3
"x = 99999999999999999999;\n": `99999999999999999999` doesn't fit in an i64
```

## Step 7 — generate typed structs with `--ast-types`

Everything so far went through `SourceNode`: a thin wrapper around
`tree_sitter::Node` plus the source text. It gives you traversal and
`node.text()`, but a node's payload stays stringly-typed. There is no Rust
struct shaped like your grammar, so nothing stops you from asking a `num`
for its `target` field and getting `None` at runtime.

`--ast-types` adds that missing layer. Rerun `scaffold` with the flag,
still inside `decls/`:

```sh
ts-bnf-tool scaffold --ast-types -o . decls.bnf
```

Three things appear:

- `bindings/rust/ast.rs`, with one owned Rust struct per grammar rule.
- `examples/ast.rs`, a runnable dumper, written once like `walk.rs`.
- a `pub mod ast;` line inserted into your existing `bindings/rust/lib.rs`.
  That last one is the single exception to `lib.rs` being yours alone: the
  new `examples/ast.rs` wouldn't compile without it. Nothing you added is
  removed.

Run the new example on the sample from Step 2:

```sh
$ cargo run --example ast -- sample.decls
sample.decls:
Program {
    _pragma: Pragma {
        line: 1,
        column: 1,
    },
}
```

That is almost certainly not what you expected: the two declarations are
missing. Nothing is broken. **Only labelled fields become struct fields**,
and `program -> decl* ;` never labelled anything. `_pragma`, which records
the node's start line and column, is injected by the tool, so it's all
`Program` has.

So label it. Edit `decls.bnf` and give that repetition a name:

```bnf
program -> items: decl* ;
```

Then regenerate. The scaffolded `Makefile` has a target for exactly this,
and it needs no flags: `ts-bnf-tool.toml` already recorded `--ast-types`
from the run above.

```sh
$ make generate
ts-bnf-tool scaffold .
```

Now run the dumper again, on a one-line input to keep the output short:

```sh
$ printf 'x = 1;\n' > one.decls
$ cargo run --example ast -- one.decls
one.decls:
Program {
    _pragma: Pragma {
        line: 1,
        column: 1,
    },
    items: [
        Decl {
            _pragma: Pragma {
                line: 1,
                column: 1,
            },
            target: Ident {
                _pragma: Pragma {
                    line: 1,
                    column: 1,
                },
                _text: "x",
            },
            value: Expr {
                _pragma: Pragma {
                    line: 1,
                    column: 5,
                },
            },
        },
    ],
}
```

That's the whole grammar as owned Rust values. `items` is a `Vec<Decl>`,
`target` is an `Ident`, and `Ident`, being a leaf, carries its token in an
injected `_text: String`. No per-kind printing code was involved: every
generated type derives `Debug`, so `{:#?}` does the rest.

`Expr` is still empty, and now you know why: `expr -> ident | num ;`
labels neither alternative, exactly as `program` didn't label `decl*`.
Labelling them is a good exercise. Note also that a rerun of
`make generate` is a no-op until `decls.bnf` changes again.

Two rules to carry away:

- `ast.rs` is regenerated in full on every run, like `visitor.rs`. To add
  behaviour to a generated type, wrap it in your own struct; don't edit
  the generated file.
- `--ast-types` is a one-way switch. A later rerun can add it, but can't
  take it back off.

The struct-by-struct breakdown, the reserved `_`-prefixed names, and
`--merge-config` (which collapses related kinds into one enum) are in
[Typed node structs](12-scaffold-reference.md#typed-node-structs---ast-types).

## Where to go next

You now have a crate with five programs in it: the bundled `walk` and
`ast`, plus `collect_text`, `fields`, `decl_extractor`, and `sum_values`
that you wrote. From here, the workflow is always the same: edit
`decls.bnf`, run `make generate`, rerun your examples.

[Scaffold reference](12-scaffold-reference.md) covers what this
walkthrough skipped: every generated file and whether a rerun overwrites
it, the remaining command-line flags, the full `Visitor` trait including
its ANTLR correspondence, what happens when you rerun against a changed
or different grammar, and `--merge-config`.

---

Previous: [Visualising a grammar](10-visualising.md) · Next: [Scaffold reference](12-scaffold-reference.md) · Back to the [index](../index.md)
