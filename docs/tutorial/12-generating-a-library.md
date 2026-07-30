# Generating a processing library

`ts-bnf-tool library` goes one step further than `visitor`: instead of a
single trait file, it scaffolds a complete, self-contained Rust crate for
parsing and traversing a language — the tree-sitter parser, the same
ANTLR-style `Visitor<'tree>` trait [`visitor`](11-generating-visitors.md)
generates, and a runnable example. `cd` into the output directory and
`cargo run --example walk -- <file>` works immediately, with no edits.

```sh
ts-bnf-tool library grammar.bnf                        # crate in ./<name>
ts-bnf-tool library -o out/decls grammar.bnf            # crate in out/decls
ts-bnf-tool library --name decls grammar.bnf            # override the crate/grammar name
ts-bnf-tool library --no-header grammar.bnf             # suppress generated-file comments
```

Like `visitor`, `library` runs no static checks before generating —
diagnostics never gate output — but it does run the same collision
pre-flight `visitor` runs internally: two rule names that would generate the
same `visit_*` method (see [Generating visitors](11-generating-visitors.md))
are rejected with a clear diagnostic before anything is written to disk.

## What gets generated

Using the same `decls.bnf` from the previous page:

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

`ts-bnf-tool library --name decls decls.bnf` creates a `decls/` directory:

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
output, unchanged. `library` adds the `bindings/rust/` and `examples/`
directories on top:

- **`bindings/rust/lib.rs`** — the parser bindings (`LANGUAGE`, `NODE_TYPES`,
  same shape as any `tree-sitter generate`-produced crate), a `pub mod
  visitor;`, and a `parse` convenience function:

  ```rust
  pub fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
      let mut parser = tree_sitter::Parser::new();
      parser.set_language(&LANGUAGE.into())?;
      parser
          .parse(source, None)
          .ok_or_else(|| "tree-sitter failed to parse the given source".into())
  }
  ```

- **`bindings/rust/visitor.rs`** — the same generated `Visitor<'tree>` trait
  `ts-bnf-tool visitor` would produce for this grammar.

- **`examples/walk.rs`** — a small program that parses a file and counts its
  nodes, implementing nothing but the trait's one required method:

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
  every node in the tree without touching a single per-kind method.

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

From here, `examples/walk.rs` is a normal part of the crate: extend
`Counter`, or write your own `Visitor` implementation the same way the
[worked example](11-generating-visitors.md#a-real-use-extracting-declared-names)
on the previous page does — the generated `parse` function and `Visitor`
trait are exactly what that example already builds on.

---

Previous: [Generating visitors](11-generating-visitors.md) · Back to the [index](../index.md)
