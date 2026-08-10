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
`src/*`, `queries/`, and `bindings/rust/visitor.rs` are regenerated every
time so they always track the current grammar, but `Cargo.toml`,
`bindings/rust/lib.rs`, and `examples/walk.rs` are only ever written once —
if they already exist, `scaffold` leaves them alone, so hand-written code
there survives a grammar change.

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
  hand-written `Visitor` implementations, and a rerun won't touch them:

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

## Typed node structs

The generated `Visitor` trait works with `SourceNode` — a thin wrapper
around `tree_sitter::Node` plus the source text — and
`node.kind()`/`children_by_field_name` reached through it — you get
traversal and dispatch, but node payloads stay stringly-typed. If you'd
rather have a real Rust struct per kind with typed field accessors, see
[type-sitter](https://github.com/Jakobeha/type-sitter), which generates those
from the same `node-types.json` the generated crate's `NODE_TYPES` constant
also embeds. The two approaches compose: a `Visitor` implementation can
construct type-sitter's typed wrappers from the `Node` inside the
`SourceNode` it's handed, combining the generated crate's traversal skeleton
with type-sitter's typed payloads.

---

Previous: [Visualising a grammar](10-visualising.md) · Back to the [index](../index.md)
