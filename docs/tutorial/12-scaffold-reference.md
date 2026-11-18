---
title: Scaffold Reference
nav_order: 13
---

# Scaffold reference

[Generating a processing scaffold](11-generating-a-scaffold.md) walks
through building a crate from `decls.bnf` and writing visitors for it.
This page is the reference behind that walkthrough: every generated file,
every flag, the full `Visitor` trait, what a rerun does, and the typed-AST
options.

It uses the same `decls.bnf` throughout:

```bnf
# decls.bnf: a tiny declaration language
program -> decl* ;
decl -> target: ident '=' value: expr ';' ;
expr -> ident | num ;
ident -> /[a-z][a-zA-Z0-9_]*/ ;
num -> /[0-9]+/ ;
```

## What `scaffold` creates

The table below lists every file, what it's for, and whether
`scaffold` ever touches it again after the first run. This is relevant
so you know what files are safe to hand-edit.

| File / directory | What it is | Can you edit it? |
|---|---|---|
| `decls.bnf` | The grammar you wrote. Always the source of truth. | **Yes.** Edit this, then rerun `scaffold`. |
| `grammar.js`, `src/` | The tree-sitter grammar and C parser. Files `grammar.js` and folder `src/` are exactly what `convert --generate` already produces. | No: regenerated on every rerun. |
| `tree-sitter.json` | Tree-sitter's own package metadata file. | Written once, then left alone. Edit freely. |
| `queries/highlights.scm` | A starter syntax-highlighting query. | Written once, then left alone. Edit/extend freely (see [Keeping the grammar in sync](#keeping-the-grammar-in-sync) below). |
| `ts-bnf-tool.toml` | Records how the crate was scaffolded: the bundled grammar's filename, the crate name, and which flags were used. | Written once; a rerun updates only the fields matching a flag you actually pass. |
| `Makefile` | A makefile with useful targets, like `generate` that reruns `scaffold` for you. | Written once. Add your own targets. |
| `Cargo.toml` | The crate manifest. With `--partial-trees` it also depends on `ts-bnf-runtime` (see [the shared runtime crate](#input-with-syntax-errors---partial-trees)). | Written once. Edit freely. |
| `bindings/rust/build.rs` | Compiles `src/parser.c` (and `src/scanner.c`, if present). | No: regenerated on every rerun. |
| `bindings/rust/lib.rs` | Parser bindings (`LANGUAGE`, `NODE_TYPES`), plus a `parse` convenience function. | Written once. **This is where you add `pub mod` for your own `Visitor` implementations.** |
| `bindings/rust/visitor.rs` | The generated `Visitor<'tree>` trait (described below). | **No: always regenerated. Never hand-edit this file.** Write your own visitor in a different file instead, and register it in `lib.rs`. |
| `examples/walk.rs` | A small program that parses a file and counts its nodes. | Written once. Edit it, or drop a new file into `examples/`: Cargo picks those up automatically. |
| `.gitignore` | Ignores `/target`. | Written once. Extend freely. |

`lib.rs` includes the `parse` function, that looks like this:

```rust
pub fn parse(source: &str) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&LANGUAGE.into())?;
    parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse the given source".into())
}
```

This function takes a piece of source code in the target language you
are parsing as a string and uses Tree-Sitter to turn it into a syntax
tree.  If parsing succeeds, it returns the resulting `Tree` inside
`Ok`; if configuring the parser fails, or Tree-Sitter cannot produce a
tree, it returns an error instead.

One exception to `lib.rs`'s "written once" rule: if you scaffolded
without `--ast-types` and later rerun with it added, the rerun still
inserts the lines it needs (`pub mod ast;` and `pub use ast::parse_ast;`)
into your existing `lib.rs`, because otherwise the newly generated
`examples/ast.rs` couldn't even compile. A crate scaffolded with
`--ast-types` before `parse_ast` existed gets the `pub use` line the same
way on its next rerun, unless `lib.rs` already mentions `parse_ast`: a
function of your own under that name is left alone. It never removes
anything you've added yourself.

### Formatting

After writing the files, `scaffold` runs `rustfmt` (edition 2021, the
crate's own) over the Rust files **it wrote on that run**, so a freshly
scaffolded crate already passes `cargo fmt --check`. The tool passes no
formatting options of its own; a `rustfmt.toml` you keep above the crate is
honoured, as it is for your own `cargo fmt`.

Because only files written on that run are formatted, a rerun reformats the
regenerated `bindings/rust/visitor.rs`, `bindings/rust/ast.rs` and
`bindings/rust/build.rs`, and leaves `lib.rs`, `examples/walk.rs` and
`examples/ast.rs` exactly as you left them, however you formatted them.

Formatting is cosmetic, so a missing or failing `rustfmt` does not fail the
run: the whole crate is still written, with a `warning:` on stderr saying
the Rust files were left unformatted.

## Other ways to invoke it

Putting the `.bnf` file inside a folder and using that folder as the
target, as the walkthrough does, is the simplest approach. But there are
other options:

```sh
ts-bnf-tool scaffold grammar.bnf                # crate in ./<name>, name from the filename
ts-bnf-tool scaffold -o out/decls grammar.bnf   # crate in out/decls
ts-bnf-tool scaffold --name decls grammar.bnf   # override the crate/grammar name
ts-bnf-tool scaffold --no-header grammar.bnf    # suppress generated-file comments
```

`--name` also names the grammar in the generated trait's own doc
comment.  It defaults to the input filename's stem. So, for `decls.bnf`
there was no need for the `--name` option: its stem is already `decls`.

A hyphenated name (`my-lang`) is fine. `Cargo.toml`'s `[package] name`
keeps the hyphen, since that's Cargo's own convention. Everywhere else
(the tree-sitter grammar name, the generated C parser symbol, the
module path `examples/*.rs` imports) the hyphen becomes an underscore
(`my_lang`) instead, because tree-sitter's own `grammar()` call
rejects a hyphenated name outright.

A name still invalid after that substitution (a leading digit, whitespace,
…) is also rejected up front, before anything is written to disk.

`scaffold` runs no static checks on the grammar before generating,
same as `railroad` and `graph`. The only exception is that it does
check that no two rules would produce the same `visit_*` method name.
A grammar that fails this check is rejected with a clear diagnostic.

## The `Visitor` trait in full

### One method per grammar rule

For `decls.bnf` the trait has one method per kind (`visit_program`,
`visit_decl`, `visit_expr`, `visit_ident`, `visit_num`), the `visit()`
dispatcher, and the six helper methods every generated trait shares:
`combine`, `children_visitor`, `field_visitor`, `default_result`,
`error_visitor`, and `missing_visitor`.

A rule whose children carry **field** labels documents them. `decl` has
two, so its generated doc comment lists both, alongside the call that
would visit just that one field:

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

The `via ...` part is not what the default body does. The default is
`children_visitor(node)`, which visits every named child regardless of
field. `via ...` is a recipe for an override of your own;
[Step 4](11-generating-a-scaffold.md#step-4--visit-one-field-only) of the
walkthrough builds one and shows the difference in output.

### Leaves

`ident` and `num` are single tokens: no fields, no anonymous children,
nothing underneath to recurse into. Their generated methods differ from
`visit_decl` in both halves, and that's how you spot a leaf when reading
`visitor.rs`:

```rust
/// Visits a `ident` node.
///
/// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
fn visit_ident(&mut self, node: SourceNode<'tree>) -> Result<Self::Output, Self::Error> {
    let _ = node;
    self.default_result()
}
```

The doc comment carries a **Leaf node** marker instead of a field list,
and the body calls `default_result()` instead of `children_visitor(node)`.
Since `default_result()` is `combine(vec![])`, a leaf contributes nothing
until you override it. Doing so is how a token's text gets into a result
at all, which is exactly what `CollectText`'s `visit_ident` did with
`node.text().to_string()`.

### ANTLR correspondence

If you've used ANTLR's `AbstractParseTreeVisitor`, this should feel
familiar. The trait's own doc comment includes this table:

| ANTLR (`AbstractParseTreeVisitor`) | Here                                                            |
|-------------------------------------|-----------------------------------------------------------------|
| `visit(tree)`                        | `Visitor::visit`                                                 |
| `visitChildren(node)`                | `Visitor::children_visitor`                                      |
| `aggregateResult(agg, next)`         | `Visitor::combine` (`Vec`-based, not pairwise)                   |
| `defaultResult()`                    | `Visitor::default_result` (`= combine(vec![])`)                  |
| `visitErrorNode(node)`               | `Visitor::error_visitor`                                         |
| (no ANTLR analogue)                  | `Visitor::missing_visitor`, for tree-sitter's `MISSING` nodes    |

One thing has no ANTLR equivalent: `missing_visitor`. tree-sitter's error
recovery can insert a zero-width `MISSING` node, standing in for a token
the parser expected but never found. `Node::kind()` reports the
*expected* kind on that node, not a distinct "missing" kind. So `visit()`
checks `Node::is_missing()` first, before matching on kind, and routes
there instead.

## Keeping the grammar in sync

Once you've hand-edited a scaffolded crate, by adding files under
`examples/` or registering a hand-written `Visitor` in `lib.rs`, it's
worth knowing exactly what a rerun does.

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
rerun the same way; see
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

`ts-bnf-tool scaffold .`, which is what `make generate` runs, is worth
knowing on its own. Point `scaffold` at the crate's own *directory*,
instead of its grammar file, and it reruns using whatever
`ts-bnf-tool.toml` already recorded: the bundled grammar's filename,
`--name`, `--ast-types`, `--partial-trees`, `--merge-config`. No flags
needed.

Pass a flag anyway, and it overrides what's recorded: `ts-bnf-tool.toml`
is updated to match. One exception: `--ast-types` is a one-way switch. You
can add it on a later rerun, but never remove it. `--partial-trees` is a
one-way switch on the command line too; see
[its section](#input-with-syntax-errors---partial-trees) for how to turn
it back off.

### The mismatch guard

A crate remembers which grammar file it was bundled from. Point `scaffold`
at a *different* `.bnf` file for that same crate (rather than the
directory), and it refuses, rather than silently swap the bundled grammar
or ignore the new file:

```
$ ts-bnf-tool scaffold --output-dir decls other.bnf
error: a different grammar file ('decls.bnf') is already bundled in decls; scaffold again with that file, or remove/rename it first if you mean to replace it
```

This guard doesn't cover files pulled in via `%include`; those are freely
re-bundled as the include graph changes (see below).

### Bundling `%include`d files

If `decls.bnf` `%include`s another file, `scaffold` bundles the *whole*
include closure, not just the root file. Each included file lands at the
same path, relative to the crate root, that it had relative to the root
grammar's own directory. The `%include` directive itself is copied
unchanged; it already resolves correctly from the new location, since
`%include` paths are always relative to the file that names them.

Unlike the root grammar, included files aren't covered by the mismatch
guard above. The include graph is freely re-bundled every time it changes.

An `%include` that resolves *outside* the root grammar's own directory
tree is left external: not bundled, not rewritten. `scaffold` still
succeeds, but warns:

```
note: /path/outside/child.bnf is outside /path/to/decls; leaving it unbundled
```

### Non-in-place scaffolding

You can also point `scaffold` at an external `.bnf` file and a *fresh*
output directory, which is the original workflow and still fully
supported. It bundles a copy on that first run, and prints a note telling
you which copy is live from here on:

```
$ ts-bnf-tool scaffold --name decls --output-dir decls path/to/decls.bnf
bundled a copy of decls.bnf into decls; edit that copy from now on — path/to/decls.bnf is no longer read
```

From here on there are *two* copies of the grammar on disk: the original
external file, and the bundled one inside `decls/`. Only the bundled copy
is ever read again. The in-place workflow the walkthrough uses avoids
that split, since source and destination are the same file.

## Typed node structs (`--ast-types`)

[Step 7](11-generating-a-scaffold.md#step-7--generate-typed-structs-with---ast-types)
of the walkthrough turns this on and runs it. This section is the detail
behind it.

The generated `Visitor` trait works with `SourceNode`: a thin wrapper
around `tree_sitter::Node` plus the source text. You get traversal and
dispatch through it (`node.kind()`, `children_by_field_name`, and
`node.text()`), but a node's payload stays stringly-typed: there's no
Rust struct matching your grammar's shape.

`--ast-types` adds that, as a second, independent layer:
`bindings/rust/ast.rs`, one owned Rust struct per grammar rule. "Owned"
means no `'tree` lifetime survives construction: once built, a value
doesn't borrow from the parse tree anymore. Each struct gets a
`TryFrom<super::visitor::SourceNode<'tree>>` impl, and a `_span:
runtime::Span` field recording where the node is: its byte range in the
source, and where it starts and ends as a line and column (each a
`runtime::Pragma`). Lines and columns are 1-indexed, and a column counts
characters rather than bytes (so a line containing `ç` or `ã` reports the
column an editor shows) and does not count a leading UTF-8 byte-order mark.
The end is the position just past the node's last character. The byte range
is exactly what tree-sitter reports, so it indexes straight into the
source. A leaf kind (one with no visible children of its own) also gets a
`_text: String` field.

A field that holds a bare literal (a keyword, punctuation or an operator, as
in `op: '='`) is a `runtime::Token { kind, text, span }`, not a `String`: a
`String` would keep what the literal said but lose where it was. The same
goes for a list of them (`Vec<runtime::Token>`) and for the `Token` variant
of an enum that holds either a kind or a literal. A leaf's `_text` stays a
`String`, because the leaf struct already has its own `_span`.

`Pragma`, `Span`, `Token` and `BuildError` (the shared `TryFrom` error type)
live inside an inner `runtime` module, not at `ast.rs`'s own top level. And
the injected fields start with an underscore. Both choices exist for the same
reason: so a grammar rule or field genuinely named `pragma`, `span`, `text`,
`token`, `build_error`, or `source_node` can never collide with these fixed,
tool-injected names. See "A note on vocabulary" below.

Enabling it adds two files on top of the tree the walkthrough showed:

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

For `decls.bnf`, `bindings/rust/ast.rs` gets one struct per kind. `decl`
has two fields, so its `TryFrom` impl builds both from the parsed node:

```rust
/// `decl` node.
#[derive(Debug)]
pub struct Decl {
    pub _span: runtime::Span,
    pub target: Ident,
    pub value: Expr,
}

impl<'tree> TryFrom<super::visitor::SourceNode<'tree>> for Decl {
    type Error = runtime::BuildError;

    fn try_from(node: super::visitor::SourceNode<'tree>) -> Result<Self, Self::Error> {
        let _span = runtime::Span::from(node);
        let target = node
            .child_by_field("target")
            .ok_or(runtime::BuildError::MissingField { kind: "decl", field: "target" })?
            .try_into()?;
        let value = node
            .child_by_field("value")
            .ok_or(runtime::BuildError::MissingField { kind: "decl", field: "value" })?
            .try_into()?;
        Ok(Decl { _span, target, value })
    }
}
```

`ident`, being a leaf, gets a `_text: String` field instead of any nested
struct:

```rust
/// `ident` node.
#[derive(Debug)]
pub struct Ident {
    pub _span: runtime::Span,
    pub _text: String,
}
```

Every generated struct/enum derives `Debug`, so `{:#?}` recursively
pretty-prints an entire typed tree with no per-kind code. That's exactly
what the scaffolded `examples/ast.rs` does.

Only labelled fields become struct fields. A rule like `program -> decl* ;`
produces a `Program` with nothing but `_span`; labelling the repetition
(`program -> items: decl* ;`) gives it `pub items: Vec<Decl>` instead. (With
[`--partial-trees`](#input-with-syntax-errors---partial-trees), unlabelled
children are captured too, so a grammar without labels still gets a usable
tree.)

Like `visitor.rs`, `ast.rs` is always regenerated from the current
grammar, and there's no hand-edit support for it. If you need more fields
on a generated type, wrap it in your own struct rather than editing the
generated file. A rerun of `scaffold --ast-types` overwrites `ast.rs` every
time, same as `visitor.rs`, while `examples/ast.rs` itself follows the
same write-once-then-leave-alone rule as `examples/walk.rs`.

**A note on vocabulary.** Grammar rule and field names are otherwise
completely unrestricted. `pragma`, `text`, `build_error`, and
`source_node` are all plausible names a real grammar might want (a
`pragma` directive rule, a `text` leaf, a field called `text`), and all of
them are fine. A kind named any of these generates an ordinary top-level
`struct Pragma`/`Text`/`BuildError`/`SourceNode`, distinct from the tool's
own fixed `runtime::Pragma`, `runtime::Span`, `runtime::Token`,
`runtime::BuildError`, and `super::visitor::SourceNode`.

The only real restriction is on field labels: one starting with `_` is
rejected at generation time. That whole leading-underscore namespace is
reserved for the fields this tool injects itself (`_span`, `_text`, and
`_children` with `--partial-trees`).

### Collapsing related kinds (`--merge-config`)

A grammar sometimes has several kinds that are really one construct, from
the caller's point of view. `for_statement`, `while_statement`, and
`repeat_statement` might all just be "a loop." Left alone, `--ast-types`
gives each its own, unrelated struct.

`--merge-config <path>`, passed alongside `--ast-types`, collapses a group
of kinds like that into one Rust `enum`. The enum's own variants
discriminate on the Rust type, with no stringly-typed `kind` field needed.

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
  otherwise changing it. Here, `comment`'s struct is emitted as
  `DocComment` instead of the `Comment` its kind name would otherwise
  derive.
- **`ignore`** (not used above) explicitly marks a kind as "leave it as
  the default baseline struct". See the coverage report below for why
  you'd write this out loud instead of just doing nothing.

Every `merge`/`passthrough` entry's `target` is emitted verbatim as a Rust
`struct`/`enum` name. It must be a valid, non-keyword Rust identifier:
`Loop` is fine; `loop`, `my-loop`, and an empty string aren't. `scaffold`
rejects an invalid config up front, rather than emit Rust that won't
compile.

One kind can't be merged away: the grammar's own root rule. The scaffolded
`examples/ast.rs` needs one concrete, nameable root type to construct
(`use {crate}::ast::{RootKind};`). A kind claimed by a `merge` entry loses
its own `pub` struct and is only reachable through that entry's enum, so
it can't serve as that root type. A `merge` entry naming the root rule in
its `from` list is therefore rejected up front, the same as an invalid
`target`. To rename the root rule's struct, use `passthrough` instead.

Suppose your grammar has a `program -> items: (for_statement | while_statement
| repeat_statement)* doc: comment ;` rule. Running
`ts-bnf-tool scaffold --ast-types --merge-config ast-merge.toml grammar.bnf`
with the config above generates:

```rust
pub struct Program {
    pub _span: runtime::Span,
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
still gets the ordinary `TryFrom` impl `--ast-types` always generates;
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
kind still generates normally, as an ordinary `pub` struct, exactly as if
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
kind not otherwise claimed", so listing specific kinds alongside it would
be redundant at best, a likely typo at worst. `check_merge_config` rejects
that combination outright.

### A tree-sitter-free entry point (`parse_ast`)

Building the typed tree by hand takes tree-sitter: parse the source, wrap
the root node in a `SourceNode`, then convert it (or, with
`--partial-trees`, recover it). A program that only wants the typed tree
shouldn't have to depend on tree-sitter at all, so `ast.rs` also generates
`parse_ast`, and `lib.rs` re-exports it at the crate root:

```rust
let program: decls::ast::Program = decls::parse_ast(&source)?;
```

With `--partial-trees` the same call returns a
[`Recovered`](#input-with-syntax-errors---partial-trees) instead, and a
syntax error is *not* an `Err` there: whatever could be built is in `value`,
the problems are in `diagnostics`, and a broken region stays in the tree.

```rust
let recovered = decls::parse_ast(&source)?;   // Recovered<Program>
```

| | default | `--partial-trees` |
|---|---|---|
| returns | `Result<Program, Box<dyn Error>>` | `Result<Recovered<Program>, Box<dyn Error>>` |
| `Err` when | the parser can't run, or the tree can't be built (it stops at the first problem) | the parser can't run |

`Program` is the grammar's root rule's struct, under its `passthrough`
name if it has one. The error type is the same as `parse`'s. `parse_ast`
calls `lib.rs`'s own `parse` function, so keep that one: you can change how
it sets the parser up, but not its name or its `Result<Tree, _>` shape.
Nothing is generated when the root rule is claimed by a `merge` entry, since
its struct is private (`scaffold` rejects that configuration anyway).

### Input with syntax errors (`--partial-trees`)

By default, building the typed AST is all-or-nothing. One syntax error is
enough for the `TryFrom` impl of the node it touches to fail with a
`BuildError`, and you get no AST at all. That is right for a compiler and
wrong for an editor or a linter, which have to cope with code that is
half-typed.

`--partial-trees` (it requires `--ast-types`) generates a different API
for the same types, one that builds whatever it can from a tree with
syntax errors and reports the rest:

```sh
ts-bnf-tool scaffold --ast-types --partial-trees -o . decls.bnf
```

The flag changes the *shape* of `ast.rs`, so it is a choice you make once
when you scaffold, not something you pick at each call site:

| | default | `--partial-trees` |
|---|---|---|
| a single-valued field | `target: Ident` | `target: Option<Ident>` |
| a bare-token field | `runtime::Token` | `Option<runtime::Token>` |
| a repeated field | `items: Vec<Decl>` | `items: Vec<Decl>`; a child that can't be built is left out |
| building | `Program::try_from(node)?`, which stops at the first problem | `runtime::recover::<Program>(node)`, which never fails |
| result | `Result<Program, BuildError>` | `Recovered<Program>` |

`_span`, `_text` and `runtime::Token` are as in plain `--ast-types`, and
`TryFrom` and `BuildError` are not generated at all in this mode. Leave the
flag off and the scaffold is a plain `--ast-types` one: this flag changes
nothing about it. (One more difference, covered under "One runtime for every
language" below: the plain-data types of `runtime` come from a crate, rather
than being declared in `ast.rs`.)

`runtime::recover` returns a `Recovered<T>` with two public fields:
`value: Option<T>`, which is `None` only when not even the root could be
built, and `diagnostics: Vec<Diagnostic>`, ordered by position in the
source. `recovered.is_complete()` is true when there is a value and no
diagnostics. A `Diagnostic` carries a `kind` and a `span`: the offending
node's `Span`, the same type as every node's `_span`, so it has the byte
range and the start *and* end line and column (end exclusive). A `MISSING`
token is zero-width, so its end equals its start. The offending text is
`&source[span.byte_range.clone()]`, so a diagnostic needs no snippet of its
own.

The generated `examples/ast.rs` uses it: it prints whatever was built,
then one `path:line:column: problem` line per diagnostic, and exits with
status 1 if there were any. Here it is on a declaration that is missing
its `;`:

```sh
$ printf 'x = 1\n' > broken.decls
$ cargo run --example ast -- broken.decls
broken.decls:
Program {
    _span: Span {
        byte_range: 0..6,
        start: Pragma {
            line: 1,
            column: 1,
        },
        end: Pragma {
            line: 2,
            column: 1,
        },
    },
    items: [
        Decl {
            _span: Span {
                byte_range: 0..5,
                start: Pragma {
                    line: 1,
                    column: 1,
                },
                end: Pragma {
                    line: 1,
                    column: 6,
                },
            },
            target: Some(
                Ident {
                    _span: Span {
                        byte_range: 0..1,
                        start: Pragma {
                            line: 1,
                            column: 1,
                        },
                        end: Pragma {
                            line: 1,
                            column: 2,
                        },
                    },
                    _text: "x",
                },
            ),
            value: Some(
                Expr {
                    _span: Span {
                        byte_range: 4..5,
                        start: Pragma {
                            line: 1,
                            column: 5,
                        },
                        end: Pragma {
                            line: 1,
                            column: 6,
                        },
                    },
                    _children: [
                        Num(
                            Num {
                                _span: Span {
                                    byte_range: 4..5,
                                    start: Pragma {
                                        line: 1,
                                        column: 5,
                                    },
                                    end: Pragma {
                                        line: 1,
                                        column: 6,
                                    },
                                },
                                _text: "1",
                            },
                        ),
                    ],
                },
            ),
            _children: [
                Token(
                    Token {
                        kind: "=",
                        text: "=",
                        span: Span {
                            byte_range: 2..3,
                            start: Pragma {
                                line: 1,
                                column: 3,
                            },
                            end: Pragma {
                                line: 1,
                                column: 4,
                            },
                        },
                    },
                ),
            ],
        },
    ],
    _children: [],
}
broken.decls:1:6: Missing { expected: ";" }
```

The `Decl` is all there, and the one diagnostic says where the `;` should
have been. Note `Expr` and the `_children` of `Decl`: `expr -> ident | num ;`
labels nothing, yet its `1` is there, and the `=` the declaration wrote
out is kept too, as a `Token` with its own span. The `;` is not, because it
isn't in the file: a missing token is a diagnostic, not a child. That is
the next section.

Which of the diagnostics you get depends on what tree-sitter found:

| tree-sitter found | the AST | diagnostic |
|---|---|---|
| a `MISSING` token the grammar required, like the `;` above | built as usual; the token itself is not in `_children` | `Missing { expected }` |
| a `MISSING` node standing in for a labelled field's value | that field is `None` | `Missing { expected }` |
| an `ERROR` region | kept where it is, as an `Error` child of its parent (below); a labelled field it swallowed is `None` | one `Error` for the whole region |
| a labelled field absent from a node that has a syntax error inside it | `None` | `MissingField { kind, field }` |
| a child whose kind is not one of a merged enum's variants, or not one the grammar allows as an unlabelled child | `None`, or left out of a list | `UnexpectedKind { expected, found }` |
| a subtree nested deeper than the depth limit (see "Deeply nested input" below) | not built: `None`, or left out of a list | `TooDeep { limit }` |

A field that is `None` in a node with *no* syntax error produces no
diagnostic: the parser accepted the node without it, so it was optional in
the grammar. (The generated types don't model optional fields, which is
why every single-valued field is an `Option` in this mode.) An `ERROR`
node that tree-sitter nests inside another one of the same range is
reported once, not twice, and is one region in the tree, not two.

**Grammars without field labels.** Only labelled fields become struct
fields, so a grammar that labels nothing, such as `program -> decl* ;`,
would give structs holding only `_span`, and a partial tree that holds
nothing: the diagnostics would be the only useful output. So in this mode
every struct that has children also gets `_children`, a list of its
*unlabelled* children in source order. That means both the kinds the rule
refers to and the string literals it spells out (keywords, punctuation,
operators). The list always holds one enum, named after the kind (`ProgramChild`
for `program`), with a variant for each of these:

- Each kind the rule can refer to without a label: `program -> decl* ;` gives
  `_children: Vec<ProgramChild>`, and `ProgramChild` has a `Decl(Decl)`
  variant. It is an enum even with a single kind, because of the last
  bullet below.
- A literal: a `Token(runtime::Token)` variant, holding the token's kind, text
  and span (`decl -> 'var' ident ';' ;` gives `DeclChild` with `Ident(Ident)`
  and `Token(runtime::Token)`: a `Token` for `var`, an `Ident`, a `Token` for
  `;`). A kind whose only unlabelled children are literals still has the
  enum, with just `Token` (`call -> name: ident '(' ')' ;`).
- A broken region: an `Error(runtime::ErrorRegion)` variant, always, even on
  a kind whose children are all labelled (it then holds nothing else).
  See "Broken regions stay in the tree" below.

The enums belong to one kind each and are not shared, and they are not a
`--merge-config` entry's enum even when the kinds match, since a merge enum
has no place for a region. A leaf has no `_children`: its `_text` already is
its whole source, literals included. That is why `op -> '+' | '-' ;` is a
leaf with `_text: "+"` and nothing else.

A child that a field label already captures appears in its field, not
twice. Not captured, because they are not part of a rule's shape or are
not really there:

- Comments and the other `%extras`: they can appear anywhere.
- `MISSING` nodes: a zero-width point is a diagnostic, not a child. A missing
  `;` is not a `Token` for `;`.
- A pattern written directly in a rule, as in `pair -> key '=' /[0-9]+/ ;`.
  tree-sitter keeps no node for it, so there is nothing to capture. Give it
  a rule of its own (`num -> /[0-9]+/ ;`) and it becomes a named leaf with
  `_text`.

`_children` starts with `_`, like `_span` and `_text`, so no field label
can collide with it. A grammar rule can still be named `error` or `token`:
the fixed `Error` and `Token` variants keep their names, and the variant for
that rule becomes `ErrorNode` or `TokenNode`. Plain `--ast-types` does not
capture unlabelled children: its structs hold labelled fields only, plus
`_span` (and `_text` for a leaf).

**Broken regions stay in the tree.** An `ERROR` node is the part of the source
tree-sitter could not fit into the grammar. Dropping it would leave a hole
in the tree at exactly the place a tool most needs to know about, so it
is kept: as the `Error` variant of its parent's children enum, holding a
`runtime::ErrorRegion { span, text }`, the region's extent and the source text
it covers. It sits in `_children` in source order, between its siblings,
exactly where it is in the file. It has no children of its own: whatever
tree-sitter did recognise inside it is part of its text, and an `ERROR`
inside another `ERROR` is one region. The diagnostics still report it, so
you can use either.

Through `Reflect` an `ErrorRegion` is a node of kind `"ERROR"`, named (as in
tree-sitter), with its text and no children.

**Walking the tree generically.** A typed tree is pleasant to match on and
tedious to traverse: a function per type is hundreds of functions for a real
grammar. So in this mode every node also implements `runtime::Reflect`, a
read-only view that is the same for all of them:

| method | what it gives |
|---|---|
| `kind()` | tree-sitter's kind for the node: the rule's name (or alias), not the Rust type's, so a `passthrough`-renamed struct still reports the original. For a literal, the literal; for a broken region, `"ERROR"`. |
| `span()` | the node's `Span`. |
| `text()` | `Some` for a leaf, a literal and a broken region; `None` for anything made of children. |
| `children()` | the node's children in source order, as a `Vec<(Option<&'static str>, &dyn Reflect)>`: each child with the field label it sits under (below), labelled fields and unlabelled children together, literals and broken regions included. |
| `is_named()` | `true` for a node of a rule and for a broken region, `false` for a literal. |

Every generated struct implements it, and so do `runtime::Token` and
`runtime::ErrorRegion`. Every
generated enum implements it by delegating to the variant it holds, and the
enums are transparent: `children()` hands back the node an enum holds, never
the enum, so a walk never meets a `ProgramChild` or a `Loop`. (If you do have
an enum in hand, its `inner()` gives that same node.) `children()` merges
fields and unlabelled children by position, and leaves out the extras, so it
matches what tree-sitter shows for the same file.

**Which field a child is.** Each child comes as a pair, the field label it
sits under and the node. The label is the grammar's own (`target`, `value`),
the same one tree-sitter reports for that child, or `None` for a child no
label names: an unlabelled one, a literal, a broken region. Every element of a
repeated field carries its label. A consumer that reads the pair never has to
change when you add a label to the grammar later: the child that was `None`
now has a name, and the code that ignored it still does. A label that is a Rust
keyword (`type:`) is given as written, `"type"`, not as the escaped identifier
`r#type` the struct field uses.

**One loop, no recursion.** The traversal is the same for every consumer, so
the runtime has it: `runtime::walk(&root)` yields every node, depth first, a
node before its children, in source order, each as a `Visit { node, field,
depth }` (the root has depth 0 and no field). It uses an explicit stack on the
heap, so it does not recurse, and a tree of any depth is walked without
growing the call stack. This prints an outline, with no code per type:

```rust
use decls::ast::Program;
use decls::ast::runtime::{recover, walk};
use decls::visitor::SourceNode;

fn main() {
    let path = std::env::args().nth(1).expect("usage: outline <file>");
    let source = std::fs::read_to_string(&path).unwrap();
    let tree = decls::parse(&source).unwrap();
    let root = recover::<Program>(SourceNode { node: tree.root_node(), source: &source })
        .value
        .expect("nothing could be built");

    for visit in walk(&root) {
        let s = visit.node.span();
        println!(
            "{}{}{} {}:{}-{}:{}{}",
            "  ".repeat(visit.depth),
            visit.field.map(|f| format!("{f}: ")).unwrap_or_default(),
            visit.node.kind(),
            s.start.line, s.start.column, s.end.line, s.end.column,
            visit.node.text().map(|t| format!(" {t:?}")).unwrap_or_default(),
        );
    }
}
```

On `x = 1;`:

```
program 1:1-2:1
  decl 1:1-1:7
    target: ident 1:1-1:2 "x"
    = 1:3-1:4 "="
    value: expr 1:5-1:6
      num 1:5-1:6 "1"
    ; 1:6-1:7 ";"
```

Note `decl`'s children: the `target` ident and the `value` expr, which are
labelled fields, and the `=` and `;` literals, which are not, come back
interleaved in the order they are in the file, and `expr` is reached directly,
not through the enum that holds it. To call these methods on a concrete type
rather than a `&dyn Reflect`, bring the trait into scope:
`use ….ast::runtime::Reflect;`.

On a file with a broken line in the middle (`x = 1;`, then `@@@`, then
`y = 2;`) the same loop prints the region in its place, with nothing extra
to write:

```
program 1:1-4:1
  decl 1:1-1:7
    target: ident 1:1-1:2 "x"
    = 1:3-1:4 "="
    value: expr 1:5-1:6
      num 1:5-1:6 "1"
    ; 1:6-1:7 ";"
  ERROR 2:1-2:4 "@@@"
  decl 3:1-3:7
    target: ident 3:1-3:2 "y"
    = 3:3-3:4 "="
    value: expr 3:5-3:6
      num 3:5-3:6 "2"
    ; 3:6-3:7 ";"
```

**Deeply nested input.** Building the typed tree is recursive, a few calls
per level of nesting, and so are `Drop` and `Debug` on the result. Input
nested deeper than the stack can take (a few hundred parentheses, in a debug
build on a 2 MiB thread) would not fail with an error you could catch: it
would abort the whole process. A parser that takes input from somebody else
cannot allow that, so `recover` stops at a depth limit,
`runtime::DEFAULT_MAX_DEPTH` (256 levels), chosen to be safe on a small stack
even in a debug build. A level is one non-leaf node, and the root is level 1.
A subtree that would be level 257 is not built. Its field is `None` (or it is
left out of its list), and a diagnostic
`TooDeep { limit: 256 }` says where, with the span of the subtree that was
left out, so `&source[span.byte_range.clone()]` is the text that was skipped.
The rest of the tree, down to the limit, is built as usual, and the
`ERROR`/`MISSING` diagnostics still cover the whole input however deep it
goes: that walk over tree-sitter's tree is iterative, too.

A caller that knows its stack is bigger can raise the limit (or lower it,
for a smaller one):

```rust
let recovered = runtime::recover_with_limit::<Program>(root, 5_000);
```

`recover_with_limit` does not make it safe to go deep: past what the stack
can take, a stack overflow is an abort again. Raise it only together with
the size of the thread's stack (`std::thread::Builder::stack_size`). Plain
`--ast-types`, whose `TryFrom` impls have nowhere to carry a limit, has no such
guard.

**One runtime for every language.** The data a partial tree is made of has
nothing to do with tree-sitter: `Span`, `Pragma`, `Token`, `ErrorRegion`,
`Diagnostic`, `DiagnosticKind`, `Recovered`, `DEFAULT_MAX_DEPTH`, and the
`Reflect` trait with `walk`, `Walk` and `Visit`. So they are not generated
into each crate: they are declared once, in the tree-sitter-free
[`ts-bnf-runtime`](https://crates.io/crates/ts-bnf-runtime) crate, which
every `--partial-trees` crate depends on. The scaffold writes
`ts-bnf-runtime = "<version>"` into its `Cargo.toml`, where `<version>` is the
version of the `ts-bnf-tool` that generated it (the two are released
together), and `ast::runtime` re-exports the items, so every path in this
chapter (`decls::ast::runtime::Span`, `::Reflect`, `::walk`, `::Recovered`)
is the same as ever. What stays generated is what needs tree-sitter: `recover`
and `recover_with_limit`, the `Recover` trait, the conversions from a
tree-sitter node into those types, and the `Reflect` impls for the
grammar's own structs and enums.

That matters to a program that handles more than one language. If each crate
declared its own `Reflect`, two languages would have two unrelated traits and
unrelated `Span`s, and the program would convert between them. With one
crate, they share them, and code written once against it works for every
language:

```rust
use ts_bnf_runtime::{walk, Reflect};

/// How many nodes a tree has, whichever language it is a tree of.
fn size(root: &dyn Reflect) -> usize {
    walk(root).count()
}

let decls_tree = decls::parse_ast(&decls_source)?.value;     // `decls`
let other_tree = otherlang::parse_ast(&other_source)?.value; // another language
println!("{} and {}", size(&decls_tree.unwrap()), size(&other_tree.unwrap()));
```

Add `ts-bnf-runtime` to your own `Cargo.toml` to name the trait, with the same
version requirement as the generated crates, so that Cargo builds one copy of
it for all of them. A second copy (a different version, say) would be a
different `Reflect`, and the program would stop compiling with "the trait
bound `…: ts_bnf_runtime::Reflect` is not satisfied" (and a note that there
are multiple versions of the crate), which is the conversion problem back
again.

**Rerunning.** `--partial-trees` is recorded in `ts-bnf-tool.toml` as
`partial_trees = true`, and a plain `make generate` keeps it. On the
command line it is a one-way switch, like `--ast-types`. To turn it back
off, delete that line from `ts-bnf-tool.toml` and rerun.

Either way, `ast.rs` changes API, and `examples/ast.rs`, which is yours
after it is first written, does not follow. Delete it so the matching one
is regenerated, or update it by hand. When you switch with the command-line
flag, `scaffold` warns you that the example was left untouched; when you
edit `ts-bnf-tool.toml` instead, it can't tell, so remember to do this.

`Cargo.toml` is yours too, so a rerun never edits it. That matters for a
crate that has no `ts-bnf-runtime` in it yet: one scaffolded as plain
`--ast-types`, or with `--partial-trees` by an older `ts-bnf-tool`. Its new
`ast.rs` does not compile without the dependency, so `scaffold` warns, and
names the line to add under `[dependencies]`:

```
warning: bindings/rust/ast.rs now takes its runtime types from the ts-bnf-runtime
crate, but Cargo.toml does not depend on it, so the crate will not compile;
Cargo.toml was left untouched — add this line under [dependencies]:
    ts-bnf-runtime = "<version>"
```

The warning repeats on each run until the line is there. A crate that is not
partial, or has the line, hears nothing.

`--partial-trees` composes with `--merge-config`: a merged `enum Loop`
dispatches to its variants in the same way, and a kind outside its set is
reported as `UnexpectedKind` instead of an error.

## Typed field accessors without ownership

Want typed field accessors directly over borrowed `tree_sitter::Node`s,
with no owned copy, no `'tree`-free struct, and no merge/collapse? See
[type-sitter](https://github.com/Jakobeha/type-sitter). It generates those
from the same `node-types.json` the generated crate's `NODE_TYPES`
constant also embeds.

It composes with the `Visitor` trait the same way it always has,
independently of `--ast-types`: a `Visitor` implementation can construct
type-sitter's typed wrappers from the `Node` inside the `SourceNode` it's
handed.

---

Previous: [Generating a processing scaffold](11-generating-a-scaffold.md) · Back to the [index](../index.md)
