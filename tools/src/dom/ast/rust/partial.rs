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
    AstFieldSpec, AstNodeSpec, MergeConfig, children_enum_name, field_element_is_enum, indent,
    kind_display_name, rust_field_ident, shape_has_error, shape_kinds, variant_name,
};

/// The `pub use` that makes the generated `runtime` module offer, under the
/// paths it has always had, the plain-data items that now live in the
/// `ts_bnf_runtime` crate: positions (`Pragma`, `Span`, `Token`,
/// `ErrorRegion`), diagnostics (`Diagnostic`, `DiagnosticKind`, `Recovered`,
/// `DEFAULT_MAX_DEPTH`) and the `Reflect` view with its `walk`/`Walk`/`Visit`.
/// Declared once there, shared by every generated crate. Flush-left.
pub(super) const RUNTIME_REEXPORTS: &str = indoc! {r#"
    // The plain data of the typed tree — positions, diagnostics, the `Reflect`
    // view and its `walk` — is declared once, in the `ts_bnf_runtime` crate that
    // every generated crate shares, so the typed trees of two languages have the
    // very same types. They are re-exported here so that `ast::runtime::Span`,
    // `ast::runtime::Reflect` and the rest are the paths they have always had.
    pub use ts_bnf_runtime::{
        DEFAULT_MAX_DEPTH, Diagnostic, DiagnosticKind, ErrorRegion, Pragma, Recovered, Reflect,
        Span, Token, Visit, Walk, walk,
    };
"#};

/// The items partial mode adds to the generated `runtime` module, flush-left
/// (the caller indents them into the module), after [`RUNTIME_REEXPORTS`] and
/// the `From<SourceNode>` conversions: what touches tree-sitter and so cannot
/// live in the tree-sitter-free `ts_bnf_runtime` crate — the `Recover` trait,
/// the depth-limiting `Context`, and the `recover` entry points with their
/// syntax-error walk — plus the two small helpers that build the shared types
/// from a `SourceNode`. Takes the place of strict mode's `BuildError`.
pub(super) const RUNTIME_ITEMS: &str = indoc! {r#"
    /// A diagnostic of `kind` located at `node`. A free function rather than
    /// `Diagnostic::at`: `Diagnostic` lives in `ts_bnf_runtime` and an inherent
    /// method cannot be added to a foreign type from here. Only the generated
    /// code in this file builds diagnostics, so it is not public.
    pub(super) fn diagnostic_at(
        node: super::super::visitor::SourceNode<'_>,
        kind: DiagnosticKind,
    ) -> Diagnostic {
        Diagnostic { kind, span: Span::from(node) }
    }

    impl From<super::super::visitor::SourceNode<'_>> for ErrorRegion {
        fn from(node: super::super::visitor::SourceNode<'_>) -> Self {
            ErrorRegion { span: Span::from(node), text: node.text().to_string() }
        }
    }

    /// Builds a typed node from a parse-tree node without ever aborting:
    /// problems are pushed onto `cx.diagnostics`, and whatever could not be
    /// built is simply absent from the result.
    pub trait Recover<'tree>: Sized {
        /// Builds `Self` from `node`. `None` only when `node` cannot be a
        /// `Self` at all (a multi-shape enum given a kind outside its set).
        fn recover(
            node: super::super::visitor::SourceNode<'tree>,
            cx: &mut Context,
        ) -> Option<Self>;
    }

    /// What a recovery carries down the tree: the problems found so far, and
    /// how many levels deep it is and may go.
    #[derive(Debug)]
    pub struct Context {
        /// The problems found so far, in the order they were found.
        pub diagnostics: Vec<Diagnostic>,
        depth: usize,
        max_depth: usize,
    }

    impl Context {
        /// A fresh context that builds nesting up to `max_depth` levels deep.
        pub fn new(max_depth: usize) -> Self {
            Context { diagnostics: Vec::new(), depth: 0, max_depth }
        }

        /// Enters one more level of nesting, for `node`. Returns `false` — after
        /// reporting [`DiagnosticKind::TooDeep`] at `node` — if that would pass
        /// the limit, and the caller must then not build `node`. A `true` must be
        /// paired with a [`Context::leave`].
        pub fn enter(&mut self, node: super::super::visitor::SourceNode<'_>) -> bool {
            if self.depth >= self.max_depth {
                let problem = DiagnosticKind::TooDeep { limit: self.max_depth };
                self.diagnostics.push(diagnostic_at(node, problem));
                return false;
            }
            self.depth += 1;
            true
        }

        /// Leaves the level [`Context::enter`] entered.
        pub fn leave(&mut self) {
            self.depth -= 1;
        }
    }

    /// `true` for tree-sitter's recovery nodes (`ERROR` and `MISSING`). Their
    /// `kind()` is unreliable as a type tag — a `MISSING` node reports the
    /// kind it stands in for — so they are never built into typed values.
    pub fn is_recovery(node: super::super::visitor::SourceNode<'_>) -> bool {
        node.node.is_missing() || node.node.is_error()
    }

    /// Recovers a `T` from `root`, returning it together with every
    /// diagnostic found, ordered by position in the source. Nesting deeper than
    /// [`DEFAULT_MAX_DEPTH`] is not built: see [`recover_with_limit`].
    ///
    /// Diagnostics come from two sources that never overlap: a walk over the
    /// tree's `ERROR`/`MISSING` nodes (whether or not they sit at a field
    /// position), and the build itself, which reports only structural
    /// problems (an absent field, an unexpected kind, a subtree too deep).
    pub fn recover<'tree, T: Recover<'tree>>(
        root: super::super::visitor::SourceNode<'tree>,
    ) -> Recovered<T> {
        recover_with_limit(root, DEFAULT_MAX_DEPTH)
    }

    /// Like [`recover`], but builds nesting up to `max_depth` levels deep
    /// instead of [`DEFAULT_MAX_DEPTH`]. The root is level 1, and every
    /// non-leaf node below it is one more. A subtree that would be level
    /// `max_depth + 1` is not built — its field is `None`, or it is left out of
    /// its list — and a [`DiagnosticKind::TooDeep`] at its span says so; the
    /// text it covered is `&source[span.byte_range]`. Raise the limit only if
    /// the code runs on a stack big enough for it: a stack overflow aborts the
    /// process.
    pub fn recover_with_limit<'tree, T: Recover<'tree>>(
        root: super::super::visitor::SourceNode<'tree>,
        max_depth: usize,
    ) -> Recovered<T> {
        let mut cx = Context::new(max_depth);
        let value = if is_recovery(root) { None } else { T::recover(root, &mut cx) };
        if root.node.has_error() {
            collect_syntax_problems(root, &mut cx.diagnostics);
        }
        let mut diagnostics = cx.diagnostics;
        diagnostics.sort_by_key(|d| d.span.byte_range.start);
        Recovered { value, diagnostics }
    }

    /// Appends a diagnostic for every `MISSING` node under `root`, and for every
    /// `ERROR` node not already inside one. `inside_error` counts the `ERROR`
    /// nodes the cursor is in: tree-sitter nests an `ERROR` in an `ERROR` of the
    /// very same range (a lexer failure inside an error region, for instance),
    /// and the outermost one already covers it. A `MISSING` node is still
    /// reported there, since it says what the parser expected. Only subtrees
    /// that contain an error are descended into (`has_error` is a
    /// constant-time flag), so a clean tree costs nothing.
    ///
    /// Iterative, with a `TreeCursor`, not recursive: it is bounded by the
    /// input, not by the stack, however deeply the tree nests.
    fn collect_syntax_problems(
        root: super::super::visitor::SourceNode<'_>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = root.node.walk();
        let mut inside_error = 0usize;
        loop {
            let node = super::super::visitor::SourceNode {
                node: cursor.node(),
                source: root.source,
            };
            if node.node.is_missing() {
                let expected = node.node.kind().to_string();
                diagnostics.push(diagnostic_at(node, DiagnosticKind::Missing { expected }));
            } else if node.node.is_error() && inside_error == 0 {
                diagnostics.push(diagnostic_at(node, DiagnosticKind::Error));
            }
            // Down, into a subtree that contains an error.
            if node.node.has_error() && cursor.goto_first_child() {
                if node.node.is_error() {
                    inside_error += 1;
                }
                continue;
            }
            // Across, or back up until there is a sibling to go across to.
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
                if cursor.node().is_error() {
                    inside_error -= 1;
                }
            }
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
                cx: &mut runtime::Context,
            ) -> Option<T> {
                match self.child_by_field(field_name) {
                    Some(child) if runtime::is_recovery(child) => None,
                    Some(child) => T::recover(child, cx),
                    None => {
                        self.report_absent_field(field_name, kind, cx);
                        None
                    }
                }
            }

            /// Like [`Self::recover_field`], for a field whose value is a bare
            /// token: the token, with its text and position.
            pub fn field_token(
                &self,
                field_name: &'static str,
                kind: &'static str,
                cx: &mut runtime::Context,
            ) -> Option<runtime::Token> {
                match self.child_by_field(field_name) {
                    Some(child) if runtime::is_recovery(child) => None,
                    Some(child) => Some(runtime::Token::from(child)),
                    None => {
                        self.report_absent_field(field_name, kind, cx);
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
                cx: &mut runtime::Context,
            ) -> Vec<T> {
                let mut out = Vec::new();
                for child in self.children_by_field(field_name) {
                    if runtime::is_recovery(child) {
                        continue;
                    }
                    if let Some(value) = T::recover(child, cx) {
                        out.push(value);
                    }
                }
                out
            }

            /// Every token at `field_name`, in order, skipping `ERROR`/`MISSING`
            /// nodes.
            pub fn field_tokens(&self, field_name: &str) -> Vec<runtime::Token> {
                let mut out = Vec::new();
                for child in self.children_by_field(field_name) {
                    if !runtime::is_recovery(child) {
                        out.push(runtime::Token::from(child));
                    }
                }
                out
            }

            /// Recovers every *unlabelled* child — the ones no field label
            /// captures — as a `T`, in source order. This is what keeps a
            /// grammar without field labels from producing empty structs.
            ///
            /// `kinds` is every named kind the grammar allows there and
            /// `expected` names `T`, for the diagnostic when something else
            /// shows up. `tokens` says whether a string literal (a keyword or
            /// punctuation, an anonymous node) is a child here too: if so `T`
            /// has a `Token(runtime::Token)` variant to take it, and if not,
            /// anonymous nodes are dropped. An `ERROR` node is always kept —
            /// `T` is a children enum, whose `Error` variant takes it — so a
            /// broken region stays where it sits. Never captured: extras such
            /// as comments (they can appear anywhere, so they are not part of
            /// the grammar's structure) and `MISSING` nodes (a zero-width
            /// point, reported by `runtime::recover`'s syntax walk).
            pub fn recover_children<T: runtime::Recover<'tree>>(
                &self,
                kinds: &[&str],
                tokens: bool,
                expected: &'static str,
                cx: &mut runtime::Context,
            ) -> Vec<T> {
                let mut out = Vec::new();
                for child in self.unlabelled_children() {
                    if child.node.is_error() {
                        if let Some(value) = T::recover(child, cx) {
                            out.push(value);
                        }
                    } else if !child.node.is_named() {
                        if tokens {
                            if let Some(value) = T::recover(child, cx) {
                                out.push(value);
                            }
                        }
                    } else if kinds.contains(&child.node.kind()) {
                        if let Some(value) = T::recover(child, cx) {
                            out.push(value);
                        }
                    } else {
                        let problem = runtime::DiagnosticKind::UnexpectedKind {
                            expected,
                            found: child.node.kind().to_string(),
                        };
                        cx.diagnostics.push(runtime::diagnostic_at(child, problem));
                    }
                }
                out
            }

            /// Every child with no field label, named or anonymous, in source
            /// order, minus extras and `MISSING` nodes; `ERROR` nodes are kept
            /// (outermost only: they are not descended into). Tree-sitter flags
            /// the `ERROR` nodes it creates while skipping tokens as extras
            /// too, so the extra check must not apply to them. The walk behind
            /// [`Self::recover_children`].
            fn unlabelled_children(&self) -> Vec<super::visitor::SourceNode<'tree>> {
                let mut out = Vec::new();
                let mut cursor = self.node.walk();
                if !cursor.goto_first_child() {
                    return out;
                }
                loop {
                    let node = cursor.node();
                    if (node.is_error() || !node.is_extra()) && cursor.field_name().is_none() {
                        let child = super::visitor::SourceNode { node, source: self.source };
                        if !child.node.is_missing() {
                            out.push(child);
                        }
                    }
                    if !cursor.goto_next_sibling() {
                        break;
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
                cx: &mut runtime::Context,
            ) {
                if self.node.has_error() {
                    let problem = runtime::DiagnosticKind::MissingField { kind, field: field_name };
                    cx.diagnostics.push(runtime::diagnostic_at(*self, problem));
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
/// The generated `recover` function's parameters are `_node` and `_cx`,
/// never `node`/`diags`: a field labelled `node` or `diags` would otherwise
/// bind a local of that name and shadow the parameter for every later field.
/// Grammar field labels can't start with `_` (see
/// `check_field_labels_are_representable`), so the underscore names can never
/// collide with a binding. (`_cx` also never trips `unused_variables`,
/// even in a struct with no field that needs it.)
///
/// Panics if `field`'s target resolves to no possible shape at all, same as
/// the strict `field_try_from` — a field always has at least one target kind.
fn field_recover(kind: &str, field_name: &str, field: &AstFieldSpec) -> String {
    let target = &field.target;
    let ident = rust_field_ident(field_name);

    if field.multiple && target.named.is_empty() && target.anonymous_token {
        format!("let {ident} = _node.field_tokens(\"{field_name}\");\n")
    } else if field.multiple {
        format!("let {ident} = _node.recover_fields(\"{field_name}\", _cx);\n")
    } else if target.named.is_empty() && target.anonymous_token {
        format!("let {ident} = _node.field_token(\"{field_name}\", \"{kind}\", _cx);\n")
    } else if !target.named.is_empty() {
        format!("let {ident} = _node.recover_field(\"{field_name}\", \"{kind}\", _cx);\n")
    } else {
        unreachable!("a field always resolves to at least one target kind")
    }
}

/// Builds the statement that recovers a non-leaf kind's unlabelled children
/// into its injected `_children` list: one call to `recover_children`
/// ([`fmt_recover_helpers`]), passing every named kind the grammar allows
/// there, whether a literal can appear too (so the children enum has a `Token`
/// variant), and the name of the kind's own children enum
/// ([`children_enum_name`]) — the list's element type, which always has an
/// `Error` variant for a broken region. A kind whose children are all
/// labelled passes no kinds and no tokens, so only regions land in the list.
fn children_recover(spec: &AstNodeSpec, kind: &str, merge_config: Option<&MergeConfig>) -> String {
    let mut kinds = Vec::new();
    let mut tokens = false;
    if let Some(children) = &spec.children {
        for named in &children.target.named {
            kinds.push(format!("\"{named}\""));
        }
        tokens = children.target.anonymous_token;
    }
    let kinds_text = kinds.join(", ");
    let expected = children_enum_name(kind, merge_config);
    format!(
        "let _children = _node.recover_children(&[{kinds_text}], {tokens}, \"{expected}\", _cx);\n"
    )
}

/// Builds one kind's full `impl<'tree> runtime::Recover<'tree> for
/// {struct_name}` block — the partial-mode replacement for the strict
/// `TryFrom` impl: `Span` first, then one recovery statement per field
/// ([`field_recover`]), then the unlabelled children ([`children_recover`]) for
/// every non-leaf kind, then `_text` for a leaf kind, then the struct literal.
/// A non-leaf kind brackets all of it in `_cx.enter(_node)` / `_cx.leave()`, the
/// one place the depth limit is counted: past the limit it returns `None`
/// without building anything, so every route into the recursion — a field, a
/// list, a children enum — is covered by this single check. Otherwise always
/// returns `Some`: whether a node is recoverable at all
/// (`ERROR`/`MISSING`, or a kind outside an enum's set) is decided by the
/// caller — a `SourceNode` helper or a multi-shape enum — before this runs.
///
/// `struct_name` is resolved by the caller, same as the declaration it
/// pairs with, so a `passthrough` rename only changes one call site.
pub(super) fn fmt_struct_recover(
    struct_name: &str,
    kind: &str,
    spec: &AstNodeSpec,
    merge_config: Option<&MergeConfig>,
) -> String {
    let mut statements = String::new();
    let mut field_list = Vec::new();
    for (field, field_spec) in &spec.fields {
        statements.push_str(&field_recover(kind, field, field_spec));
        field_list.push(rust_field_ident(field));
    }
    if !spec.is_leaf {
        statements.push_str(&children_recover(spec, kind, merge_config));
        field_list.push("_children".to_string());
    }
    if spec.is_leaf {
        statements.push_str("let _text = _node.text().to_string();\n");
        field_list.push("_text".to_string());
    }

    // A leaf has nothing to recurse into, so only a non-leaf kind counts as a
    // level: it enters before reading anything and leaves just before it
    // returns. Nothing between the two returns early, so they always pair.
    let mut body = String::new();
    if !spec.is_leaf {
        body.push_str("if !_cx.enter(_node) {\n    return None;\n}\n");
    }
    body.push_str("let _span = runtime::Span::from(_node);\n");
    body.push_str(&statements);
    if !spec.is_leaf {
        body.push_str("_cx.leave();\n");
    }
    let field_list_text = field_list.join(", ");
    let name = struct_name;
    body.push_str(&format!("Some({name} {{ _span, {field_list_text} }})"));
    let body_text = indent(&body, 8);

    formatdoc! {r#"
        impl<'tree> runtime::Recover<'tree> for {name} {{
            fn recover(
                _node: super::visitor::SourceNode<'tree>,
                _cx: &mut runtime::Context,
            ) -> Option<Self> {{
        {body_text}
            }}
        }}
    "#}
}

/// Builds one multi-shape (or `merge`) enum's `Recover` impl — the
/// partial-mode counterpart of the strict enum `TryFrom`: a bare-token early
/// return when `shape` includes the `"$token"` sentinel, then a `match` on
/// `node.node.kind()` with one arm per real kind delegating to that kind's
/// own `Recover`. A kind outside the set yields an `UnexpectedKind`
/// diagnostic and `None` instead of an error. A shape with no real kind at all
/// has nothing to dispatch on, so it gets the diagnostic without the `match`
/// (a `match` with only a catch-all arm is a clippy warning in generated
/// code). Each variant goes through
/// [`kind_display_name`], so a `passthrough`-renamed kind's variant still
/// names the type it is actually declared under.
pub(super) fn fmt_multi_shape_enum_recover(
    shape: &[String],
    name: &str,
    merge_config: Option<&MergeConfig>,
) -> String {
    let (clean_shape, has_token) = shape_kinds(shape);
    let has_error = shape_has_error(shape);

    let mut arms = String::new();
    for kind in clean_shape {
        let payload = kind_display_name(kind, merge_config);
        let variant = variant_name(kind, merge_config, has_token, has_error);
        arms.push_str(&format!(
            "\"{kind}\" => <{payload} as runtime::Recover<'tree>>::recover(_node, _cx).map({name}::{variant}),\n"
        ));
    }

    // The diagnostic for a kind none of the arms took, and `None`.
    let unexpected_kind = formatdoc! {r#"
        let problem = runtime::DiagnosticKind::UnexpectedKind {{
            expected: "{name}",
            found: found.to_string(),
        }};
        _cx.diagnostics.push(runtime::diagnostic_at(_node, problem));
        None
    "#};
    // With no kind to dispatch on (an enum that can only hold an `ERROR`
    // region and/or a bare token, as the children enum of a struct whose
    // children are all labelled fields is) a `match` would have the one
    // catch-all arm, which clippy flags (`infallible_destructuring_match`).
    let dispatch = if arms.is_empty() {
        format!(
            "let found = _node.node.kind();\n{}",
            unexpected_kind.trim_end()
        )
    } else {
        format!(
            "match _node.node.kind() {{\n{}\n    found => {{\n{}\n    }}\n}}",
            indent(arms.trim_end(), 4),
            indent(unexpected_kind.trim_end(), 8)
        )
    };
    let dispatch_text = indent(&dispatch, 8);

    // Checked before the token branch: an `ERROR` node is named, but its kind
    // is `"ERROR"`, which no arm below would take.
    let mut early_returns = String::new();
    if has_error {
        early_returns.push_str(&formatdoc! {r#"
            if _node.node.is_error() {{
                return Some({name}::Error(runtime::ErrorRegion::from(_node)));
            }}
        "#});
    }
    if has_token {
        early_returns.push_str(&formatdoc! {r#"
            if !_node.node.is_named() {{
                return Some({name}::Token(runtime::Token::from(_node)));
            }}
        "#});
    }
    let token_early_return_text = indent(early_returns.trim_end(), 8);

    formatdoc! {r#"
        impl<'tree> runtime::Recover<'tree> for {name} {{
            fn recover(
                _node: super::visitor::SourceNode<'tree>,
                _cx: &mut runtime::Context,
            ) -> Option<Self> {{
        {token_early_return_text}
        {dispatch_text}
            }}
        }}
    "#}
}

/// Builds the statements that add one field's values to the `out` vector
/// inside a generated `children()`: a loop for a repeated list, an `if let` for
/// an optional single value, each value pushed as a `(label, node)` pair. An
/// enum-typed element is pushed as `v.inner()`, the node the enum holds, so the
/// enum is transparent to whoever reads `children()`; a struct or a token is
/// pushed as itself. `access` is the Rust expression naming the field
/// (`self.name`) and `label` is the grammar's own field label, verbatim —
/// not the escaped Rust identifier. (Unlabelled children are not fields; the
/// caller pushes them with a `None` label itself.)
///
/// Deliberately no `if let … && …`: the generated crate is pinned to edition
/// 2021, where let-chains don't exist.
fn reflect_push(
    access: &str,
    label: &str,
    field: &AstFieldSpec,
    merge_config: Option<&MergeConfig>,
) -> String {
    let item = if field_element_is_enum(field, merge_config) {
        "v.inner()"
    } else {
        "v"
    };
    let label = format!("Some(\"{label}\")");
    if field.multiple {
        format!("for v in &{access} {{\n    out.push(({label}, {item}));\n}}\n")
    } else {
        format!("if let Some(v) = &{access} {{\n    out.push(({label}, {item}));\n}}\n")
    }
}

/// Builds one kind's `impl runtime::Reflect for {struct_name}`: the generic,
/// read-only view of the node that lets one loop walk the whole typed tree.
///
/// - `kind()` is the *grammar* kind (`kind`), not the Rust name, so a
///   `passthrough`-renamed struct still reports the kind tree-sitter does.
/// - `text()` is `Some` only for a leaf kind, whose `_text` is its whole
///   source.
/// - `children()` gathers every labelled field's value and the unlabelled
///   `_children`, then sorts by start byte: fields and unlabelled children
///   interleave in the source, and the struct stores them apart. The sort is
///   stable, so equal starts (zero-width nodes) keep declaration order. A
///   kind with nothing to gather returns an empty `Vec` directly.
/// - `is_named()` is `true`: a struct is always a named rule; only a
///   [`runtime::Token`](RUNTIME_ITEMS) is not.
///
/// `struct_name` is resolved by the caller, same as the declaration it pairs
/// with.
pub(super) fn fmt_struct_reflect(
    struct_name: &str,
    kind: &str,
    spec: &AstNodeSpec,
    merge_config: Option<&MergeConfig>,
) -> String {
    let text = if spec.is_leaf {
        "Some(self._text.as_str())"
    } else {
        "None"
    };

    let mut pushes = String::new();
    for (field, field_spec) in &spec.fields {
        let access = format!("self.{}", rust_field_ident(field));
        pushes.push_str(&reflect_push(&access, field, field_spec, merge_config));
    }
    if !spec.is_leaf {
        // Always a children enum, so always unwrapped to the node it holds.
        pushes.push_str("for v in &self._children {\n    out.push((None, v.inner()));\n}\n");
    }
    let children_body = if pushes.is_empty() {
        "Vec::new()".to_string()
    } else {
        format!(
            "let mut out: Vec<(Option<&'static str>, &dyn runtime::Reflect)> = Vec::new();\n{pushes}\
             out.sort_by_key(|c| runtime::Reflect::span(c.1).byte_range.start);\nout"
        )
    };
    let children_text = indent(&children_body, 8);
    let name = struct_name;

    formatdoc! {r#"
        impl runtime::Reflect for {name} {{
            fn kind(&self) -> &str {{
                "{kind}"
            }}

            fn span(&self) -> &runtime::Span {{
                &self._span
            }}

            fn text(&self) -> Option<&str> {{
                {text}
            }}

            fn children(&self) -> Vec<(Option<&'static str>, &dyn runtime::Reflect)> {{
        {children_text}
            }}

            fn is_named(&self) -> bool {{
                true
            }}
        }}
    "#}
}

/// Builds one multi-shape (or `merge`) enum's `impl runtime::Reflect`, plus
/// the inherent `inner()` that makes it transparent: `inner()` returns the
/// node the active variant holds, every `Reflect` method simply delegates to
/// it, and a parent's `children()` pushes `inner()` rather than the enum, so
/// a consumer walking the tree never meets the enum at all. A `Token` variant
/// holds a [`runtime::Token`](RUNTIME_ITEMS), which is itself `Reflect`.
///
/// Variants go through [`kind_display_name`], matching the enum declaration,
/// so a `passthrough`-renamed kind's variant names the type it is declared
/// under.
pub(super) fn fmt_multi_shape_enum_reflect(
    shape: &[String],
    name: &str,
    merge_config: Option<&MergeConfig>,
) -> String {
    let (clean_shape, has_token) = shape_kinds(shape);
    let has_error = shape_has_error(shape);

    let mut arms = String::new();
    for kind in clean_shape {
        let variant = variant_name(kind, merge_config, has_token, has_error);
        arms.push_str(&format!("{name}::{variant}(v) => v,\n"));
    }
    if has_token {
        arms.push_str(&format!("{name}::Token(v) => v,\n"));
    }
    if has_error {
        arms.push_str(&format!("{name}::Error(v) => v,\n"));
    }
    let arms_text = indent(arms.trim_end(), 12);

    formatdoc! {r#"
        impl runtime::Reflect for {name} {{
            fn kind(&self) -> &str {{
                runtime::Reflect::kind(self.inner())
            }}

            fn span(&self) -> &runtime::Span {{
                runtime::Reflect::span(self.inner())
            }}

            fn text(&self) -> Option<&str> {{
                runtime::Reflect::text(self.inner())
            }}

            fn children(&self) -> Vec<(Option<&'static str>, &dyn runtime::Reflect)> {{
                runtime::Reflect::children(self.inner())
            }}

            fn is_named(&self) -> bool {{
                runtime::Reflect::is_named(self.inner())
            }}
        }}

        impl {name} {{
            /// The node this value holds: what a parent's `children()` yields,
            /// so the enum itself never shows up when walking the tree.
            pub fn inner(&self) -> &dyn runtime::Reflect {{
                match self {{
        {arms_text}
                }}
            }}
        }}
    "#}
}

#[cfg(test)]
mod tests {
    use super::super::RustAst;
    use super::{AstFieldSpec, field_recover, fmt_multi_shape_enum_recover};
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

    /// Renders `grammar` in [`BuildMode::Strict`], headerless, with a merge
    /// config.
    fn strict_with(grammar: &Grammar, merge_config: &MergeConfig) -> String {
        RustAst::new(grammar, "g", true, Some(merge_config), BuildMode::Strict)
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
    /// injected `_span` is never optional.
    #[test]
    fn struct_fields_single_is_option_multiple_stays_vec() {
        let out = partial(&every_shape_grammar(), None);

        let decl = item_body(&out, "pub struct Decl {");
        assert!(decl.contains("pub _span: runtime::Span,"));
        assert!(decl.contains("pub target: Option<Ident>,"));
        assert!(decl.contains("pub value: Option<Value>,"));
        assert!(decl.contains("pub ops: Vec<runtime::Token>,"));
        assert!(decl.contains("pub tag: Option<runtime::Token>,"));

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
        assert!(decl.contains("_node.recover_field(\"target\", \"decl\", _cx)"));
        assert!(decl.contains("_node.recover_field(\"value\", \"decl\", _cx)"));
        assert!(decl.contains("_node.field_tokens(\"ops\")"));
        assert!(decl.contains("_node.field_token(\"tag\", \"decl\", _cx)"));
        assert!(decl.contains("Some(Decl { _span, target, value, ops, tag, _children })"));

        let program = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Program {");
        assert!(program.contains("_node.recover_fields(\"items\", _cx)"));
    }

    /// A leaf builds its `_text` and returns it in the struct literal.
    #[test]
    fn leaf_recover_builds_text() {
        let out = partial(&every_shape_grammar(), None);
        let ident = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Ident {");
        assert!(ident.contains("let _text = _node.text().to_string();"));
        assert!(ident.contains("Some(Ident { _span, _text })"));
    }

    /// The generated `recover` always names its parameters `_node`/`_cx`,
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
                body.contains("_cx: &mut runtime::Context,"),
                "{kind}: {body}"
            );
        }
        // A field labelled `node` binds a local `node`, distinct from `_node`.
        let one_field = item_body(&out, "impl<'tree> runtime::Recover<'tree> for OneField {");
        assert!(
            one_field.contains("let node = _node.recover_field(\"node\", \"one_field\", _cx);")
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
        assert!(body.contains("let r#type = _node.recover_field(\"type\", \"stmt\", _cx);"));
        assert!(body.contains("Some(Stmt { _span, r#type, _children })"));
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
            "\"ident\" => <Ident as runtime::Recover<'tree>>::recover(_node, _cx).map(Value::Ident),"
        ));
        assert!(body.contains(
            "\"num\" => <Num as runtime::Recover<'tree>>::recover(_node, _cx).map(Value::Num),"
        ));
        assert!(body.contains("runtime::DiagnosticKind::UnexpectedKind"));
        assert!(body.contains("expected: \"Value\","));
        assert!(body.contains("None"));
        assert!(!body.contains("Err("));
        assert!(!body.contains("Token"));
    }

    /// An enum with no kind of its own to dispatch on — only an `ERROR` region
    /// and/or a bare token, which is what the children enum of a struct whose
    /// children are all labelled fields is — reports every other kind as
    /// unexpected without a `match` that has nothing but a catch-all arm, which
    /// clippy rejects (`infallible_destructuring_match`).
    #[test]
    fn multi_shape_enum_with_no_kinds_has_no_single_arm_match() {
        for shape in [
            vec!["$error".to_string()],
            vec!["$token".to_string(), "$error".to_string()],
        ] {
            let out = fmt_multi_shape_enum_recover(&shape, "ProgramChild", None);
            assert!(!out.contains("match "), "{out}");
            assert!(out.contains("let found = _node.node.kind();"), "{out}");
            assert!(out.contains("expected: \"ProgramChild\","), "{out}");
            assert!(out.contains("found: found.to_string(),"), "{out}");
            assert!(out.trim_end().ends_with("None\n    }\n}"), "{out}");
        }
    }

    /// A shape that can also be a bare token keeps its `Token(runtime::Token)`
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
            .find("return Some(Value::Token(runtime::Token::from(_node)));")
            .unwrap();
        let match_pos = body.find("match _node.node.kind() {").unwrap();
        assert!(token_pos < match_pos);
        let decl = item_body(&out, "pub enum Value {");
        assert!(decl.contains("Token(runtime::Token),"));
    }

    /// The runtime module carries the recovery machinery that touches
    /// tree-sitter — the `Recover` trait, the entry points, the free function
    /// that builds a diagnostic — and no `BuildError`. The plain data it works
    /// on is not declared here; see `partial_runtime_reexports_the_shared_crate_items`.
    #[test]
    fn runtime_module_has_the_recovery_machinery() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        for needle in [
            "pub trait Recover<'tree>: Sized {",
            "pub struct Context {",
            "pub fn recover<'tree, T: Recover<'tree>>(",
            "pub fn recover_with_limit<'tree, T: Recover<'tree>>(",
            "pub fn is_recovery(",
            "pub(super) fn diagnostic_at(",
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
            "\"for_statement\" => <ForStatement as runtime::Recover<'tree>>::recover(_node, _cx).map(Loop::ForStatement),"
        ));
    }

    // ── unlabelled children (`_children`) ───────────────────────────────

    /// A grammar with no field labels at all, like the one that prompted
    /// this feature: every struct would be `_span`-only without
    /// `_children`. Covers a single child kind (`program`, `decl`), a
    /// multi-kind choice (`stmt`, `assign`, `expr`), and leaves.
    fn unlabelled_grammar() -> Grammar {
        Grammar::from_rules([
            p("program", ZeroOrMore(Box::new(nt("stmt")))),
            p("stmt", Choice(vec![nt("decl"), nt("assign")])),
            p(
                "decl",
                Sequence(vec![TerminalLiteral("'var'".into()), nt("ident")]),
            ),
            p(
                "assign",
                Sequence(vec![nt("ident"), TerminalLiteral("'='".into()), nt("expr")]),
            ),
            p("expr", Choice(vec![nt("ident"), nt("num")])),
            p("ident", TerminalPattern("/[a-z]+/".into())),
            p("num", TerminalPattern("/[0-9]+/".into())),
        ])
    }

    /// Every non-leaf kind owns a children enum, even when its children are a
    /// single kind (`Vec<ProgramChild>`, not `Vec<Stmt>`) — a broken region can
    /// sit under any of them. A leaf has none.
    #[test]
    fn every_non_leaf_kind_has_a_children_enum() {
        let out = partial(&unlabelled_grammar(), None);

        for kind in ["Program", "Stmt", "Decl", "Assign", "Expr"] {
            let body = item_body(&out, &format!("pub struct {kind} {{"));
            assert!(
                body.contains(&format!("pub _children: Vec<{kind}Child>,")),
                "{kind}: {body}"
            );
            let children = item_body(&out, &format!("pub enum {kind}Child {{"));
            assert!(
                children.contains("Error(runtime::ErrorRegion),"),
                "{kind}Child"
            );
        }

        let ident = item_body(&out, "pub struct Ident {");
        assert!(!ident.contains("_children"));
        assert!(ident.contains("pub _text: String,"));
        assert!(
            !out.contains("pub enum IdentChild"),
            "a leaf has no children enum"
        );
    }

    /// The per-kind enum has one variant per possible child kind, then the
    /// `Error` variant, and its own `Recover` impl.
    #[test]
    fn unlabelled_children_enum_has_a_variant_per_kind() {
        let out = partial(&unlabelled_grammar(), None);
        let stmt_child = item_body(&out, "pub enum StmtChild {");
        assert!(stmt_child.contains("Decl(Decl),"));
        assert!(stmt_child.contains("Assign(Assign),"));
        assert!(stmt_child.contains("Error(runtime::ErrorRegion),"));
        assert!(
            !stmt_child.contains("Token"),
            "no literal can be a stmt's child"
        );
        assert!(out.contains("impl<'tree> runtime::Recover<'tree> for StmtChild {"));

        // A single-kind list is an enum too: that is what lets it hold a region.
        let program_child = item_body(&out, "pub enum ProgramChild {");
        assert!(program_child.contains("Stmt(Stmt),"));
        assert!(program_child.contains("Error(runtime::ErrorRegion),"));
    }

    /// The generated `recover` passes the helper every allowed kind and the
    /// children enum's name, and puts `_children` in the struct literal.
    #[test]
    fn recover_collects_children_with_allowed_kinds_and_expected_type() {
        let out = partial(&unlabelled_grammar(), None);

        let program = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Program {");
        assert!(
            program.contains("_node.recover_children(&[\"stmt\"], false, \"ProgramChild\", _cx)")
        );
        assert!(program.contains("Some(Program { _span, _children })"));

        let stmt = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Stmt {");
        assert!(stmt.contains(
            "_node.recover_children(&[\"decl\", \"assign\"], false, \"StmtChild\", _cx)"
        ));
    }

    /// A kind with both a labelled field and unlabelled children keeps both:
    /// the field first, then `_children`, in the struct and in `recover`.
    #[test]
    fn labelled_fields_and_unlabelled_children_coexist_in_order() {
        let g = Grammar::from_rules([
            p(
                "block",
                Sequence(vec![
                    Field("name".into(), Box::new(nt("ident"))),
                    ZeroOrMore(Box::new(nt("stmt"))),
                ]),
            ),
            p("stmt", TerminalLiteral("'s'".into())),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);

        let decl = item_body(&out, "pub struct Block {");
        let name_pos = decl.find("pub name: Option<Ident>,").unwrap();
        let children_pos = decl.find("pub _children: Vec<BlockChild>,").unwrap();
        assert!(name_pos < children_pos);

        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Block {");
        let field_pos = recover.find("let name = ").unwrap();
        let children_pos = recover.find("let _children = ").unwrap();
        assert!(field_pos < children_pos);
        assert!(recover.contains("Some(Block { _span, name, _children })"));
    }

    /// A fully labelled kind still has a `_children` list — it can have a broken
    /// region under it — but it holds only regions: no kinds, no literals. A
    /// leaf has none, since its `_text` is its whole source.
    #[test]
    fn fully_labelled_kind_has_a_regions_only_children_list_and_a_leaf_has_none() {
        let out = partial(&every_shape_grammar(), None);

        assert!(item_body(&out, "pub struct Decl {").contains("pub _children: Vec<DeclChild>,"));
        let decl_child = item_body(&out, "pub enum DeclChild {");
        assert_eq!(decl_child.trim(), "Error(runtime::ErrorRegion),");
        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Decl {");
        assert!(recover.contains("_node.recover_children(&[], false, \"DeclChild\", _cx)"));

        assert!(!item_body(&out, "pub struct Ident {").contains("_children"));
        assert!(!out.contains("pub enum IdentChild"));
    }

    /// Strict mode is untouched: no `_children`, no per-kind child enum, no
    /// helper.
    #[test]
    fn strict_mode_never_captures_children() {
        let out = strict(&unlabelled_grammar());
        for needle in ["_children", "StmtChild", "recover_children", "child_tokens"] {
            assert!(
                !out.contains(needle),
                "strict output must not mention `{needle}`"
            );
        }
    }

    /// The runtime walk only yields what is grammar structure: children with
    /// no field label, minus extras (comments can appear anywhere) and
    /// `MISSING` nodes — but never an `ERROR` node, which tree-sitter itself
    /// flags as an extra — and `recover_children` reports a named kind the
    /// grammar doesn't allow instead of mis-typing it.
    #[test]
    fn children_runtime_helpers_skip_what_is_not_structure() {
        let out = partial(&unlabelled_grammar(), None);
        let walk_start = out.find("fn unlabelled_children(").unwrap();
        let walk = item_body(
            &out[walk_start..],
            ") -> Vec<super::visitor::SourceNode<'tree>> {",
        );
        assert!(walk.contains("(node.is_error() || !node.is_extra())"));
        assert!(walk.contains("cursor.field_name().is_none()"));
        assert!(walk.contains("!child.node.is_missing()"));

        let start = out.find("pub fn recover_children<").unwrap();
        let recover = item_body(&out[start..], ") -> Vec<T> {");
        assert!(recover.contains("self.unlabelled_children()"));
        assert!(recover.contains("child.node.is_error()"));
        assert!(recover.contains("runtime::DiagnosticKind::UnexpectedKind"));
    }

    /// The generated crate is pinned to edition 2021, where `if let … && …`
    /// chains don't exist; the repo itself is edition 2024, so a compile-check
    /// built inside it would not catch one slipping into generated text.
    #[test]
    fn generated_partial_code_uses_no_let_chains() {
        for grammar in [every_shape_grammar(), unlabelled_grammar()] {
            let out = partial(&grammar, None);
            assert!(!out.contains("&& let "), "let-chain in generated code");
        }
    }

    /// Two kinds whose unlabelled children are the same set each get their own
    /// children enum, named for the owner: nothing is shared, because each
    /// owner's enum is its own type to hold its own broken regions.
    #[test]
    fn kinds_with_the_same_child_set_each_get_their_own_enum() {
        let g = Grammar::from_rules([
            p("a", Choice(vec![nt("x"), nt("y")])),
            p("b", Choice(vec![nt("x"), nt("y")])),
            p("x", TerminalLiteral("'x'".into())),
            p("y", TerminalLiteral("'y'".into())),
        ]);
        let out = partial(&g, None);
        assert!(!out.contains("pub enum XY {"));
        assert!(item_body(&out, "pub struct A {").contains("pub _children: Vec<AChild>,"));
        assert!(item_body(&out, "pub struct B {").contains("pub _children: Vec<BChild>,"));
        for owner in ["pub enum AChild {", "pub enum BChild {"] {
            let body = item_body(&out, owner);
            assert!(body.contains("X(X),") && body.contains("Y(Y),"), "{owner}");
        }
    }

    /// A `passthrough` rename of the owning kind is followed: the struct and
    /// its child enum are both named for the display name.
    #[test]
    fn child_enum_follows_a_passthrough_rename() {
        let g = Grammar::from_rules([
            p("stmt", Choice(vec![nt("decl"), nt("assign")])),
            p("decl", TerminalLiteral("'d'".into())),
            p("assign", TerminalLiteral("'a'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![PassthroughEntry {
                kind: "stmt".to_string(),
                target: "Statement".to_string(),
            }],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));
        assert!(out.contains("pub enum StatementChild {"));
        assert!(
            item_body(&out, "pub struct Statement {")
                .contains("pub _children: Vec<StatementChild>,")
        );
        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Statement {");
        assert!(recover.contains("\"StatementChild\""));
    }

    /// A child set that is exactly a `merge` entry's `from` no longer reuses
    /// that entry's enum: a children enum needs an `Error` variant the merge
    /// enum doesn't have. It gets its own, and the merge enum stays what it
    /// is, for the fields that use it.
    #[test]
    fn children_never_reuse_a_merge_entrys_enum() {
        let g = Grammar::from_rules([
            p(
                "program",
                ZeroOrMore(Box::new(Choice(vec![
                    nt("for_statement"),
                    nt("while_statement"),
                ]))),
            ),
            p("for_statement", TerminalLiteral("'for'".into())),
            p("while_statement", TerminalLiteral("'while'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));

        assert!(
            item_body(&out, "pub struct Program {").contains("pub _children: Vec<ProgramChild>,")
        );
        let program_child = item_body(&out, "pub enum ProgramChild {");
        assert!(program_child.contains("ForStatement(ForStatement),"));
        assert!(program_child.contains("WhileStatement(WhileStatement),"));
        assert!(program_child.contains("Error(runtime::ErrorRegion),"));
        assert!(
            out.contains("pub enum Loop {"),
            "the merge entry's own enum is still emitted"
        );
    }

    /// A children enum that holds a merged-away (private) kind needs
    /// `#[allow(private_interfaces)]` on the *enum* — the struct only names the
    /// (public) enum, so it needs none.
    #[test]
    fn children_enum_of_a_merged_away_kind_allows_private_interfaces() {
        let g = Grammar::from_rules([
            p("program", ZeroOrMore(Box::new(nt("for_statement")))),
            p("for_statement", TerminalLiteral("'for'".into())),
            p("while_statement", TerminalLiteral("'while'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));

        let before_enum = &out[..out.find("pub enum ProgramChild {").unwrap()];
        assert!(
            before_enum
                .trim_end()
                .ends_with("#[allow(private_interfaces)]\n#[derive(Debug)]"),
            "ProgramChild must allow private_interfaces:\n{before_enum}"
        );
        let before_struct = &out[..out.find("pub struct Program {").unwrap()];
        assert!(
            !before_struct
                .trim_end()
                .ends_with("#[allow(private_interfaces)]\n#[derive(Debug)]"),
            "Program itself only names the public enum"
        );
    }

    /// The generated child enum is a top-level type name like any other, so it
    /// takes part in the collision check: a kind literally named
    /// `program_child` clashes with `program`'s child enum in partial mode —
    /// and only there, since strict mode generates no such enum.
    #[test]
    fn child_enum_name_collision_is_rejected_in_partial_mode_only() {
        let g = Grammar::from_rules([
            p("program", Choice(vec![nt("a"), nt("b")])),
            p("a", TerminalLiteral("'a'".into())),
            p("b", TerminalLiteral("'b'".into())),
            p("program_child", TerminalLiteral("'c'".into())),
        ]);
        let rejected = RustAst::new(&g, "g", true, None, BuildMode::Partial)
            .err()
            .expect("partial mode must reject the colliding name");
        assert!(rejected.contains("ProgramChild"), "{rejected}");
        assert!(RustAst::new(&g, "g", true, None, BuildMode::Strict).is_ok());
    }

    /// A field whose target names no kind and no token has no shape to recover
    /// into. The strict `field_try_from` panics the same way: a field always
    /// resolves to at least one target kind, so this is a bug guard, not input
    /// handling.
    #[test]
    #[should_panic(expected = "a field always resolves to at least one target kind")]
    fn field_recover_panics_on_empty_target() {
        field_recover("kind", "field", &AstFieldSpec::default());
    }

    // ── literal children (`Token`) ──────────────────────────────────────

    /// A literal beside a named kind joins that kind's child enum as the
    /// shared `Token(runtime::Token)` variant, and the recover call says so.
    #[test]
    fn literal_beside_a_named_kind_becomes_a_token_variant() {
        let out = partial(&unlabelled_grammar(), None);

        let decl_child = item_body(&out, "pub enum DeclChild {");
        assert!(decl_child.contains("Ident(Ident),"));
        assert!(decl_child.contains("Token(runtime::Token),"));

        let assign_child = item_body(&out, "pub enum AssignChild {");
        assert!(assign_child.contains("Token(runtime::Token),"));

        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Decl {");
        assert!(recover.contains("_node.recover_children(&[\"ident\"], true, \"DeclChild\", _cx)"));
        // The enum's own `recover` takes an anonymous node as its `Token`.
        let enum_recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for DeclChild {");
        assert!(enum_recover.contains("if !_node.node.is_named() {"));
    }

    /// A kind whose only unlabelled children are literals (a label captures
    /// everything named) still gets a children enum: its tokens plus the
    /// `Error` variant, no kinds.
    #[test]
    fn token_only_kind_gets_a_children_enum_with_token_and_error() {
        let g = Grammar::from_rules([
            p(
                "call",
                Sequence(vec![
                    Field("name".into(), Box::new(nt("ident"))),
                    TerminalLiteral("'('".into()),
                    TerminalLiteral("')'".into()),
                ]),
            ),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);

        assert!(item_body(&out, "pub struct Call {").contains("pub _children: Vec<CallChild>,"));
        let call_child = item_body(&out, "pub enum CallChild {");
        assert!(call_child.contains("Token(runtime::Token),"));
        assert!(call_child.contains("Error(runtime::ErrorRegion),"));
        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Call {");
        assert!(recover.contains("_node.recover_children(&[], true, \"CallChild\", _cx)"));
        assert!(recover.contains("Some(Call { _span, name, _children })"));
        assert!(
            !out.contains("child_tokens"),
            "the token-only helper is gone"
        );
    }

    /// A leaf's `_text` is its whole source, literals included, so a rule
    /// that is only literals has no `_children`.
    #[test]
    fn literal_only_leaf_has_no_children() {
        let g = Grammar::from_rules([p(
            "op",
            Choice(vec![
                TerminalLiteral("'+'".into()),
                TerminalLiteral("'-'".into()),
            ]),
        )]);
        let out = partial(&g, None);
        let op = item_body(&out, "pub struct Op {");
        assert!(!op.contains("_children"));
        assert!(op.contains("pub _text: String,"));
    }

    /// An inline pattern never appears as a node, so a kind with only a
    /// pattern beside a named kind gets no `Token` variant for it.
    #[test]
    fn inline_pattern_adds_no_token_variant() {
        let g = Grammar::from_rules([
            p(
                "pair",
                Sequence(vec![nt("key"), TerminalPattern("/[0-9]+/".into())]),
            ),
            p("key", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);
        assert!(item_body(&out, "pub struct Pair {").contains("pub _children: Vec<PairChild>,"));
        let pair_child = item_body(&out, "pub enum PairChild {");
        assert!(pair_child.contains("Key(Key),"));
        assert!(
            !pair_child.contains("Token("),
            "an inline pattern is not a node"
        );
    }

    /// A merge entry's enum has only the entry's kinds, so a child set that
    /// is those kinds *plus* a literal can't reuse it: it gets its own enum
    /// with a `Token` variant instead.
    #[test]
    fn children_with_a_literal_do_not_reuse_a_merge_entrys_enum() {
        let g = Grammar::from_rules([
            p(
                "program",
                Sequence(vec![
                    TerminalLiteral("'{'".into()),
                    ZeroOrMore(Box::new(Choice(vec![
                        nt("for_statement"),
                        nt("while_statement"),
                    ]))),
                ]),
            ),
            p("for_statement", TerminalLiteral("'for'".into())),
            p("while_statement", TerminalLiteral("'while'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));

        assert!(
            item_body(&out, "pub struct Program {").contains("pub _children: Vec<ProgramChild>,")
        );
        let program_child = item_body(&out, "pub enum ProgramChild {");
        assert!(program_child.contains("ForStatement(ForStatement),"));
        assert!(program_child.contains("Token(runtime::Token),"));
    }

    /// Two string-like rules whose children are the same "character or quote"
    /// set (the shape that once failed with `kind 'x' and shape [x, $token]
    /// both generate 'X'`) each get their own children enum, named for the
    /// owner, so the character kind's own struct name is never in play.
    #[test]
    fn string_rules_each_get_their_own_children_enum() {
        let g = Grammar::from_rules([
            p(
                "single_string",
                Sequence(vec![
                    TerminalLiteral("'q1'".into()),
                    ZeroOrMore(Box::new(nt("char"))),
                    TerminalLiteral("'q1'".into()),
                ]),
            ),
            p(
                "double_string",
                Sequence(vec![
                    TerminalLiteral("'q2'".into()),
                    ZeroOrMore(Box::new(nt("char"))),
                    TerminalLiteral("'q2'".into()),
                ]),
            ),
            p("char", TerminalPattern("/[a-z]/".into())),
        ]);
        let out = partial(&g, None);

        for (owner, children) in [
            ("pub struct SingleString {", "SingleStringChild"),
            ("pub struct DoubleString {", "DoubleStringChild"),
        ] {
            assert!(item_body(&out, owner).contains(&format!("pub _children: Vec<{children}>,")));
            let body = item_body(&out, &format!("pub enum {children} {{"));
            assert!(body.contains("Char(Char),"));
            assert!(body.contains("Token(runtime::Token),"));
            assert!(body.contains("Error(runtime::ErrorRegion),"));
        }
        assert!(!out.contains("CharOrToken"));
        assert!(out.contains("pub struct Char {"));
    }

    // ── Span, Token and Reflect ─────────────────────────────────────────

    /// A struct carries `_span` in both modes (it replaced the start-only
    /// `_pragma`), and `Span` is the module's own in strict mode, next to the
    /// `Pragma` it reuses for its start and end; in partial mode it is
    /// `ts_bnf_runtime`'s, re-exported. Either way a tree-sitter node converts
    /// into it from the generated crate.
    #[test]
    fn span_replaces_pragma_in_both_modes() {
        for (is_partial, out) in [
            (true, partial(&every_shape_grammar(), None)),
            (false, strict(&every_shape_grammar())),
        ] {
            assert!(item_body(&out, "pub struct Decl {").contains("pub _span: runtime::Span,"));
            assert!(!out.contains("_pragma"), "`_pragma` is gone");
            let runtime = item_body(&out, "pub mod runtime {");
            for target in ["Pragma", "Span"] {
                assert!(
                    runtime.contains(&format!(
                        "impl From<super::super::visitor::SourceNode<'_>> for {target} {{"
                    )),
                    "a node converts into `{target}`, in either mode"
                );
            }
            if is_partial {
                assert!(
                    !runtime.contains("pub struct Span {"),
                    "declared by ts_bnf_runtime"
                );
                assert!(
                    !runtime.contains("pub struct Pragma {"),
                    "declared by ts_bnf_runtime"
                );
            } else {
                let span = item_body(&runtime, "pub struct Span {");
                assert!(span.contains("pub byte_range: std::ops::Range<usize>,"));
                assert!(span.contains("pub start: Pragma,"));
                assert!(span.contains("pub end: Pragma,"));
            }
        }
    }

    /// A diagnostic is built from a node by a free function, because
    /// `Diagnostic` is `ts_bnf_runtime`'s and a foreign type takes no inherent
    /// method: what it locates itself by is that node's `Span`. No `Diagnostic::at`
    /// is left anywhere in the generated file.
    #[test]
    fn diagnostics_are_built_by_a_free_function_from_a_node() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        assert!(
            !runtime.contains("pub struct Diagnostic {"),
            "declared by ts_bnf_runtime"
        );
        assert!(
            !runtime.contains("impl Diagnostic {"),
            "a foreign type takes no inherent impl"
        );
        assert!(!out.contains("Diagnostic::at("));

        let builder = item_body(&runtime, "pub(super) fn diagnostic_at(");
        assert!(builder.contains("kind: DiagnosticKind,"));
        assert!(builder.contains("-> Diagnostic {"));
        assert!(builder.contains("span: Span::from(node)"));
    }

    /// A token is built from a node by the generated crate — kind, text and
    /// span all from the node — and is `ts_bnf_runtime`'s type, re-exported,
    /// along with its `Reflect` impl (an unnamed leaf), which that crate tests.
    #[test]
    fn token_is_built_from_a_node_with_its_span() {
        let out = partial(&unlabelled_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        assert!(
            !runtime.contains("pub struct Token {"),
            "declared by ts_bnf_runtime"
        );
        assert!(
            !runtime.contains("impl Reflect for Token {"),
            "implemented by ts_bnf_runtime"
        );

        let from = item_body(
            &runtime,
            "impl From<super::super::visitor::SourceNode<'_>> for Token {",
        );
        assert!(from.contains("kind: node.node.kind().to_string(),"));
        assert!(from.contains("text: node.text().to_string(),"));
        assert!(from.contains("span: Span::from(node),"));
    }

    /// A struct reflects its grammar kind, span and (for a non-leaf) no text,
    /// and is named.
    #[test]
    fn struct_reflect_reports_kind_span_text_and_is_named() {
        let out = partial(&every_shape_grammar(), None);

        let decl = item_body(&out, "impl runtime::Reflect for Decl {");
        assert!(decl.contains("\"decl\""));
        assert!(decl.contains("&self._span"));
        assert!(decl.contains("None"));
        assert!(decl.contains("true"));

        let ident = item_body(&out, "impl runtime::Reflect for Ident {");
        assert!(
            ident.contains("Some(self._text.as_str())"),
            "a leaf has text"
        );
        assert!(ident.contains("Vec::new()"), "a leaf has nothing to gather");
    }

    /// `kind()` is the grammar's kind, not the Rust type's name: a
    /// `passthrough`-renamed struct still says what tree-sitter says.
    #[test]
    fn reflect_kind_is_the_grammar_kind_not_the_rust_name() {
        let g = Grammar::from_rules([p("comment", TerminalLiteral("'c'".into()))]);
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![PassthroughEntry {
                kind: "comment".to_string(),
                target: "DocComment".to_string(),
            }],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));
        let reflect = item_body(&out, "impl runtime::Reflect for DocComment {");
        assert!(reflect.contains("\"comment\""));
        assert!(!reflect.contains("\"DocComment\""));
    }

    /// `children()` gathers every field shape — optional single, enum-typed
    /// (unwrapped with `inner()`), repeated, token — and the unlabelled
    /// `_children`, then sorts by start byte so fields and unlabelled children
    /// interleave as they do in the source.
    #[test]
    fn struct_reflect_children_gathers_every_field_and_sorts_by_position() {
        let out = partial(&every_shape_grammar(), None);
        let decl = item_body(&out, "impl runtime::Reflect for Decl {");

        assert!(decl.contains("if let Some(v) = &self.target {\n"));
        // `value` is a multi-shape enum: pushed as the node it holds.
        let value_pos = decl.find("&self.value {").unwrap();
        assert!(decl[value_pos..].contains("out.push((Some(\"value\"), v.inner()));"));
        assert!(decl.contains("for v in &self.ops {\n"));
        assert!(decl.contains("if let Some(v) = &self.tag {\n"));
        assert!(
            decl.contains("out.sort_by_key(|c| runtime::Reflect::span(c.1).byte_range.start);")
        );

        // `items` holds plain structs, pushed as themselves; `_children` is a
        // children enum, so it is unwrapped (the only `inner()` here).
        let program = item_body(&out, "impl runtime::Reflect for Program {");
        let loop_body = |header: &str| -> String {
            let from = program.find(header).expect(header);
            let rest = &program[from..];
            rest[..rest.find('}').unwrap()].to_string()
        };
        assert!(loop_body("for v in &self.items {").contains("out.push((Some(\"items\"), v));"));
        assert!(loop_body("for v in &self._children {").contains("out.push((None, v.inner()));"));
        assert_eq!(program.matches("inner()").count(), 1);
    }

    /// Unlabelled children are gathered too, with the same enum unwrapping.
    #[test]
    fn struct_reflect_children_includes_unlabelled_children() {
        let out = partial(&unlabelled_grammar(), None);

        let program = item_body(&out, "impl runtime::Reflect for Program {");
        assert!(program.contains("for v in &self._children {\n"));
        assert!(
            program.contains("out.push((None, v.inner()));"),
            "`ProgramChild` is an enum too, even with one kind, and unlabelled"
        );

        let stmt = item_body(&out, "impl runtime::Reflect for Stmt {");
        assert!(
            stmt.contains("out.push((None, v.inner()));"),
            "`StmtChild` is an enum"
        );
    }

    /// A kind with nothing to gather says so directly, without building and
    /// sorting an empty vector.
    #[test]
    fn struct_reflect_children_is_empty_for_a_kind_with_nothing_to_gather() {
        let g = Grammar::from_rules([p("leaf", TerminalPattern("/[a-z]/".into()))]);
        let out = partial(&g, None);
        let reflect = item_body(&out, "impl runtime::Reflect for Leaf {");
        assert!(!reflect.contains("sort_by_key"));
        assert!(reflect.contains("Vec::new()"));
    }

    /// An enum is `Reflect` by delegating every method to the node it holds,
    /// and offers `inner()` so a parent can yield that node, not the enum.
    #[test]
    fn enum_reflect_delegates_and_exposes_inner() {
        let out = partial(&unlabelled_grammar(), None);

        let reflect = item_body(&out, "impl runtime::Reflect for DeclChild {");
        for call in [
            "runtime::Reflect::kind(self.inner())",
            "runtime::Reflect::span(self.inner())",
            "runtime::Reflect::text(self.inner())",
            "runtime::Reflect::children(self.inner())",
            "runtime::Reflect::is_named(self.inner())",
        ] {
            assert!(reflect.contains(call), "missing `{call}`");
        }

        let inherent = item_body(&out, "impl DeclChild {");
        let inner = item_body(&inherent, "pub fn inner(&self) -> &dyn runtime::Reflect {");
        let arms = item_body(inner, "match self {");
        assert!(arms.contains("DeclChild::Ident(v) => v,"));
        assert!(
            arms.contains("DeclChild::Token(v) => v,"),
            "the literal variant too"
        );
    }

    /// A `merge` entry's enum is transparent the same way, and the merged-away
    /// struct it wraps is `Reflect` under its grammar kind.
    #[test]
    fn merge_enum_and_merged_away_struct_are_reflect() {
        let g = Grammar::from_rules([
            p(
                "program",
                ZeroOrMore(Box::new(Choice(vec![
                    nt("for_statement"),
                    nt("while_statement"),
                ]))),
            ),
            p("for_statement", TerminalLiteral("'for'".into())),
            p("while_statement", TerminalLiteral("'while'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Loop".to_string(),
                from: vec!["for_statement".to_string(), "while_statement".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let out = partial(&g, Some(&config));

        let inherent = item_body(&out, "impl Loop {");
        assert!(inherent.contains("Loop::ForStatement(v) => v,"));
        assert!(inherent.contains("Loop::WhileStatement(v) => v,"));
        assert!(
            item_body(&out, "impl runtime::Reflect for ForStatement {")
                .contains("\"for_statement\"")
        );
        // The parent unwraps `Loop` to the node inside.
        let program = item_body(&out, "impl runtime::Reflect for Program {");
        assert!(program.contains("out.push((None, v.inner()));"));
    }

    /// Strict mode gets `Span` and `Token` — a token value should carry its
    /// position whichever API you use — but none of the partial-only
    /// machinery: no `Reflect`, no `inner()`.
    #[test]
    fn strict_mode_has_span_and_token_but_no_reflect() {
        let out = strict(&unlabelled_grammar());
        for needle in ["Reflect", "inner()", "ErrorRegion"] {
            assert!(
                !out.contains(needle),
                "strict output must not mention `{needle}`"
            );
        }
        assert!(out.contains("pub struct Span {"));
        assert!(out.contains("pub struct Token {"));
    }

    /// In strict mode a bare token value — a field, a list of them — is a
    /// `runtime::Token` built from its node, not a `String` that has lost
    /// where the literal was.
    #[test]
    fn strict_mode_token_values_carry_a_span() {
        let out = strict(&every_shape_grammar());

        let decl = item_body(&out, "pub struct Decl {");
        assert!(decl.contains("pub ops: Vec<runtime::Token>,"));
        assert!(decl.contains("pub tag: runtime::Token,"));

        let try_from = item_body(
            &out,
            "impl<'tree> TryFrom<super::visitor::SourceNode<'tree>> for Decl {",
        );
        assert!(try_from.contains(".map(runtime::Token::from)"));
        assert!(try_from.contains("let tag = runtime::Token::from("));
        assert!(!out.contains("c.text().to_string()"));
    }

    /// The `Token` variant of a "kind or literal" enum holds a `Token` in
    /// strict mode too, built from the anonymous node.
    #[test]
    fn strict_mode_enum_token_variant_carries_a_span() {
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
        let out = strict(&g);
        assert!(item_body(&out, "pub enum Value {").contains("Token(runtime::Token),"));
        assert!(out.contains("return Ok(Value::Token(runtime::Token::from(node)));"));
    }

    // ── ERROR regions kept in the tree ──────────────────────────────────

    /// An error region is built from an `ERROR` node by the generated crate —
    /// its span and its source text. The type, and its `Reflect` impl (kind
    /// `"ERROR"`, named as tree-sitter reports it, text but no children), are
    /// `ts_bnf_runtime`'s.
    #[test]
    fn error_region_is_built_from_an_error_node() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        assert!(
            !runtime.contains("pub struct ErrorRegion {"),
            "declared by ts_bnf_runtime"
        );
        assert!(
            !runtime.contains("impl Reflect for ErrorRegion {"),
            "implemented by ts_bnf_runtime"
        );

        let from = item_body(
            &runtime,
            "impl From<super::super::visitor::SourceNode<'_>> for ErrorRegion {",
        );
        assert!(from.contains("span: Span::from(node),"));
        assert!(from.contains("text: node.text().to_string()"));
    }

    /// A children enum builds its `Error` variant from an `ERROR` node before
    /// it considers anything else: an `ERROR` node is named, but its kind is
    /// `"ERROR"`, which no kind arm would take (and it can be an extra too).
    #[test]
    fn children_enum_recover_takes_an_error_node_first() {
        let out = partial(&unlabelled_grammar(), None);
        let recover = item_body(&out, "impl<'tree> runtime::Recover<'tree> for DeclChild {");

        let error_branch = recover
            .find("if _node.node.is_error() {")
            .expect("an error branch");
        assert!(
            recover.contains("return Some(DeclChild::Error(runtime::ErrorRegion::from(_node)));")
        );
        let token_branch = recover.find("if !_node.node.is_named() {").unwrap();
        let match_pos = recover.find("match _node.node.kind() {").unwrap();
        assert!(error_branch < token_branch && token_branch < match_pos);
    }

    /// The children enum is transparent like any other, error variant
    /// included: `inner()` hands back the region, and `Reflect` delegates to
    /// it, so a generic walk meets a region as a child with kind `"ERROR"`.
    #[test]
    fn children_enum_reflect_and_inner_cover_the_error_variant() {
        let out = partial(&unlabelled_grammar(), None);
        let inherent = item_body(&out, "impl ProgramChild {");
        let inner = item_body(&inherent, "pub fn inner(&self) -> &dyn runtime::Reflect {");
        let arms = item_body(inner, "match self {");
        assert!(arms.contains("ProgramChild::Stmt(v) => v,"));
        assert!(arms.contains("ProgramChild::Error(v) => v,"));
        assert!(out.contains("impl runtime::Reflect for ProgramChild {"));
    }

    /// A kind literally named `error` or `token` would give a children enum a
    /// second variant of the same name. Only that kind's variant is renamed —
    /// the payload type keeps its name — and the fixed `Token`/`Error`
    /// variants keep theirs.
    #[test]
    fn kinds_named_error_or_token_get_renamed_variants() {
        let g = Grammar::from_rules([
            p(
                "holder",
                Sequence(vec![
                    nt("error"),
                    nt("token"),
                    TerminalLiteral("'x'".into()),
                ]),
            ),
            p("error", TerminalPattern("/e/".into())),
            p("token", TerminalPattern("/t/".into())),
        ]);
        let out = partial(&g, None);

        let holder_child = item_body(&out, "pub enum HolderChild {");
        assert!(holder_child.contains("ErrorNode(Error),"));
        assert!(holder_child.contains("TokenNode(Token),"));
        assert!(holder_child.contains("Token(runtime::Token),"));
        assert!(holder_child.contains("Error(runtime::ErrorRegion),"));

        let recover = item_body(
            &out,
            "impl<'tree> runtime::Recover<'tree> for HolderChild {",
        );
        assert!(recover.contains(
            "<Error as runtime::Recover<'tree>>::recover(_node, _cx).map(HolderChild::ErrorNode)"
        ));
        assert!(recover.contains(
            "<Token as runtime::Recover<'tree>>::recover(_node, _cx).map(HolderChild::TokenNode)"
        ));

        let inherent = item_body(&out, "impl HolderChild {");
        assert!(inherent.contains("HolderChild::ErrorNode(v) => v,"));
        assert!(inherent.contains("HolderChild::TokenNode(v) => v,"));
    }

    /// The rename only happens where the enum really has the colliding fixed
    /// variant, so no grammar that generates today changes: in strict mode a
    /// kind named `error` is still plain `Error(Error)` (no `Error` variant
    /// exists there), and a kind named `token` only gets renamed in an enum
    /// that also has the `Token` literal variant — where it used to fail to
    /// compile.
    #[test]
    fn variant_rename_applies_only_when_the_fixed_variant_exists() {
        let g = Grammar::from_rules([
            p(
                "decl",
                Field(
                    "value".into(),
                    Box::new(Choice(vec![nt("error"), nt("other")])),
                ),
            ),
            p("error", TerminalPattern("/e/".into())),
            p("other", TerminalPattern("/o/".into())),
        ]);
        let out = strict(&g);
        assert!(item_body(&out, "pub enum Value {").contains("Error(Error),"));

        let g = Grammar::from_rules([
            p(
                "decl",
                Field(
                    "value".into(),
                    Box::new(Choice(vec![nt("token"), TerminalLiteral("'nil'".into())])),
                ),
            ),
            p("token", TerminalPattern("/t/".into())),
        ]);
        let out = strict(&g);
        let value = item_body(&out, "pub enum Value {");
        assert!(value.contains("TokenNode(Token),"));
        assert!(value.contains("Token(runtime::Token),"));
    }

    // ── parse_ast: the tree-sitter-free entry point ─────────────────────

    /// Partial mode: parse + wrap + `recover`, returning the `Recovered` of
    /// the root rule's struct. A syntax error is not an `Err`, which is the
    /// point of the mode.
    #[test]
    fn partial_parse_ast_returns_the_recovered_root_and_calls_the_crates_parse() {
        let out = partial(&every_shape_grammar(), None);
        assert!(
            out.contains(") -> Result<runtime::Recovered<Program>, Box<dyn std::error::Error>> {")
        );
        let start = out.find("pub fn parse_ast(").unwrap();
        let body = item_body(
            &out[start..],
            ") -> Result<runtime::Recovered<Program>, Box<dyn std::error::Error>> {",
        );
        assert!(body.contains("super::parse(source)?"));
        assert!(body.contains("runtime::recover("));
        assert!(body.contains("super::visitor::SourceNode"));
        assert!(
            !body.contains("tree_sitter"),
            "the generated code names no parser type itself"
        );
    }

    /// Strict mode: parse + build, returning the root's struct, with a failed
    /// build reported as the error.
    #[test]
    fn strict_parse_ast_returns_the_built_root_and_reports_a_failed_build() {
        let out = strict(&every_shape_grammar());
        let start = out.find("pub fn parse_ast(").unwrap();
        let body = item_body(
            &out[start..],
            "pub fn parse_ast(source: &str) -> Result<Program, Box<dyn std::error::Error>> {",
        );
        assert!(body.contains("super::parse(source)?"));
        assert!(body.contains("Program::try_from("));
        assert!(body.contains("could not build the typed tree"));
        assert!(!out.contains("Recovered"), "strict has no recovered result");
    }

    /// The root's struct is named through a `passthrough` rename, as it is
    /// everywhere else.
    #[test]
    fn parse_ast_names_the_root_through_a_passthrough_rename() {
        let config = MergeConfig {
            merge: vec![],
            passthrough: vec![PassthroughEntry {
                kind: "program".to_string(),
                target: "Module".to_string(),
            }],
            ignore: vec![],
        };
        let out = partial(&every_shape_grammar(), Some(&config));
        assert!(out.contains("-> Result<runtime::Recovered<Module>,"));
        assert!(!out.contains("Recovered<Program>"));
    }

    /// A root claimed by a `merge` entry has a private struct, which a public
    /// function cannot return: no entry point is emitted rather than one that
    /// would not compile. (The scaffold command rejects that configuration
    /// before it gets here.)
    #[test]
    fn parse_ast_is_absent_when_the_root_is_merged_away() {
        let g = Grammar::from_rules([
            p("program", TerminalLiteral("'p'".into())),
            p("other", TerminalLiteral("'o'".into())),
        ]);
        let config = MergeConfig {
            merge: vec![MergeEntry {
                target: "Either".to_string(),
                from: vec!["program".to_string(), "other".to_string()],
            }],
            passthrough: vec![],
            ignore: vec![],
        };
        let partial_out = partial(&g, Some(&config));
        assert!(
            !partial_out.contains("parse_ast"),
            "no entry point for a private root"
        );
        assert!(!strict_with(&g, &config).contains("parse_ast"));
    }

    // ── field names per child, the walk helper, the depth guard ─────────

    /// Each labelled field is pushed with its grammar label — verbatim, not
    /// the Rust identifier a keyword needs (`r#type`) — and everything else
    /// (the `_children` list) with `None`.
    #[test]
    fn children_pairs_carry_the_grammar_label_verbatim() {
        let g = Grammar::from_rules([
            p(
                "stmt",
                Sequence(vec![
                    Field("type".into(), Box::new(nt("ident"))),
                    Field(
                        "ops".into(),
                        Box::new(ZeroOrMore(Box::new(TerminalLiteral("'+'".into())))),
                    ),
                    Field("tag".into(), Box::new(TerminalLiteral("';'".into()))),
                ]),
            ),
            p("ident", TerminalPattern("/[a-z]+/".into())),
        ]);
        let out = partial(&g, None);
        let reflect = item_body(&out, "impl runtime::Reflect for Stmt {");

        let pushed = |header: &str| -> String {
            let from = reflect.find(header).expect(header);
            let rest = &reflect[from..];
            rest[..rest.find('}').unwrap()].to_string()
        };
        // The access is the escaped identifier; the label is the grammar's own.
        assert!(
            pushed("if let Some(v) = &self.r#type {").contains("out.push((Some(\"type\"), v));")
        );
        assert!(pushed("for v in &self.ops {").contains("out.push((Some(\"ops\"), v));"));
        assert!(pushed("if let Some(v) = &self.tag {").contains("out.push((Some(\"tag\"), v));"));
        assert!(pushed("for v in &self._children {").contains("out.push((None, v.inner()));"));
    }

    /// The depth limit is counted in exactly one place: a non-leaf struct's
    /// `recover` enters before it reads anything and leaves just before it
    /// returns. A leaf has nothing to recurse into and isn't counted.
    #[test]
    fn non_leaf_recover_is_bracketed_by_the_depth_guard_and_a_leaf_is_not() {
        let out = partial(&every_shape_grammar(), None);

        let decl = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Decl {");
        let enter = decl.find("if !_cx.enter(_node) {").expect("enters first");
        let span = decl.find("let _span = ").unwrap();
        let leave = decl.find("_cx.leave();").expect("and leaves");
        let built = decl.find("Some(Decl {").unwrap();
        assert!(enter < span && span < leave && leave < built);
        assert!(decl[enter..span].contains("return None;"));

        let ident = item_body(&out, "impl<'tree> runtime::Recover<'tree> for Ident {");
        assert!(!ident.contains("enter") && !ident.contains("leave"));
    }

    /// The generated runtime carries the context and the explicit-limit entry
    /// point; the default limit and the diagnostic for a subtree that was too
    /// deep are `ts_bnf_runtime`'s, re-exported.
    #[test]
    fn runtime_has_the_depth_limit_machinery() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        for needle in [
            "pub struct Context {",
            "pub fn enter(&mut self,",
            "pub fn leave(&mut self) {",
            "pub fn recover_with_limit<'tree, T: Recover<'tree>>(",
            "recover_with_limit(root, DEFAULT_MAX_DEPTH)",
            "DiagnosticKind::TooDeep { limit: self.max_depth }",
        ] {
            assert!(
                runtime.contains(needle),
                "runtime module missing `{needle}`"
            );
        }
        assert!(
            !runtime.contains("pub const DEFAULT_MAX_DEPTH"),
            "ts_bnf_runtime's"
        );
        assert!(
            !runtime.contains("TooDeep { limit: usize }"),
            "declared by ts_bnf_runtime"
        );
        // `Recover` takes the context, not a bare vector of diagnostics.
        let recover = item_body(&runtime, "pub trait Recover<'tree>: Sized {");
        assert!(recover.contains("cx: &mut Context,"));
        assert!(!out.contains("&mut Vec<runtime::Diagnostic>"));
    }

    /// The `ERROR`/`MISSING` walk is bounded by the input, not the stack: it
    /// drives a cursor in a loop and never calls itself.
    #[test]
    fn syntax_problem_walk_is_iterative() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        let start = runtime.find("fn collect_syntax_problems(").unwrap();
        let body = item_body(&runtime[start..], ") {");
        assert!(body.contains("cursor.goto_first_child()"));
        assert!(body.contains("cursor.goto_parent()"));
        assert!(
            !body.contains("collect_syntax_problems("),
            "it must not recurse"
        );
    }

    // ── the plain data lives in `ts_bnf_runtime` ────────────────────────

    /// The names the partial `runtime` module re-exports from `ts_bnf_runtime`:
    /// everything that moved, and nothing else.
    const MOVED: [&str; 12] = [
        "DEFAULT_MAX_DEPTH",
        "Diagnostic",
        "DiagnosticKind",
        "ErrorRegion",
        "Pragma",
        "Recovered",
        "Reflect",
        "Span",
        "Token",
        "Visit",
        "Walk",
        "walk",
    ];

    /// The generated `runtime` module re-exports exactly the moved items from
    /// `ts_bnf_runtime`, so every old path (`ast::runtime::Span`, `::Reflect`,
    /// `::walk`, `::Recovered`, …) keeps resolving — and that one `pub use` is
    /// the only place the crate is named.
    #[test]
    fn partial_runtime_reexports_the_shared_crate_items() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");

        let from = runtime
            .find("pub use ts_bnf_runtime::{")
            .expect("a `pub use ts_bnf_runtime::{..}`");
        let list = &runtime[from + "pub use ts_bnf_runtime::{".len()..];
        let list = &list[..list.find("};").expect("the use list ends")];
        let mut names: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        names.sort_unstable();
        let mut expected = MOVED.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected);

        assert_eq!(
            out.matches("ts_bnf_runtime::").count(),
            1,
            "the crate is named once, in the `pub use`"
        );
    }

    /// None of the moved items is declared in the generated file: each would
    /// be a second, incompatible copy, which is what the shared crate removes.
    #[test]
    fn partial_runtime_declares_none_of_the_moved_items() {
        for out in [
            partial(&every_shape_grammar(), None),
            partial(&unlabelled_grammar(), None),
        ] {
            for declaration in [
                "pub struct Pragma",
                "pub struct Span",
                "pub struct Token",
                "pub struct ErrorRegion",
                "pub struct Diagnostic",
                "pub enum DiagnosticKind",
                "pub struct Recovered",
                "pub struct Visit",
                "pub struct Walk",
                "pub trait Reflect",
                "pub fn walk(",
                "pub const DEFAULT_MAX_DEPTH",
                "impl Reflect for Token",
                "impl Reflect for ErrorRegion",
                "impl<T> Recovered<T>",
            ] {
                assert!(
                    !out.contains(declaration),
                    "`{declaration}` belongs to ts_bnf_runtime, not the generated file"
                );
            }
        }
    }

    /// What does stay generated is the conversion from a tree-sitter node into
    /// each shared type: legal on a foreign type because `SourceNode`, the
    /// trait's parameter, is the generated crate's own.
    #[test]
    fn partial_runtime_keeps_the_node_conversions() {
        let out = partial(&every_shape_grammar(), None);
        let runtime = item_body(&out, "pub mod runtime {");
        for target in ["Pragma", "Span", "Token", "ErrorRegion"] {
            assert!(
                runtime.contains(&format!(
                    "impl From<super::super::visitor::SourceNode<'_>> for {target} {{"
                )),
                "missing the conversion into `{target}`"
            );
        }
    }

    /// Strict mode does not depend on the shared crate at all: it still
    /// declares its own `Pragma`, `Span`, `Token` and `BuildError`, and never
    /// names `ts_bnf_runtime`.
    #[test]
    fn strict_runtime_stays_self_contained() {
        let out = strict(&every_shape_grammar());
        assert!(!out.contains("ts_bnf_runtime"));
        let runtime = item_body(&out, "pub mod runtime {");
        for declaration in [
            "pub struct Pragma {",
            "pub struct Span {",
            "pub struct Token {",
            "pub enum BuildError {",
        ] {
            assert!(runtime.contains(declaration), "strict lost `{declaration}`");
        }
    }
}
