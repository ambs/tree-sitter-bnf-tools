// Emitters for `BuildMode::Partial` (`--partial-trees`): the generated AST
// accepts a parse tree containing tree-sitter `ERROR`/`MISSING` nodes and
// returns whatever it could build plus diagnostics, instead of aborting on
// the first problem the way the strict `TryFrom` API does.
//
// Everything this mode adds to the generated file is either inside the
// `runtime` module or an inherent method on `SourceNode`; a grammar kind can
// therefore never collide with it (same structural-disjointness approach as
// the strict `runtime::Pragma`/`runtime::BuildError`).

use indoc::{formatdoc, indoc};
use std::fmt::{self, Formatter};

use super::{
    AstFieldSpec, AstNodeSpec, MergeConfig, indent, kind_display_name, rust_field_ident,
    shape_kinds,
};

/// The items partial mode adds to the generated `runtime` module, flush-left
/// (the caller indents them into the module): the diagnostic types, the
/// `Recover` trait, and the `recover` entry point with its syntax-error walk.
/// Takes the place of strict mode's `BuildError`.
pub(super) const RUNTIME_ITEMS: &str = indoc! {r#"
    /// What kind of problem a [`Diagnostic`] reports.
    #[derive(Debug)]
    pub enum DiagnosticKind {
        /// A tree-sitter `ERROR` node: source text the parser could not fit
        /// into the grammar. Reported once per error region, not once per
        /// nested `ERROR` node.
        Error,
        /// A tree-sitter `MISSING` node: a zero-width stand-in for something
        /// the grammar required. `expected` is the kind (or token) it stands in for.
        Missing { expected: String },
        /// A field absent from a node whose subtree contains a syntax error.
        MissingField { kind: &'static str, field: &'static str },
        /// A child whose kind is outside the variant set of the multi-shape
        /// (or merged) enum named by `expected`.
        UnexpectedKind { expected: &'static str, found: String },
    }

    /// One problem found while recovering a typed AST from a parse tree.
    #[derive(Debug)]
    pub struct Diagnostic {
        /// What went wrong.
        pub kind: DiagnosticKind,
        /// The offending node's byte range in the source (empty for `MISSING`).
        pub byte_range: std::ops::Range<usize>,
        /// The offending node's start position.
        pub start: Pragma,
    }

    impl Diagnostic {
        /// A diagnostic of `kind` located at `node`.
        pub fn at(node: super::super::visitor::SourceNode<'_>, kind: DiagnosticKind) -> Self {
            Diagnostic { kind, byte_range: node.node.byte_range(), start: Pragma::from(node) }
        }
    }

    /// The result of recovering a typed AST: whatever could be built, and
    /// everything that could not.
    #[derive(Debug)]
    pub struct Recovered<T> {
        /// The recovered value, or `None` if not even the root could be built.
        pub value: Option<T>,
        /// Every problem found, ordered by position in the source.
        pub diagnostics: Vec<Diagnostic>,
    }

    impl<T> Recovered<T> {
        /// `true` if a value was built and no problem was found.
        pub fn is_complete(&self) -> bool {
            self.value.is_some() && self.diagnostics.is_empty()
        }
    }

    /// Builds a typed node from a parse-tree node without ever aborting:
    /// problems are pushed onto `diags`, and whatever could not be built is
    /// simply absent from the result.
    pub trait Recover<'tree>: Sized {
        /// Builds `Self` from `node`. `None` only when `node` cannot be a
        /// `Self` at all (a multi-shape enum given a kind outside its set).
        fn recover(
            node: super::super::visitor::SourceNode<'tree>,
            diags: &mut Vec<Diagnostic>,
        ) -> Option<Self>;
    }

    /// `true` for tree-sitter's recovery nodes (`ERROR` and `MISSING`). Their
    /// `kind()` is unreliable as a type tag — a `MISSING` node reports the
    /// kind it stands in for — so they are never built into typed values.
    pub fn is_recovery(node: super::super::visitor::SourceNode<'_>) -> bool {
        node.node.is_missing() || node.node.is_error()
    }

    /// Recovers a `T` from `root`, returning it together with every
    /// diagnostic found, ordered by position in the source.
    ///
    /// Diagnostics come from two sources that never overlap: a walk over the
    /// tree's `ERROR`/`MISSING` nodes (whether or not they sit at a field
    /// position), and the build itself, which reports only structural
    /// problems (an absent field, an unexpected kind).
    pub fn recover<'tree, T: Recover<'tree>>(
        root: super::super::visitor::SourceNode<'tree>,
    ) -> Recovered<T> {
        let mut diagnostics = Vec::new();
        let value = if is_recovery(root) { None } else { T::recover(root, &mut diagnostics) };
        if root.node.has_error() {
            collect_syntax_problems(root, false, &mut diagnostics);
        }
        diagnostics.sort_by_key(|d| d.byte_range.start);
        Recovered { value, diagnostics }
    }

    /// Appends a diagnostic for `node` if it is a `MISSING` node, or an
    /// `ERROR` node not already inside one, then recurses into its children.
    /// `inside_error` is `true` below an `ERROR` node: tree-sitter nests an
    /// `ERROR` in an `ERROR` of the very same range (a lexer failure inside
    /// an error region, for instance), and the outermost one already covers
    /// it. A `MISSING` node is still reported there, since it says what the
    /// parser expected. Only subtrees that contain an error are descended
    /// into (`has_error` is a constant-time flag), so a clean tree costs
    /// nothing.
    fn collect_syntax_problems(
        node: super::super::visitor::SourceNode<'_>,
        inside_error: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if node.node.is_missing() {
            let expected = node.node.kind().to_string();
            diagnostics.push(Diagnostic::at(node, DiagnosticKind::Missing { expected }));
        } else if node.node.is_error() && !inside_error {
            diagnostics.push(Diagnostic::at(node, DiagnosticKind::Error));
        }
        if !node.node.has_error() {
            return;
        }
        let inside_error = inside_error || node.node.is_error();
        let mut cursor = node.node.walk();
        for child in node.node.children(&mut cursor) {
            let child = super::super::visitor::SourceNode { node: child, source: node.source };
            collect_syntax_problems(child, inside_error, diagnostics);
        }
    }
"#};

/// Emits the `SourceNode` recovery helpers: a second inherent `impl` block
/// (after the one `fmt_node_helpers` emits, whose `child_by_field` /
/// `children_by_field` these build on). Each helper owns the `ERROR`/`MISSING`
/// gate, so the per-kind `Recover` impls stay one line per field.
///
/// An absent field is reported as `MissingField` only when the owning node
/// itself contains a syntax error. In a clean subtree the parser accepted the
/// node without that field, so it is legitimately optional — the AST layer
/// does not model optionality — and staying silent is correct.
pub(super) fn fmt_recover_helpers(f: &mut Formatter<'_>) -> fmt::Result {
    let body = indoc! {r#"
        impl<'tree> super::visitor::SourceNode<'tree> {
            /// Recovers the child at `field_name` as a `T`: `None` if it is
            /// absent, or is an `ERROR`/`MISSING` node (reported separately by
            /// `runtime::recover`'s syntax walk, so not repeated here).
            pub fn recover_field<T: runtime::Recover<'tree>>(
                &self,
                field_name: &'static str,
                kind: &'static str,
                diags: &mut Vec<runtime::Diagnostic>,
            ) -> Option<T> {
                match self.child_by_field(field_name) {
                    Some(child) if runtime::is_recovery(child) => None,
                    Some(child) => T::recover(child, diags),
                    None => {
                        self.report_absent_field(field_name, kind, diags);
                        None
                    }
                }
            }

            /// Like [`Self::recover_field`], for a field whose value is a bare
            /// token: its source text.
            pub fn field_text(
                &self,
                field_name: &'static str,
                kind: &'static str,
                diags: &mut Vec<runtime::Diagnostic>,
            ) -> Option<String> {
                match self.child_by_field(field_name) {
                    Some(child) if runtime::is_recovery(child) => None,
                    Some(child) => Some(child.text().to_string()),
                    None => {
                        self.report_absent_field(field_name, kind, diags);
                        None
                    }
                }
            }

            /// Recovers every child at `field_name` as a `T`, in order. A child
            /// that is an `ERROR`/`MISSING` node, or cannot be built, is
            /// dropped rather than aborting the rest.
            pub fn recover_fields<T: runtime::Recover<'tree>>(
                &self,
                field_name: &str,
                diags: &mut Vec<runtime::Diagnostic>,
            ) -> Vec<T> {
                let mut out = Vec::new();
                for child in self.children_by_field(field_name) {
                    if runtime::is_recovery(child) {
                        continue;
                    }
                    if let Some(value) = T::recover(child, diags) {
                        out.push(value);
                    }
                }
                out
            }

            /// The source text of every child at `field_name`, in order,
            /// skipping `ERROR`/`MISSING` nodes.
            pub fn field_texts(&self, field_name: &str) -> Vec<String> {
                let mut out = Vec::new();
                for child in self.children_by_field(field_name) {
                    if !runtime::is_recovery(child) {
                        out.push(child.text().to_string());
                    }
                }
                out
            }

            /// Pushes a `MissingField` diagnostic for an absent `field_name`,
            /// but only if this node's subtree contains a syntax error (see
            /// the note on this impl block).
            fn report_absent_field(
                &self,
                field_name: &'static str,
                kind: &'static str,
                diags: &mut Vec<runtime::Diagnostic>,
            ) {
                if self.node.has_error() {
                    let problem = runtime::DiagnosticKind::MissingField { kind, field: field_name };
                    diags.push(runtime::Diagnostic::at(*self, problem));
                }
            }
        }
    "#};
    writeln!(f, "\n{}", body.trim_end())
}

/// Builds one field's recovery statement inside a `Recover` body: the
/// partial-mode counterpart of the strict `field_try_from`, with the same
/// four shapes (repeated token, repeated named/multi-shape, single token,
/// single named/multi-shape), each delegating to the matching `SourceNode`
/// helper ([`fmt_recover_helpers`]). The `let` binding is named via
/// [`rust_field_ident`]; every helper call uses `field_name` verbatim, since
/// tree-sitter's field lookup needs the grammar's own label.
///
/// The generated `recover` function's parameters are `_node` and `_diags`,
/// never `node`/`diags`: a field labelled `node` or `diags` would otherwise
/// bind a local of that name and shadow the parameter for every later field.
/// Grammar field labels can't start with `_` (see
/// `check_field_labels_are_representable`), so the underscore names can never
/// collide with a binding. (`_diags` also never trips `unused_variables`,
/// even in a struct with no field that needs it.)
///
/// Panics if `field`'s target resolves to no possible shape at all, same as
/// the strict `field_try_from` — a field always has at least one target kind.
fn field_recover(kind: &str, field_name: &str, field: &AstFieldSpec) -> String {
    let target = &field.target;
    let ident = rust_field_ident(field_name);

    if field.multiple && target.named.is_empty() && target.anonymous_token {
        format!("let {ident} = _node.field_texts(\"{field_name}\");\n")
    } else if field.multiple {
        format!("let {ident} = _node.recover_fields(\"{field_name}\", _diags);\n")
    } else if target.named.is_empty() && target.anonymous_token {
        format!("let {ident} = _node.field_text(\"{field_name}\", \"{kind}\", _diags);\n")
    } else if !target.named.is_empty() {
        format!("let {ident} = _node.recover_field(\"{field_name}\", \"{kind}\", _diags);\n")
    } else {
        unreachable!("a field always resolves to at least one target kind")
    }
}

/// Builds one kind's full `impl<'tree> runtime::Recover<'tree> for
/// {struct_name}` block — the partial-mode replacement for the strict
/// `TryFrom` impl: `Pragma` first, then one recovery statement per field
/// ([`field_recover`]), then `_text` for a leaf kind, then the struct
/// literal. Always returns `Some`: whether a node is recoverable at all
/// (`ERROR`/`MISSING`, or a kind outside an enum's set) is decided by the
/// caller — a `SourceNode` helper or a multi-shape enum — before this runs.
///
/// `struct_name` is resolved by the caller, same as the declaration it
/// pairs with, so a `passthrough` rename only changes one call site.
pub(super) fn fmt_struct_recover(struct_name: &str, kind: &str, spec: &AstNodeSpec) -> String {
    let mut statements = String::new();
    let mut field_list = Vec::new();
    for (field, field_spec) in &spec.fields {
        statements.push_str(&field_recover(kind, field, field_spec));
        field_list.push(rust_field_ident(field));
    }
    if spec.is_leaf {
        statements.push_str("let _text = _node.text().to_string();\n");
        field_list.push("_text".to_string());
    }

    let statements_text = indent(statements.trim_end(), 8);
    let field_list_text = field_list.join(", ");
    let name = struct_name;

    formatdoc! {r#"
        impl<'tree> runtime::Recover<'tree> for {name} {{
            fn recover(
                _node: super::visitor::SourceNode<'tree>,
                _diags: &mut Vec<runtime::Diagnostic>,
            ) -> Option<Self> {{
                let _pragma = runtime::Pragma::from(_node);
        {statements_text}
                Some({name} {{ _pragma, {field_list_text} }})
            }}
        }}
    "#}
}

/// Builds one multi-shape (or `merge`) enum's `Recover` impl — the
/// partial-mode counterpart of the strict enum `TryFrom`: a bare-token early
/// return when `shape` includes the `"$token"` sentinel, then a `match` on
/// `node.node.kind()` with one arm per real kind delegating to that kind's
/// own `Recover`. A kind outside the set yields an `UnexpectedKind`
/// diagnostic and `None` instead of an error. Each variant goes through
/// [`kind_display_name`], so a `passthrough`-renamed kind's variant still
/// names the type it is actually declared under.
pub(super) fn fmt_multi_shape_enum_recover(
    shape: &[String],
    name: &str,
    merge_config: Option<&MergeConfig>,
) -> String {
    let (clean_shape, has_token) = shape_kinds(shape);

    let mut arms = String::new();
    for kind in clean_shape {
        let variant = kind_display_name(kind, merge_config);
        arms.push_str(&format!(
            "\"{kind}\" => <{variant} as runtime::Recover<'tree>>::recover(_node, _diags).map({name}::{variant}),\n"
        ));
    }
    let arms_text = indent(arms.trim_end(), 12);

    let token_early_return = if has_token {
        formatdoc! {r#"
            if !_node.node.is_named() {{
                return Some({name}::Token(_node.text().to_string()));
            }}
        "#}
    } else {
        String::new()
    };
    let token_early_return_text = indent(token_early_return.trim_end(), 8);

    formatdoc! {r#"
        impl<'tree> runtime::Recover<'tree> for {name} {{
            fn recover(
                _node: super::visitor::SourceNode<'tree>,
                _diags: &mut Vec<runtime::Diagnostic>,
            ) -> Option<Self> {{
        {token_early_return_text}
                match _node.node.kind() {{
        {arms_text}
                    found => {{
                        let problem = runtime::DiagnosticKind::UnexpectedKind {{
                            expected: "{name}",
                            found: found.to_string(),
                        }};
                        _diags.push(runtime::Diagnostic::at(_node, problem));
                        None
                    }}
                }}
            }}
        }}
    "#}
}

#[cfg(test)]
mod tests {
    use super::super::RustAst;
    use crate::dom::Grammar;
    use crate::dom::GrammarNode::{
        Choice, Field, Sequence, TerminalLiteral, TerminalPattern, ZeroOrMore,
    };
    use crate::dom::ast::BuildMode;
    use crate::dom::ast::merge::{MergeConfig, MergeEntry, PassthroughEntry};
    use crate::dom::test_utils::{nt, p};

    /// Renders `grammar` in [`BuildMode::Partial`], headerless, optionally
    /// with a merge config.
    fn partial(grammar: &Grammar, merge_config: Option<&MergeConfig>) -> String {
        RustAst::new(grammar, "g", true, merge_config, BuildMode::Partial)
            .expect("grammar must be ast-safe")
            .to_string()
    }

    /// Renders `grammar` in [`BuildMode::Strict`], headerless.
    fn strict(grammar: &Grammar) -> String {
        RustAst::new(grammar, "g", true, None, BuildMode::Strict)
            .expect("grammar must be ast-safe")
            .to_string()
    }

    /// Extracts the text strictly between `header` (which must end in the
    /// item's opening `{`) and its balanced closing `}`, so tests can check
    /// one item's own members without depending on what the file emits
    /// around it. Same technique as `trait_body` in the visitor's tests.
    fn item_body<'a>(out: &'a str, header: &str) -> &'a str {
        let open = out
            .find(header)
            .unwrap_or_else(|| panic!("`{header}` present in:\n{out}"))
            + header.len()
            - 1;
        let mut depth = 0i32;
        for (i, c) in out[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &out[open + 1..open + i];
                    }
                }
                _ => {}
            }
        }
        panic!("`{header}` has no matching closing brace");
    }

    /// An unbalanced item is a loud failure, not a silently truncated body
    /// that makes later assertions pass vacuously.
    #[test]
    #[should_panic(expected = "has no matching closing brace")]
    fn item_body_panics_on_unbalanced_input() {
        item_body("struct A { unbalanced", "struct A {");
    }

    /// One kind exercising every field shape: single named, single
    /// multi-shape, repeated token, single token — plus a repeating parent
    /// and a leaf.
    fn every_shape_grammar() -> Grammar {
        Grammar::from_rules([
            p(
                "program",
                Field("items".into(), Box::new(ZeroOrMore(Box::new(nt("decl"))))),
            ),
            p(
                "decl",
                Sequence(vec![
                    Field("target".into(), Box::new(nt("ident"))),
                    Field(
                        "value".into(),
                        Box::new(Choice(vec![nt("ident"), nt("num")])),
                    ),
                    Field(
                        "ops".into(),
                        Box::new(ZeroOrMore(Box::new(TerminalLiteral("'+'".into())))),
                    ),
                    Field("tag".into(), Box::new(TerminalLiteral("'t'".into()))),
                ]),
            ),
            p("ident", TerminalPattern("/[a-z]+/".into())),
            p("num", TerminalPattern("/[0-9]+/".into())),
        ])
    }

    /// Single-valued fields become `Option`, repeated ones stay `Vec`, and the
    /// injected `_pragma` is never optional.
    #[test]
    fn struct_fields_single_is_option_multiple_stays_vec() {
        let out = partial(&every_shape_grammar(), None);

        let decl = item_body(&out, "pub struct Decl {");
        assert!(decl.contains("pub _pragma: runtime::Pragma,"));
        assert!(decl.contains("pub target: Option<Ident>,"));
        assert!(decl.contains("pub value: Option<Value>,"));
        assert!(decl.contains("pub ops: Vec<String>,"));
        assert!(decl.contains("pub tag: Option<String>,"));

        let program = item_body(&out, "pub struct Program {");
        assert!(program.contains("pub items: Vec<Decl>,"));
    }

    /// A leaf kind's injected `_text` is not wrapped: a node that exists
    /// always has text, even when it is empty.
    #[test]
    fn leaf_struct_text_is_not_optional() {
        let out = partial(&every_shape_grammar(), None);
        let ident = item_body(&out, "pub struct Ident {");
        assert!(ident.contains("pub _text: String,"));
        assert!(!ident.contains("Option"));
    }

    /// Partial mode replaces `TryFrom`/`BuildError` entirely.
    #[test]
    fn emits_recover_impls_and_no_try_from_or_build_error() {
        let out = partial(&every_shape_grammar(), None);
        assert!(out.contains("impl<'tree> runtime::Recover<'tree> for Decl {"));
        assert!(out.contains("impl<'tree> runtime::Recover<'tree> for Value {"));
        assert!(!out.contains("TryFrom"));
        assert!(!out.contains("BuildError"));
    }

    /// Each field shape delegates to its own `SourceNode` helper, passing the
    /// grammar's own label and kind name verbatim.
    #[test]
    fn recover_body_uses_the_helper_for_each_field_shape() {
        let out = partial(&every_shape_grammar(), None);

        let decl = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Decl {");
        assert!(decl.contains("_node.recover_field(\"target\", \"decl\", _diags)"));
        assert!(decl.contains("_node.recover_field(\"value\", \"decl\", _diags)"));
        assert!(decl.contains("_node.field_texts(\"ops\")"));
        assert!(decl.contains("_node.field_text(\"tag\", \"decl\", _diags)"));
        assert!(decl.contains("Some(Decl { _pragma, target, value, ops, tag })"));

        let program = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Program {");
        assert!(program.contains("_node.recover_fields(\"items\", _diags)"));
    }

    /// A leaf builds its `_text` and returns it in the struct literal.
    #[test]
    fn leaf_recover_builds_text() {
        let out = partial(&every_shape_grammar(), None);
        let ident = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Ident {");
        assert!(ident.contains("let _text = _node.text().to_string();"));
        assert!(ident.contains("Some(Ident { _pragma, _text })"));
    }

    /// The generated `recover` always names its parameters `_node`/`_diags`,
    /// whether or not any field needs them: grammar field labels can't start
    /// with `_`, so a field labelled `node` or `diags` can never shadow them,
    /// and an underscore parameter never trips `unused_variables`.
    #[test]
    fn recover_parameters_are_always_underscore_names() {
        let g = Grammar::from_rules([
            p(
                "ops_only",
                Field(
                    "ops".into(),
                    Box::new(ZeroOrMore(Box::new(TerminalLiteral("'+'".into())))),
                ),
            ),
            p("leaf", TerminalLiteral("'x'".into())),
            p("one_field", Field("node".into(), Box::new(nt("leaf")))),
        ]);
        let out = partial(&g, None);

        for kind in ["OpsOnly", "Leaf", "OneField"] {
            let body = item_body(
                &out,
                &format!("impl<'tree> runtime::Recover<'tree> for {kind} {{"),
            );
            assert!(
                body.contains("_node: super::visitor::SourceNode<'tree>,"),
                "{kind}: {body}"
            );
            assert!(
                body.contains("_diags: &mut Vec<runtime::Diagnostic>,"),
                "{kind}: {body}"
            );
        }
        // A field labelled `node` binds a local `node`, distinct from `_node`.
        let one_field = item_body(&out, "impl<'tree> runtime::Recover<'tree> for OneField {");
        assert!(
            one_field.contains("let node = _node.recover_field(\"node\", \"one_field\", _diags);")
        );
    }

    /// A keyword field label is escaped in the binding and struct literal, but
    /// the helper call and `MissingField` label keep the grammar's own text.
    #[test]
    fn keyword_field_label_is_escaped_only_in_rust_identifiers() {
        let g = Grammar::from_rules([
            p("stmt", Field("type".into(), Box::new(nt("ident")))),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);
        let body = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Stmt {");
        assert!(body.contains("let r#type = _node.recover_field(\"type\", \"stmt\", _diags);"));
        assert!(body.contains("Some(Stmt { _pragma, r#type })"));
        let decl = item_body(&out, "pub struct Stmt {");
        assert!(decl.contains("pub r#type: Option<Ident>,"));
    }

    /// A multi-shape enum dispatches on the node's kind to each variant's own
    /// `Recover`, and anything else is an `UnexpectedKind` diagnostic plus
    /// `None` — never a `Result::Err`.
    #[test]
    fn multi_shape_enum_dispatches_and_reports_unexpected_kind() {
        let out = partial(&every_shape_grammar(), None);
        let body = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Value {");
        assert!(body.contains(
            "\"ident\" => <Ident as runtime::Recover<'tree>>::recover(_node, _diags).map(Value::Ident),"
        ));
        assert!(body.contains(
            "\"num\" => <Num as runtime::Recover<'tree>>::recover(_node, _diags).map(Value::Num),"
        ));
        assert!(body.contains("runtime::DiagnosticKind::UnexpectedKind"));
        assert!(body.contains("expected: \"Value\","));
        assert!(body.contains("None"));
        assert!(!body.contains("Err("));
        assert!(!body.contains("Token"));
    }

    /// A shape that can also be a bare token keeps its `Token(String)`
    /// variant, returned before the kind dispatch for anonymous nodes.
    #[test]
    fn multi_shape_enum_with_token_variant_handles_anonymous_nodes_first() {
        let g = Grammar::from_rules([
            p(
                "decl",
                Field(
                    "value".into(),
                    Box::new(Choice(vec![nt("ident"), TerminalLiteral("'nil'".into())])),
                ),
            ),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);
        let body = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Value {");
        let token_pos = body
            .find("return Some(Value::Token(_node.text().to_string()));")
            .unwrap();
        let match_pos = body.find("match _node.node.kind() {").unwrap();
        assert!(token_pos < match_pos);
        let decl = item_body(&out, "pub enum Value {");
        assert!(decl.contains("Token(String),"));
    }

    /// The runtime module carries the diagnostics machinery next to the
    /// shared `Pragma`, and no `BuildError`.
    #[test]
    fn runtime_module_has_diagnostics_machinery() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        for needle in [
            "pub struct Pragma {",
            "pub enum DiagnosticKind {",
            "pub struct Diagnostic {",
            "pub struct Recovered<T> {",
            "pub trait Recover<'tree>: Sized {",
            "pub fn recover<'tree, T: Recover<'tree>>(",
            "pub fn is_recovery(",
        ] {
            assert!(
                runtime.contains(needle),
                "runtime module missing `{needle}`"
            );
        }
        assert!(!runtime.contains("BuildError"));
    }

    /// The `SourceNode` helpers follow the strict ones they build on and
    /// precede every struct.
    #[test]
    fn recovery_helpers_sit_between_strict_helpers_and_structs() {
        let out = partial(&every_shape_grammar(), None);
        let child_by_field = out.find("pub fn child_by_field(").unwrap();
        let recover_field = out.find("pub fn recover_field<").unwrap();
        let first_struct = out.find("pub struct Program {").unwrap();
        assert!(child_by_field < recover_field);
        assert!(recover_field < first_struct);
    }

    /// Strict output contains none of partial mode's machinery, and no
    /// optional fields.
    #[test]
    fn strict_output_has_no_partial_machinery() {
        let out = strict(&every_shape_grammar());
        for needle in ["Recover", "Diagnostic", "recover_field", "is_recovery"] {
            assert!(
                !out.contains(needle),
                "strict output must not mention `{needle}`"
            );
        }
        let decl = item_body(&out, "pub struct Decl {");
        assert!(!decl.contains("Option<"));
    }

    /// With a merge config, a merged-away kind stays private, still gets its
    /// `Recover` impl, and the merge entry's enum dispatches to it; a
    /// `passthrough` rename is used for the variant payload.
    #[test]
    fn merge_config_enum_dispatches_to_private_structs_and_renamed_kinds() {
        let g = Grammar::from_rules([
            p(
                "program",
                Choice(vec![
                    nt("for_statement"),
                    nt("while_statement"),
                    nt("comment"),
                ]),
            ),
            p("for_statement", TerminalLiteral("'for'".into())),
            p("while_statement", TerminalLiteral("'while'".into())),
            p("comment", TerminalLiteral("'#'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![PassthroughEntry {
                kind: "comment".to_string(),
                target: "DocComment".to_string(),
            }],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));

        assert!(!out.contains("pub struct ForStatement {"));
        assert!(out.contains("impl<'tree> runtime::Recover<'tree> for ForStatement {"));
        assert!(out.contains("impl<'tree> runtime::Recover<'tree> for DocComment {"));

        let loop_impl = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Loop {");
        assert!(loop_impl.contains(
            "\"for_statement\" => <ForStatement as runtime::Recover<'tree>>::recover(_node, _diags).map(Loop::ForStatement),"
        ));
    }
}
