//! Compile-check for the generated ANTLR-style `Visitor<'tree>` trait
//! (issue #210).
//!
//! `include!`ing the checked-in fixture below makes its `pub trait
//! Visitor<'tree> { ... }` a real item in *this test binary* — if the
//! emitter (`ts_bnf_tool::dom::RustVisitor`) ever produces invalid Rust,
//! `cargo test` fails to *compile*, not just fails a string-comparison
//! assertion.
//!
//! The fixture itself is generated from `tools/tests/fixtures/visitor_sample.bnf`
//! by `make visitor-fixture`; `make visitor-fixture-check` (part of `make
//! check`) regenerates it and fails if the checked-in copy has drifted —
//! see the Makefile, which mirrors the same generate/check split already
//! used there for `grammar/railroad.svg` and `grammar/graph.pdf`. Nothing
//! here duplicates that freshness check: this file's only job is proving
//! the fixture compiles.
//!
//! This is deliberately a small, self-contained synthetic grammar, not the
//! dogfood `grammar/bnf.bnf` — that fixture and its own behavior tests
//! (running a real visitor over a parsed tree, `MISSING`-node handling)
//! come later (210.31-210.34), once the CLI subcommand exists.

// Compiles `pub trait Visitor<'tree> { ... }` (and everything else the
// fixture contains) as real items in this test binary.
include!("fixtures/visitor_sample.rs");
