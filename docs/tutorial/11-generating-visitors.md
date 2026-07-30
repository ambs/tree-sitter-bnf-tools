# Generating visitors

`ts-bnf-tool visitor` reads a `.bnf` file and emits a single Rust file
containing an ANTLR-style `Visitor<'tree>` trait: one `visit_*` method per
node kind, a central `visit()` dispatcher, and a `combine`-based fold so you
only write the bodies you care about. This page walks through the shape of
the generated trait and a small realistic use of it. This is a Rust-only
feature for now; the design deliberately leaves room for other target
languages later.

```sh
ts-bnf-tool visitor grammar.bnf                       # trait to stdout
ts-bnf-tool visitor -o visitor.rs grammar.bnf         # write to file
ts-bnf-tool visitor --name decls grammar.bnf          # override the doc comment's grammar name
ts-bnf-tool visitor --no-header grammar.bnf           # suppress the generated-file comment
```

`--name` only affects wording in the trait's own doc comment; it defaults to
the input filename's stem. Like `railroad` and `graph`, `visitor` runs no
static checks before generating — diagnostics never gate output.

## A worked example

Save this tiny declaration language as `decls.bnf`:

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

Running `ts-bnf-tool visitor --name decls decls.bnf` emits a trait with one
method per kind (`visit_program`, `visit_decl`, `visit_expr`, `visit_ident`,
`visit_num`), a `visit()` dispatcher matching on `node.kind()`, and five
ANTLR-mirroring helper methods with sensible default bodies. The `decl` kind
has two fields, so its doc comment lists both:

```rust
/// Visits a `decl` node.
///
/// **Fields:**
/// - `target` -> `ident` ([`Visitor::visit_ident`]), via `self.field_visitor(node, "target")`
/// - `value` -> `expr` ([`Visitor::visit_expr`]), via `self.field_visitor(node, "value")`
///
/// **Anonymous children** (not visited by default): `'='`, `';'`
fn visit_decl(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
    self.children_visitor(node)
}
```

`ident` and `num` have no visible children of their own, so they're leaves:

```rust
/// Visits a `ident` node.
///
/// **Anonymous children** (not visited by default): `/[a-z][a-zA-Z0-9_]*/`
///
/// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
fn visit_ident(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
    let _ = node;
    self.default_result()
}
```

## ANTLR correspondence

If you've used ANTLR's `AbstractParseTreeVisitor`, the shape should feel
familiar — the generated trait's own header doc comment includes this table:

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

## A real use: extracting declared names

Say you want the list of names declared by a `decls.bnf` program, ignoring
any identifiers that appear only on the right-hand side of `=`. Override
`visit_ident` to capture a leaf's own text, and override `visit_decl` to
visit *only* its `target` field — skipping `value` entirely, so a name used
inside an expression never gets collected as if it were a declaration:

```rust
use tree_sitter::Node;

struct DeclExtractor<'src> {
    source: &'src str,
}

impl<'t> Visitor<'t> for DeclExtractor<'t> {
    type Output = Vec<String>;
    type Error = std::convert::Infallible;

    fn combine(&mut self, results: Vec<Vec<String>>) -> Result<Vec<String>, Self::Error> {
        Ok(results.into_iter().flatten().collect())
    }

    fn visit_ident(&mut self, node: Node<'t>) -> Result<Vec<String>, Self::Error> {
        Ok(vec![node.utf8_text(self.source.as_bytes()).unwrap().to_string()])
    }

    fn visit_decl(&mut self, node: Node<'t>) -> Result<Vec<String>, Self::Error> {
        self.field_visitor(node, "target")
    }
}
```

`visit_program`'s default body (`children_visitor`) already does the right
thing: it visits every `decl`, and `combine`'s `flatten` concatenates their
results. Given `x = 1; y = x;`, `DeclExtractor` returns `["x", "y"]` — the
declared names, not `x`'s later use as a value.

## Typed node structs

The generated `Visitor` trait works directly with `tree_sitter::Node` and
`node.kind()`/`children_by_field_name` — you get traversal and dispatch, but
node payloads stay stringly-typed. If you'd rather have a real Rust struct
per kind with typed field accessors, see
[type-sitter](https://github.com/Jakobeha/type-sitter), which generates those
from the same `node-types.json` that `ts-bnf-tool convert` produces alongside
your grammar. The two approaches compose: a `Visitor` implementation can
construct type-sitter's typed wrappers from the `Node` it's handed, combining
`ts-bnf-tool`'s traversal skeleton with type-sitter's typed payloads.

---

Previous: [Visualising a grammar](10-visualising.md) · Next: [Generating a processing library](12-generating-a-library.md)
