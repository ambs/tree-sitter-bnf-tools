use tree_sitter::Node;

/// ANTLR-style visitor for the `sample` tree-sitter grammar.
///
/// ## How to implement
///
/// Implement [`Visitor::combine`], folding a `Vec` of child outputs into
/// one output, and override whichever `visit_<kind>` methods you care
/// about. Every method you don't override falls back to a sensible
/// default: non-leaf kinds recurse via [`Visitor::children_visitor`], leaf
/// kinds return [`Visitor::default_result`].
///
/// ## ANTLR correspondence
///
/// | ANTLR (`AbstractParseTreeVisitor`) | Here                                                            |
/// |------------------------------------|-----------------------------------------------------------------|
/// | `visit(tree)`                      | [`Visitor::visit`]                                              |
/// | `visitChildren(node)`              | [`Visitor::children_visitor`]                                   |
/// | `aggregateResult(agg, next)`       | [`Visitor::combine`] (`Vec`-based, not pairwise)                |
/// | `defaultResult()`                  | [`Visitor::default_result`] (`= combine(vec![])`)               |
/// | `visitErrorNode(node)`             | [`Visitor::error_visitor`]                                      |
/// | (no ANTLR analogue)                | [`Visitor::missing_visitor`], for tree-sitter's `MISSING` nodes |
///
/// ## `combine` patterns
///
/// | Output type              | Typical `combine` body                        |
/// |--------------------------|-----------------------------------------------|
/// | `()` (side-effect)       | `Ok(())`                                      |
/// | `Vec<T>` (collect)       | `Ok(results.into_iter().flatten().collect())` |
/// | `Option<T>` (find-first) | `Ok(results.into_iter().flatten().next())`    |
///
/// ## Minimal example
///
/// ```ignore
/// struct Counter(usize);
///
/// impl<'t> Visitor<'t> for Counter {
///     type Output = ();
///     type Error = std::convert::Infallible;
///
///     fn combine(&mut self, _: Vec<()>) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn visit_decl(&mut self, node: Node<'t>) -> Result<(), Self::Error> {
///         self.0 += 1;
///         self.children_visitor(node)
///     }
/// }
/// ```
pub trait Visitor<'tree> {
    /// The value produced by each `visit_*` call.
    type Output;

    /// The error type. Use `std::convert::Infallible` for a
    /// visitor that can't fail.
    type Error;

    /// Merges child outputs into a single output.
    ///
    /// This is the only method you *must* implement. Called by
    /// [`Visitor::children_visitor`] and [`Visitor::field_visitor`]
    /// with one [`Visitor::Output`] per visited child, and by
    /// [`Visitor::default_result`] with an empty `Vec`.
    fn combine(&mut self, results: Vec<Self::Output>) -> Result<Self::Output, Self::Error>;

    /// Walks all named children of `node` and combines their outputs.
    ///
    /// This is the default body of every non-leaf `visit_*`
    /// method. Anonymous children (punctuation, keywords) are
    /// silently skipped.
    fn children_visitor(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        let mut cursor = node.walk();
        let mut results = Vec::new();
        for child in node.named_children(&mut cursor) {
            results.push(self.visit(child)?);
        }
        self.combine(results)
    }

    /// Visits every child in field `field_name` and combines
    /// their outputs.
    ///
    /// Handles `multiple: true` fields correctly, visiting
    /// every match rather than just the first. Returns
    /// [`Visitor::default_result`] when the field is absent.
    fn field_visitor(
        &mut self,
        node: Node<'tree>,
        field_name: &str,
    ) -> Result<Self::Output, Self::Error> {
        let mut cursor = node.walk();
        let mut results = Vec::new();
        for child in node.children_by_field_name(field_name, &mut cursor) {
            results.push(self.visit(child)?);
        }
        self.combine(results)
    }

    /// The neutral output, `combine(vec![])`.
    ///
    /// ANTLR's `defaultResult()`. Return this from a leaf's
    /// `visit_*` method, or from any override meaning: handled,
    /// do not recurse.
    fn default_result(&mut self) -> Result<Self::Output, Self::Error> {
        self.combine(vec![])
    }

    /// Visits an `ERROR` node, one of tree-sitter's
    /// parse-recovery nodes.
    ///
    /// ANTLR's `visitErrorNode`. Defaults to recursing into its
    /// children.
    fn error_visitor(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        self.children_visitor(node)
    }

    /// Visits a `MISSING` node, tree-sitter's zero-width
    /// parse-recovery marker.
    ///
    /// [`Node::kind`] reports the *expected* kind on a `MISSING`
    /// node, not a distinct missing kind, so [`Visitor::visit`]
    /// checks [`Node::is_missing`] before kind dispatch and
    /// routes here instead. Defaults to
    /// [`Visitor::default_result`]: treated as absent, not
    /// recursed into.
    fn missing_visitor(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        let _ = node;
        self.default_result()
    }

    /// Dispatches `node` to the appropriate `visit_<kind>` method
    /// based on its kind.
    ///
    /// [`Node::is_missing`] is checked first: `node.kind()` alone
    /// can't distinguish a `MISSING` node from an ordinary one of
    /// whatever kind the parser expected there, so a `MISSING`
    /// node always routes to [`Visitor::missing_visitor`]
    /// regardless of what kind it reports.
    fn visit(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        if node.is_missing() {
            return self.missing_visitor(node);
        }
        match node.kind() {
            "program" => self.visit_program(node),
            "decl" => self.visit_decl(node),
            "expr" => self.visit_expr(node),
            "ident" => self.visit_ident(node),
            "num" => self.visit_num(node),
            "ERROR" => self.error_visitor(node),
            _ => self.children_visitor(node),
        }
    }

    /// Visits a `program` node.
    fn visit_program(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        self.children_visitor(node)
    }

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

    /// Visits a `expr` node.
    fn visit_expr(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        self.children_visitor(node)
    }

    /// Visits a `ident` node.
    ///
    /// **Anonymous children** (not visited by default): `/[a-z]+/`
    ///
    /// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
    fn visit_ident(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        let _ = node;
        self.default_result()
    }

    /// Visits a `num` node.
    ///
    /// **Anonymous children** (not visited by default): `/[0-9]+/`
    ///
    /// **Leaf node**: no visible children; defaults to [`Visitor::default_result`].
    fn visit_num(&mut self, node: Node<'tree>) -> Result<Self::Output, Self::Error> {
        let _ = node;
        self.default_result()
    }
}
