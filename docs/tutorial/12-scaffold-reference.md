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
| `Cargo.toml` | The crate manifest. | Written once. Edit freely. |
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
inserts the one line it needs (`pub mod ast;`) into your existing
`lib.rs`, because otherwise the newly generated `examples/ast.rs`
couldn't even compile. It never removes anything you've added yourself.

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
`TryFrom<super::visitor::SourceNode<'tree>>` impl, and a `_pragma:
runtime::Pragma` field recording its start line/column. Both are
1-indexed, and the column counts characters rather than bytes (so a line
containing `ç` or `ã` reports the column an editor shows) and does not
count a leading UTF-8 byte-order mark. A leaf kind (one with no visible
children of its own) also gets a `_text: String` field.

`Pragma` and `BuildError` (the shared `TryFrom` error type) live inside an
inner `runtime` module, not at `ast.rs`'s own top level. And the two
injected fields start with an underscore. Both choices exist for the same
reason: so a grammar rule or field genuinely named `pragma`, `text`,
`build_error`, or `source_node` can never collide with these fixed,
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
pretty-prints an entire typed tree with no per-kind code. That's exactly
what the scaffolded `examples/ast.rs` does.

Only labelled fields become struct fields. A rule like `program -> decl* ;`
produces a `Program` with nothing but `_pragma`; labelling the repetition
(`program -> items: decl* ;`) gives it `pub items: Vec<Decl>` instead.

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
own fixed `runtime::Pragma`, `runtime::BuildError`, and
`super::visitor::SourceNode`.

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
| a bare-token field | `String` | `Option<String>` |
| a repeated field | `items: Vec<Decl>` | `items: Vec<Decl>`; a child that can't be built is left out |
| building | `Program::try_from(node)?`, which stops at the first problem | `runtime::recover::<Program>(node)`, which never fails |
| result | `Result<Program, BuildError>` | `Recovered<Program>` |

`_pragma` and `_text` are unchanged, and `TryFrom` and `BuildError` are
not generated at all in this mode. Nothing changes without the flag: the
output of a plain `--ast-types` scaffold is byte-for-byte what it was.

`runtime::recover` returns a `Recovered<T>` with two public fields:
`value: Option<T>`, which is `None` only when not even the root could be
built, and `diagnostics: Vec<Diagnostic>`, ordered by position in the
source. `recovered.is_complete()` is true when there is a value and no
diagnostics. A `Diagnostic` carries a `kind`, the offending node's
`byte_range`, and its `start` (line and column, 1-indexed like `_pragma`).

The generated `examples/ast.rs` uses it: it prints whatever was built,
then one `path:line:column: problem` line per diagnostic, and exits with
status 1 if there were any. Here it is on a declaration that is missing
its `;`:

```sh
$ printf 'x = 1\n' > broken.decls
$ cargo run --example ast -- broken.decls
broken.decls:
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
            target: Some(
                Ident {
                    _pragma: Pragma {
                        line: 1,
                        column: 1,
                    },
                    _text: "x",
                },
            ),
            value: Some(
                Expr {
                    _pragma: Pragma {
                        line: 1,
                        column: 5,
                    },
                },
            ),
        },
    ],
}
broken.decls:1:6: Missing { expected: ";" }
```

The `Decl` is all there, and the one diagnostic says where the `;` should
have been. Which of the diagnostics you get depends on what tree-sitter
found:

| tree-sitter found | the AST | diagnostic |
|---|---|---|
| a `MISSING` token the grammar required, like the `;` above | built as usual | `Missing { expected }` |
| a `MISSING` node standing in for a labelled field's value | that field is `None` | `Missing { expected }` |
| an `ERROR` region | a field it swallowed is `None`, or a repeated child is left out | one `Error` for the whole region |
| a labelled field absent from a node that has a syntax error inside it | `None` | `MissingField { kind, field }` |
| a child whose kind is not one of a merged enum's variants | `None` | `UnexpectedKind { expected, found }` |

A field that is `None` in a node with *no* syntax error produces no
diagnostic: the parser accepted the node without it, so it was optional in
the grammar. (The generated types don't model optional fields, which is
why every single-valued field is an `Option` in this mode.) An `ERROR`
node that tree-sitter nests inside another one of the same range is
reported once, not twice.

**Rerunning.** `--partial-trees` is recorded in `ts-bnf-tool.toml` as
`partial_trees = true`, and a plain `make generate` keeps it. On the
command line it is a one-way switch, like `--ast-types`. To turn it back
off, delete that line from `ts-bnf-tool.toml` and rerun.

Either way, `ast.rs` changes API, and `examples/ast.rs`, which is yours
after it is first written, does not follow. Delete it so the matching one
is regenerated, or update it by hand. When you switch with the command-line
flag, `scaffold` warns you that the example was left untouched; when you
edit `ts-bnf-tool.toml` instead, it can't tell, so remember to do this.

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
