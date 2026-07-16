use crate::dom::GrammarNode::{self, *};
use crate::dom::ParseError::SyntaxError;
use crate::dom::directive::{ConflictGroup, DirectiveItem, NameOrLiteral, loc};
use crate::dom::{
    Diagnostic, Grammar, ParseError, PrecKind, PrecLevel, PrecedenceGroup, Production,
    ReservedEntry,
};
use crate::util::syntax_error_diagnostics;
use std::collections::HashSet;
use std::path::PathBuf;
use tree_sitter::Node;

/// Groups a source file's text, filename, and resolved filesystem path for use throughout the visitor.
pub struct SourceFile<'a> {
    /// The original source text.
    pub source: &'a str,
    /// The filename associated with this source (empty string if unknown).
    pub filename: &'a str,
    /// Canonical absolute path to the file, used to resolve `%include` paths.
    /// `None` when parsing from stdin or an in-memory string (no file backing).
    pub path: Option<PathBuf>,
}

/// Tracks `%include` resolution state across a single top-level parse.
///
/// Bundles two canonical-path sets that both grow during recursive descent into
/// included files but are cleared under different rules:
/// - `stack`: paths currently being processed (an include-of-an-include chain).
///   Popped on backtrack (see [`visit_include_directive`]) so that diamond includes
///   (two files independently including the same third file) aren't misreported as
///   a cycle. A path already on `stack` when re-encountered is a genuine cycle.
/// - `merged`: paths that have been fully parsed and merged into the composed
///   grammar at least once. Never popped, so a second `%include` of the same file —
///   direct or transitive — is a silent no-op instead of duplicating its rules and
///   directives into the grammar a second time (issue #301).
#[derive(Default)]
struct IncludeState {
    /// Canonical paths on the current include chain; popped on backtrack.
    stack: HashSet<PathBuf>,
    /// Canonical paths already fully parsed and merged; never popped.
    merged: HashSet<PathBuf>,
}

/// Parses a BNF source string and returns the [`Grammar`] DOM and any diagnostics.
pub fn parse_source(src: &str) -> Result<(Grammar, Vec<Diagnostic>), ParseError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .map_err(|_| ParseError::ParseFailed)?;
    let tree = parser.parse(src, None).ok_or(ParseError::ParseFailed)?;
    let ctx = SourceFile {
        source: src,
        filename: "",
        path: None,
    };
    if tree.root_node().has_error() {
        let diagnostics = syntax_error_diagnostics(&tree.root_node(), &ctx);
        return Err(SyntaxError(diagnostics));
    }
    visit_grammar(&tree.root_node(), &ctx)
}

/// Returns `Ok(())` if `node.kind() == node_type`, otherwise an [`ParseError::UnexpectedNodeType`] error.
fn ensure_node_type(node: &Node, node_type: &str) -> Result<(), ParseError> {
    if node.kind() != node_type {
        Err(ParseError::UnexpectedNodeType {
            expected: node_type.to_string(),
            got: node.kind().to_string(),
        })
    } else {
        Ok(())
    }
}

/// Converts the root `grammar` tree-sitter node into a [`Grammar`] DOM.
///
/// Returns the grammar and any diagnostics from cross-reference checks.
/// Seeds the include-cycle stack with the current file's path before delegating
/// to [`visit_grammar_inner`].
pub fn visit_grammar(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<(Grammar, Vec<Diagnostic>), ParseError> {
    let mut state = IncludeState::default();
    if let Some(path) = &ctx.path {
        state.stack.insert(path.clone());
    }
    visit_grammar_inner(node, ctx, &mut state)
}

/// Inner implementation of [`visit_grammar`], carrying the `%include` resolution state.
fn visit_grammar_inner(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
    state: &mut IncludeState,
) -> Result<(Grammar, Vec<Diagnostic>), ParseError> {
    ensure_node_type(node, "grammar")?;
    let mut grammar = Grammar::new();
    let count = node.child_count() as u32;
    for i in 0..count {
        let child = node.child(i).expect("child index in bounds");
        match child.kind() {
            "rule" => {
                let prod = visit_rule(&mut grammar, &child, ctx)?;
                if grammar.productions.contains_key(&prod.name) {
                    grammar.parse_diagnostics.push(Diagnostic::warning(format!(
                        "rule '{}' is defined more than once ({})",
                        prod.name,
                        loc(&prod.filename, prod.line)
                    )));
                }
                grammar.record_own_first_rule(&prod.name);
                grammar.productions.insert(prod.name.clone(), prod);
            }
            "conflictsDirective" => {
                grammar
                    .conflicts
                    .extend(visit_conflicts_directive(&child, ctx)?);
            }
            "precedencesDirective" => {
                grammar
                    .precedences
                    .extend(visit_precedences_directive(&child, ctx)?);
            }
            "inlineDirective" => {
                grammar.inline.extend(visit_inline_directive(&child, ctx));
            }
            "supertypesDirective" => {
                grammar
                    .supertypes
                    .extend(visit_supertypes_directive(&child, ctx));
            }
            "extrasDirective" => {
                grammar.extras.extend(visit_extras_directive(&child, ctx));
            }
            "externalsDirective" => {
                grammar
                    .externals
                    .extend(visit_externals_directive(&child, ctx)?);
            }
            "wordDirective" => {
                let item = visit_simple_directive(&child, ctx);
                if let Some(diag) = grammar.declare_word(item) {
                    grammar.parse_diagnostics.push(diag);
                }
            }
            "axiomDirective" => {
                let item = visit_simple_directive(&child, ctx);
                if let Some(diag) = grammar.declare_axiom(item) {
                    grammar.parse_diagnostics.push(diag);
                }
            }
            "includeDirective" => {
                visit_include_directive(&mut grammar, &child, ctx, state)?;
            }
            "reservedDirective" => {
                grammar
                    .reserved_sets
                    .extend(visit_reserved_directive(&child, ctx)?);
            }
            _ => {}
        }
    }
    let warnings = grammar.check();
    Ok((grammar, warnings))
}

/// Converts a `reservedDirective` node into a list of [`ReservedEntry`]s, one per `reservedEntry`.
fn visit_reserved_directive(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<Vec<ReservedEntry>, ParseError> {
    let items = (0..node.named_child_count() as u32)
        .map(|j| {
            let item_node = node.named_child(j).expect("named child index in bounds");
            visit_reserved_item(&item_node, ctx)
        })
        .collect();
    Ok(items)
}

/// Converts a `reservedEntry` node into a [`ReservedEntry`], reading the `set` field and
/// the `nonTerminalOrLiteral` items that follow it.
fn visit_reserved_item(node: &Node<'_>, ctx: &SourceFile<'_>) -> ReservedEntry {
    let line = node.start_position().row + 1;
    let set_name = node
        .child_by_field_name("set")
        .expect("reservedItem has a set name")
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();

    let rule_names = (1..node.named_child_count() as u32)
        .map(|i| {
            visit_name_or_literal(
                &node.named_child(i).expect("named child index in bounds"),
                ctx,
            )
        })
        .collect();

    ReservedEntry {
        set_name,
        rule_names,
        line,
        filename: ctx.filename.to_string(),
    }
}

/// Resolves a `%include` directive, parses the referenced file, and merges it into `grammar`.
///
/// `state.stack` holds canonical paths on the current include chain and is checked first,
/// so a genuine cycle (e.g. A includes B which includes A again) is always reported as an
/// error, even if the disjointness with `state.merged` asserted below were ever broken by
/// a future change — failing loud beats silently skipping a file and producing an
/// incomplete grammar.
///
/// `state.merged` holds canonical paths already fully parsed and merged, anywhere in this
/// parse. A path never occupies both sets at once: it enters `stack` before recursing and
/// the swap to `merged` happens atomically with its removal from `stack` on backtrack (see
/// below), so at any point during traversal `stack` and `merged` are disjoint. Checking
/// `merged` lets a diamond include (two files independently including the same third file)
/// skip re-reading, re-parsing, and re-merging that file a second time, instead of
/// duplicating its rules and directives into the composed grammar (issue #301).
///
/// Check diagnostics for each included file in isolation are discarded — cross-reference
/// checks run on the fully-merged grammar once the top-level `visit_grammar` call returns.
fn visit_include_directive(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
    state: &mut IncludeState,
) -> Result<(), ParseError> {
    let raw = node
        .named_child(0)
        .expect("includeDirective has a literal child")
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8");
    let path_str = &raw[1..raw.len() - 1]; // strip surrounding ' or "

    let base_dir = ctx
        .path
        .as_deref()
        .and_then(|p| p.parent())
        .ok_or(ParseError::IncludeFromStdin)?;
    let resolved = base_dir.join(path_str);

    let source = std::fs::read_to_string(&resolved)
        .map_err(|_| ParseError::IncludeNotFound(resolved.display().to_string()))?;

    // Cycle/dedup detection: canonicalize after confirming the file exists (read above).
    let canonical = resolved.canonicalize().ok();
    if let Some(ref canon) = canonical {
        if state.stack.contains(canon) {
            return Err(ParseError::IncludeCycle(resolved.display().to_string()));
        }
        if state.merged.contains(canon) {
            // Already fully included elsewhere in this graph — skip re-merging it.
            return Ok(());
        }
        state.stack.insert(canon.clone());
    }

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bnf::LANGUAGE.into())
        .map_err(|_| ParseError::ParseFailed)?;
    let tree = parser.parse(&source, None).ok_or(ParseError::ParseFailed)?;

    let filename_owned = resolved.to_string_lossy().into_owned();
    let included_ctx = SourceFile {
        source: &source,
        filename: &filename_owned,
        path: canonical.clone(),
    };

    if tree.root_node().has_error() {
        let diagnostics = syntax_error_diagnostics(&tree.root_node(), &included_ctx);
        return Err(ParseError::SyntaxError(diagnostics));
    }

    let (included, _) = visit_grammar_inner(&tree.root_node(), &included_ctx, state)?;

    // Backtrack the cycle stack, and remember this file as fully merged so any later
    // %include of it (direct or transitive) is skipped instead of duplicated.
    if let Some(ref canon) = canonical {
        state.stack.remove(canon);
        state.merged.insert(canon.clone());
    }

    grammar.merge_from(included);
    Ok(())
}

/// Collects every named child of `node` into a [`Vec<DirectiveItem>`], recording the
/// directive's 1-based source line for each entry.
fn collect_directive_items(node: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<DirectiveItem> {
    let line = node.start_position().row + 1;
    let filename = ctx.filename.to_string();
    (0..node.named_child_count() as u32)
        .map(|i| {
            let name = node
                .named_child(i)
                .expect("named child index in bounds")
                .utf8_text(ctx.source.as_bytes())
                .expect("valid UTF-8")
                .to_string();
            DirectiveItem {
                name,
                line,
                filename: filename.clone(),
            }
        })
        .collect()
}

/// Converts an `inlineDirective` node into a list of [`DirectiveItem`]s.
fn visit_inline_directive(node: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<DirectiveItem> {
    collect_directive_items(node, ctx)
}

/// Converts an `extrasDirective` node into a list of [`DirectiveItem`]s.
fn visit_extras_directive(node: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<DirectiveItem> {
    collect_directive_items(node, ctx)
}

/// Converts a `supertypesDirective` node into a list of [`DirectiveItem`]s.
fn visit_supertypes_directive(node: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<DirectiveItem> {
    collect_directive_items(node, ctx)
}

/// Converts an `axiomDirective` node into a single [`DirectiveItem`] for the '%axiom' or '%word'
fn visit_simple_directive(node: &Node<'_>, ctx: &SourceFile<'_>) -> DirectiveItem {
    let line = node.start_position().row + 1;
    let name = node
        .named_child(0)
        .expect("adirective has a nonTerminal child")
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();
    DirectiveItem {
        name,
        line,
        filename: ctx.filename.to_string(),
    }
}

/// Converts a `precedencesDirective` node into a list of [`PrecedenceGroup`]s.
fn visit_precedences_directive(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<Vec<PrecedenceGroup>, ParseError> {
    let line = node.start_position().row + 1;
    let mut groups = Vec::new();
    for i in 0..node.named_child_count() as u32 {
        let child = node.named_child(i).expect("named child index in bounds");
        if child.kind() == "precedenceGroup" {
            let items = (0..child.named_child_count() as u32)
                .map(|j| {
                    let item_node = child.named_child(j).expect("named child index in bounds");
                    let name_or_literal = visit_name_or_literal(&item_node, ctx);
                    if let NameOrLiteral::Literal(literal) = name_or_literal {
                        NameOrLiteral::Literal(normalize_literal(literal.as_str()))
                    } else {
                        name_or_literal
                    }
                })
                .collect();
            groups.push(PrecedenceGroup {
                items,
                line,
                filename: ctx.filename.to_string(),
            });
        }
    }
    Ok(groups)
}

/// Converts a `externalsDirective` node into a list of [`NameOrLiteral`]s.
fn visit_externals_directive(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<Vec<NameOrLiteral>, ParseError> {
    let items = (0..node.named_child_count() as u32)
        .map(|j| {
            let item_node = node.named_child(j).expect("named child index in bounds");
            visit_name_or_literal(&item_node, ctx)
        })
        .collect();
    Ok(items)
}

/// Converts a `nonTerminalOrLiteral` node into a [`NameOrLiteral`].
fn visit_name_or_literal(node: &Node<'_>, ctx: &SourceFile<'_>) -> NameOrLiteral {
    let text = node
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();
    let inner = node
        .named_child(0)
        .expect("nonTerminalOrLiteral has one child");
    match inner.kind() {
        "literal" => NameOrLiteral::Literal(text),
        _ => NameOrLiteral::Name(text),
    }
}

/// Converts a `conflictsDirective` node into a list of [`ConflictGroup`]s.
fn visit_conflicts_directive(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<Vec<ConflictGroup>, ParseError> {
    let line = node.start_position().row + 1;
    let mut groups = Vec::new();
    for i in 0..node.named_child_count() as u32 {
        let child = node.named_child(i).expect("named child index in bounds");
        if child.kind() == "conflictGroup" {
            let rules = (0..child.named_child_count() as u32)
                .map(|j| {
                    child
                        .named_child(j)
                        .expect("named child index in bounds")
                        .utf8_text(ctx.source.as_bytes())
                        .expect("valid UTF-8")
                        .to_string()
                })
                .collect();
            groups.push(ConflictGroup {
                rules,
                line,
                filename: ctx.filename.to_string(),
            });
        }
    }
    Ok(groups)
}

/// Converts a `rule` node into a [`Production`].
fn visit_rule(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<Production, ParseError> {
    ensure_node_type(node, "rule")?;
    let rule_name = node
        .child_by_field_name("name")
        .expect("rule has name field");
    let rule_body = node
        .child_by_field_name("body")
        .expect("rule has body field");
    // Extract LHS name directly — it is a definition, not an rhs reference.
    ensure_node_type(&rule_name, "nonTerminal")?;
    let name = rule_name
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();
    let body = visit(grammar, &rule_body, ctx)?;
    let line = node.start_position().row + 1;
    let filename = ctx.filename.to_string();
    Ok(Production {
        name,
        body,
        line,
        filename,
    })
}

/// Converts a `nonTerminal` node into a [`GrammarNode::NonTerminal`] and records the name.
fn visit_non_terminal(grammar: &mut Grammar, node: &Node<'_>, ctx: &SourceFile<'_>) -> GrammarNode {
    let text = node.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8");
    grammar.rhs_nonterminals.insert(text.to_string());
    NonTerminal(text.to_string())
}

/// Converts a `pattern` node into a [`GrammarNode::TerminalPattern`].
fn visit_pattern(node: &Node<'_>, ctx: &SourceFile<'_>) -> GrammarNode {
    let text = node.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8");
    TerminalPattern(text.to_string())
}

/// Normalises a BNF literal to tree-sitter single-quote form, converting double-quoted strings.
fn normalize_literal(text: &str) -> String {
    if text.starts_with('"') {
        let inner = &text[1..text.len() - 1];
        let content = inner.replace("\\\"", "\"").replace('\'', "\\'");
        format!("'{content}'")
    } else {
        text.to_string()
    }
}

/// Converts a `literal` node into a [`GrammarNode::TerminalLiteral`], normalising quotes.
fn visit_literal(node: &Node<'_>, ctx: &SourceFile<'_>) -> GrammarNode {
    let text = node.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8");
    TerminalLiteral(normalize_literal(text))
}

/// Converts a `ruleBody` or `ruleBodyInner` node into a [`GrammarNode`], wrapping multiple alternatives in [`GrammarNode::Choice`].
fn visit_rule_body(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let count = node.child_count() as u32;
    if count == 1 {
        visit(grammar, &node.child(0).expect("child 0 exists"), ctx)
    } else {
        let mut choice = Vec::new();
        let mut i: u32 = 0;
        while i < count {
            choice.push(visit(
                grammar,
                &node.child(i).expect("child index in bounds"),
                ctx,
            )?);
            i += 2;
        }
        Ok(Choice(choice))
    }
}

/// Converts a `symbol` node, applying any Kleene quantifier and optional field label.
fn visit_symbol(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let symbol = visit(
        grammar,
        &node
            .child_by_field_name("content")
            .expect("symbol has content field"),
        ctx,
    )?;
    let kleene = node
        .child_by_field_name("kleene")
        .as_ref()
        .map(|n| n.kind())
        .unwrap_or("");
    let with_kleene = match kleene {
        "plus" => OneOrMore(Box::new(symbol)),
        "asterisk" => ZeroOrMore(Box::new(symbol)),
        "questionMark" => Optional(Box::new(symbol)),
        _ => symbol,
    };
    Ok(
        if let Some(label_node) = node.child_by_field_name("label") {
            let text = label_node
                .utf8_text(ctx.source.as_bytes())
                .expect("valid UTF-8");
            let name = text.trim_end_matches(':').to_string();
            Field(name, Box::new(with_kleene))
        } else {
            with_kleene
        },
    )
}

/// Extracts the precedence kind and optional level (integer or name) from a `precAnnotation` node.
fn parse_prec_annotation(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<(PrecKind, Option<PrecLevel>), ParseError> {
    let kind_node = node
        .child_by_field_name("kind")
        .expect("precAnnotation has kind field");
    let kind_text = kind_node
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8");
    let kind = match kind_text {
        "prec" => PrecKind::Plain,
        "prec.left" => PrecKind::Left,
        "prec.right" => PrecKind::Right,
        "prec.dynamic" => PrecKind::Dynamic,
        other => return Err(ParseError::UnknownNodeKind(other.to_string())),
    };
    let level = if let Some(n) = node.child_by_field_name("level") {
        let text = n.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8");
        Some(PrecLevel::Integer(
            text.parse::<i32>()
                .map_err(|_| ParseError::MalformedProduction)?,
        ))
    } else if let Some(name) = node.child_by_field_name("name") {
        let text = normalize_literal(name.utf8_text(ctx.source.as_bytes()).expect("valid UTF-8"));
        Some(PrecLevel::Name(text))
    } else {
        None
    };
    Ok((kind, level))
}

/// Converts a `symbolSeq` or `symbolSeqInner` node into a sequence (or single node), wrapping with precedence if annotated.
fn visit_symbol_seq(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let prec_annotation = if let Some(n) = node.child_by_field_name("prec") {
        Some(parse_prec_annotation(&n, ctx)?)
    } else {
        None
    };

    let count = node.child_count() as u32;
    let symbol_count = if prec_annotation.is_some() {
        count - 1
    } else {
        count
    };

    let body = if symbol_count == 1 {
        visit(grammar, &node.child(0).expect("child 0 exists"), ctx)?
    } else {
        let seq = (0..symbol_count)
            .map(|i| visit(grammar, &node.child(i).expect("child index in bounds"), ctx))
            .collect::<Result<Vec<_>, _>>()?;
        Sequence(seq)
    };

    Ok(match prec_annotation {
        Some((kind, level)) => {
            if let Some(PrecLevel::Name(prec_name)) = &level {
                grammar.prec_name_refs.push(DirectiveItem {
                    name: prec_name.clone(),
                    line: node.start_position().row + 1,
                    filename: ctx.filename.to_string(),
                })
            }
            Prec(kind, level, Box::new(body))
        }
        None => body,
    })
}

/// Converts a `subSeq` node by delegating to its `body` field.
fn visit_symbol_subseq(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("subSeq has body field"),
        ctx,
    )
}

/// Converts a `tokenExpr` node into a [`GrammarNode::Token`] wrapping its body.
fn visit_token_expr(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let inner = visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("tokenExpr has body field"),
        ctx,
    )?;
    Ok(Token(Box::new(inner)))
}

/// Converts a `tokenImmediateExpr` node into a [`GrammarNode::TokenImmediate`] wrapping its body.
fn visit_token_immediate_expr(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let inner = visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("tokenImmediateExpr has body field"),
        ctx,
    )?;
    Ok(TokenImmediate(Box::new(inner)))
}

/// Converts a `precGroup` node into a [`GrammarNode::Prec`] wrapping its body.
fn visit_prec_group(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let body = visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("precGroup has body field"),
        ctx,
    )?;
    let annotation = node
        .child_by_field_name("annotation")
        .expect("precGroup has annotation field");
    let (kind, level) = parse_prec_annotation(&annotation, ctx)?;
    if let Some(PrecLevel::Name(prec_name)) = &level {
        grammar.prec_name_refs.push(DirectiveItem {
            name: prec_name.clone(),
            line: node.start_position().row + 1,
            filename: ctx.filename.to_string(),
        })
    }
    Ok(Prec(kind, level, Box::new(body)))
}

/// Converts a `reservedGroup` node into a [`GrammarNode::Reserved`] wrapping its body.
fn visit_reserved_group(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let body = visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("reservedGroup has body field"),
        ctx,
    )?;
    let set_name = node
        .child_by_field_name("set")
        .expect("reservedGroup has set field")
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();
    grammar.reserved_set_refs.push(DirectiveItem {
        name: set_name.clone(),
        line: node.start_position().row + 1,
        filename: ctx.filename.to_string(),
    });
    Ok(Reserved(set_name, Box::new(body)))
}

/// Converts an `aliasGroup` node into a [`GrammarNode::Alias`].
fn visit_alias_group(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    let body = visit(
        grammar,
        &node
            .child_by_field_name("body")
            .expect("aliasGroup has body field"),
        ctx,
    )?;
    let alias_node = node
        .child_by_field_name("alias")
        .expect("aliasGroup has alias field");
    let name_child = alias_node.child(0).expect("aliasName has a child");
    // The alias name is a display label, not a rule reference, so a
    // nonTerminal name must not be recorded in `rhs_nonterminals`.
    // The grammar guarantees the child is a nonTerminal or a literal.
    let name = if name_child.kind() == "nonTerminal" {
        let text = name_child
            .utf8_text(ctx.source.as_bytes())
            .expect("valid UTF-8");
        NonTerminal(text.to_string())
    } else {
        visit_literal(&name_child, ctx)
    };
    Ok(Alias(Box::new(body), Box::new(name)))
}

/// Dispatches a tree-sitter node to the appropriate typed visitor by node kind.
fn visit(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<GrammarNode, ParseError> {
    match node.kind() {
        "nonTerminal" => Ok(visit_non_terminal(grammar, node, ctx)),
        "ruleBody" => visit_rule_body(grammar, node, ctx),
        "symbolSeq" => visit_symbol_seq(grammar, node, ctx),
        "symbol" => visit_symbol(grammar, node, ctx),
        "pattern" => Ok(visit_pattern(node, ctx)),
        "literal" => Ok(visit_literal(node, ctx)),
        "subSeq" => visit_symbol_subseq(grammar, node, ctx),
        "aliasGroup" => visit_alias_group(grammar, node, ctx),
        "tokenExpr" => visit_token_expr(grammar, node, ctx),
        "tokenImmediateExpr" => visit_token_immediate_expr(grammar, node, ctx),
        "precGroup" => visit_prec_group(grammar, node, ctx),
        "reservedGroup" => visit_reserved_group(grammar, node, ctx),
        "ruleBodyInner" => visit_rule_body(grammar, node, ctx),
        "symbolSeqInner" => visit_symbol_seq(grammar, node, ctx),
        kind => Err(ParseError::UnknownNodeKind(kind.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{ParseError, Severity};
    use indoc::indoc;
    use std::fs;

    /// Writes `content` to `$TMPDIR/name` and returns the path.
    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        fs::write(&path, content).unwrap();
        path
    }

    /// Parses the BNF file at `path` (already on disk) through the full visitor,
    /// returning the merged grammar and any diagnostics.
    fn parse_path(path: &std::path::Path) -> Result<(Grammar, Vec<Diagnostic>), ParseError> {
        let source = fs::read_to_string(path).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bnf::LANGUAGE.into())
            .map_err(|_| ParseError::ParseFailed)?;
        let tree = parser.parse(&source, None).ok_or(ParseError::ParseFailed)?;
        let ctx = SourceFile {
            source: &source,
            filename: path.to_str().unwrap_or(""),
            path: path.canonicalize().ok(),
        };
        if tree.root_node().has_error() {
            let diagnostics = syntax_error_diagnostics(&tree.root_node(), &ctx);
            return Err(ParseError::SyntaxError(diagnostics));
        }

        visit_grammar(&tree.root_node(), &ctx)
    }

    // ── basic include ─────────────────────────────────────────────────────────

    #[test]
    /// Productions from an included file are merged into the parent grammar.
    fn include_basic_merges_productions() {
        write_tmp("inc_basic_b.bnf", "rule_b -> 'y' ;");
        let a = write_tmp(
            "inc_basic_a.bnf",
            "%include \"inc_basic_b.bnf\"\nrule_a -> 'x' ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(grammar.productions.contains_key("rule_a"));
        assert!(grammar.productions.contains_key("rule_b"));
    }

    // ── nested include ────────────────────────────────────────────────────────

    #[test]
    /// A→B→C: productions from all three files are visible in the final grammar.
    fn include_nested_merges_all_levels() {
        write_tmp("inc_nest_c.bnf", "rule_c -> 'z' ;");
        write_tmp(
            "inc_nest_b.bnf",
            "%include \"inc_nest_c.bnf\"\nrule_b -> 'y' ;",
        );
        let a = write_tmp(
            "inc_nest_a.bnf",
            "%include \"inc_nest_b.bnf\"\nrule_a -> 'x' ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(grammar.productions.contains_key("rule_a"));
        assert!(grammar.productions.contains_key("rule_b"));
        assert!(grammar.productions.contains_key("rule_c"));
    }

    // ── cycle detection ───────────────────────────────────────────────────────

    #[test]
    /// A circular include (A→B→A) is detected and returns IncludeCycle.
    fn include_cycle_is_detected() {
        write_tmp(
            "inc_cycle_b.bnf",
            "%include \"inc_cycle_a.bnf\"\nrule_b -> 'y' ;",
        );
        let a = write_tmp(
            "inc_cycle_a.bnf",
            "%include \"inc_cycle_b.bnf\"\nrule_a -> 'x' ;",
        );
        let err = parse_path(&a).map(|_| ()).unwrap_err();
        assert!(
            matches!(err, ParseError::IncludeCycle(_)),
            "expected IncludeCycle, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.starts_with("circular %include detected: ") && msg.contains("inc_cycle"),
            "unexpected message: {msg}"
        );
    }

    // ── missing file ──────────────────────────────────────────────────────────

    #[test]
    /// Including a file that does not exist returns IncludeNotFound.
    fn include_missing_file_returns_not_found() {
        let a = write_tmp(
            "inc_missing_a.bnf",
            "%include \"no_such_file_xyzzy.bnf\"\nroot -> 'x' ;",
        );
        let err = parse_path(&a).map(|_| ()).unwrap_err();
        assert!(
            matches!(err, ParseError::IncludeNotFound(_)),
            "expected IncludeNotFound, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.starts_with("included file not found: ") && msg.contains("no_such_file_xyzzy.bnf"),
            "unexpected message: {msg}"
        );
    }

    // ── stdin guard ───────────────────────────────────────────────────────────

    #[test]
    /// parse_source (no backing file) returns IncludeFromStdin on %include.
    fn include_from_stdin_returns_error() {
        let err = parse_source("%include \"foo.bnf\"\nroot -> 'x' ;")
            .map(|_| ())
            .unwrap_err();
        assert!(
            matches!(err, ParseError::IncludeFromStdin),
            "expected IncludeFromStdin, got: {err}"
        );
        assert_eq!(
            err.to_string(),
            "%include cannot be used when reading from stdin"
        );
    }

    // ── duplicate rule warning ────────────────────────────────────────────────

    #[test]
    /// A rule defined in both the root and an included file produces a warning.
    fn include_duplicate_rule_emits_warning() {
        write_tmp("inc_dup_b.bnf", "foo -> 'b' ;");
        let a = write_tmp(
            "inc_dup_a.bnf",
            "%include \"inc_dup_b.bnf\"\nfoo -> 'a' ;\nroot -> foo ;",
        );
        let (grammar, diags) = parse_path(&a).unwrap();
        assert!(grammar.productions.contains_key("foo"));
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Warning
                    && d.message.contains("defined more than once")),
            "expected duplicate-rule warning, got {diags:?}"
        );
    }

    // ── diamond include (#301) ────────────────────────────────────────────────

    #[test]
    /// A includes B and C directly; B also includes C. C's rule is merged exactly
    /// once, with no duplicate-rule warning, even though it's reachable via two paths.
    fn include_diamond_merges_shared_file_once() {
        write_tmp("inc_diamond_c.bnf", "rule_c -> 'z' ;");
        write_tmp(
            "inc_diamond_b.bnf",
            "%include \"inc_diamond_c.bnf\"\nrule_b -> 'y' ;",
        );
        let a = write_tmp(
            "inc_diamond_a.bnf",
            "%include \"inc_diamond_b.bnf\"\n%include \"inc_diamond_c.bnf\"\nrule_a -> 'x' ;",
        );
        let (grammar, diags) = parse_path(&a).unwrap();
        assert!(grammar.productions.contains_key("rule_a"));
        assert!(grammar.productions.contains_key("rule_b"));
        assert!(grammar.productions.contains_key("rule_c"));
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("defined more than once")),
            "diamond include of the same file must not warn, got {diags:?}"
        );
    }

    #[test]
    /// Diamond include where the doubly-included file declares `%word`: it is merged
    /// only once, so no duplicate-`%word` error is raised.
    fn include_diamond_with_word_directive_does_not_duplicate() {
        write_tmp("inc_diamond_word_c.bnf", "%word ident\nident -> /a/ ;");
        write_tmp(
            "inc_diamond_word_b.bnf",
            "%include \"inc_diamond_word_c.bnf\"\nrule_b -> ident ;",
        );
        let a = write_tmp(
            "inc_diamond_word_a.bnf",
            "%include \"inc_diamond_word_b.bnf\"\n%include \"inc_diamond_word_c.bnf\"\nrule_a -> ident ;",
        );
        let (grammar, diags) = parse_path(&a).unwrap();
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("%word declared more than once")),
            "diamond include of a %word-declaring file must not error, got {diags:?}"
        );
        assert_eq!(
            grammar.word.as_ref().map(|w| w.name.as_str()),
            Some("ident")
        );
    }

    // ── %axiom is scoped to the top-level file (#295) ─────────────────────────

    #[test]
    /// An included file's own %axiom does not conflict with the includer's: %axiom
    /// resolution is scoped to the top-level file, so no error is raised and the
    /// includer's own %axiom wins.
    fn include_own_axiom_is_not_a_duplicate_of_included_axiom() {
        write_tmp("inc_axiom_b.bnf", "%axiom b\nb -> 'y' ;");
        let a = write_tmp(
            "inc_axiom_a.bnf",
            "%axiom a\na -> 'x' ;\n%include \"inc_axiom_b.bnf\"",
        );
        let (grammar, diags) = parse_path(&a).unwrap();
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("%axiom declared more than once")),
            "included file's own %axiom must not be reported as a duplicate, got {diags:?}"
        );
        assert_eq!(grammar.root_rule(), Some("a"));
    }

    // ── axiom directive bookkeeping ───────────────────────────────────────────

    #[test]
    /// The %axiom directive records its 1-based source line for diagnostics.
    fn axiom_directive_line_is_recorded() {
        let (g, _) = parse_source("%axiom root\nroot -> 'x' ;\n").unwrap();
        assert_eq!(g.axiom_directive().map(|a| a.line), Some(1));
    }

    // ── axiom from included file is never adopted (#295) ──────────────────────

    #[test]
    /// When the parent has no %axiom, an included file's %axiom is not adopted:
    /// the parent's start rule falls back to its own first-declared rule, even
    /// though the %include appears before that rule and the included file
    /// declares its own %axiom.
    fn include_does_not_adopt_axiom_from_included_file() {
        write_tmp("inc_ax_b.bnf", "%axiom b\nb -> 'y' ;");
        let a = write_tmp("inc_ax_a.bnf", "%include \"inc_ax_b.bnf\"\na -> b ;");
        let (grammar, _) = parse_path(&a).unwrap();
        assert_eq!(
            grammar.axiom_directive(),
            None,
            "an included file's %axiom must never be adopted by the includer"
        );
        assert_eq!(
            grammar.root_rule(),
            Some("a"),
            "with no %axiom, the includer's own first rule must be the start rule, \
             not the included file's %axiom or first rule"
        );
    }

    #[test]
    /// A grammar with its own %axiom resolves that %axiom normally when parsed
    /// standalone (not included from anywhere) — %include-scoping must not
    /// affect the ordinary, non-included case.
    fn axiom_resolves_normally_when_file_is_not_included() {
        let b = write_tmp("inc_ax_standalone_b.bnf", "%axiom b\nb -> 'y' ;");
        let (grammar, _) = parse_path(&b).unwrap();
        assert_eq!(grammar.root_rule(), Some("b"));
    }

    // ── syntax error in included file ─────────────────────────────────────────

    #[test]
    /// An included file with a syntax error returns ParseError::SyntaxError
    /// with diagnostics located in the included file, not the includer.
    fn include_syntax_error_in_included_file() {
        // "-> 'x' ;" has no left-hand side and is not valid BNF syntax.
        write_tmp("inc_synerr_b.bnf", "-> 'x' ;");
        let a = write_tmp(
            "inc_synerr_a.bnf",
            "%include \"inc_synerr_b.bnf\"\nroot -> 'y' ;",
        );
        let err = parse_path(&a).map(|_| ()).unwrap_err();
        assert!(matches!(err, ParseError::SyntaxError(_)));
        // Display joins the diagnostic messages; locations must point into
        // the included file, not the includer.
        let msg = err.to_string();
        assert!(msg.contains("inc_synerr_b.bnf"));
        assert!(!msg.contains("inc_synerr_a.bnf"));
    }

    #[test]
    /// A syntax error in the top-level file itself is located in that file.
    fn syntax_error_in_top_level_file() {
        let path = write_tmp("synerr_top.bnf", "root => 'a' ;");
        let err = parse_path(&path).map(|_| ()).unwrap_err();
        assert!(matches!(err, ParseError::SyntaxError(_)));
        let msg = err.to_string();
        assert!(msg.contains("synerr_top.bnf"));
        assert!(msg.contains(":1:"));
    }

    #[test]
    /// parse_source detects syntax errors, locating them without a filename.
    fn parse_source_syntax_error_reports_bare_location() {
        let err = parse_source("root => 'a' ;").map(|_| ()).unwrap_err();
        assert!(matches!(err, ParseError::SyntaxError(_)));
        assert!(
            err.to_string()
                .contains("syntax error at line 1:1 near 'root => 'a' ;'")
        );
    }

    // ── merge directives ──────────────────────────────────────────────────────

    #[test]
    /// %inline from an included file appears in the merged grammar.
    fn include_merges_inline_directive() {
        write_tmp(
            "inc_inline_b.bnf",
            "%inline _helper\n_helper -> 'y' ;\nroot -> _helper ;",
        );
        let a = write_tmp(
            "inc_inline_a.bnf",
            "%include \"inc_inline_b.bnf\"\na -> 'x' ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(
            grammar.inline.iter().any(|d| d.name == "_helper"),
            "expected %inline from included file in merged grammar"
        );
    }

    #[test]
    /// %extras from an included file appears in the merged grammar.
    fn include_merges_extras_directive() {
        write_tmp("inc_extras_b.bnf", "%extras /\\s/\nb -> 'y' ;");
        let a = write_tmp(
            "inc_extras_a.bnf",
            "%include \"inc_extras_b.bnf\"\nroot -> b ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(
            grammar.extras.iter().any(|d| d.name == "/\\s/"),
            "expected %extras from included file in merged grammar"
        );
    }

    #[test]
    /// %supertypes from an included file appears in the merged grammar.
    fn include_merges_supertypes_directive() {
        write_tmp("inc_super_b.bnf", "%supertypes expr\nexpr -> 'y' ;");
        let a = write_tmp(
            "inc_super_a.bnf",
            "%include \"inc_super_b.bnf\"\nroot -> expr ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(
            grammar.supertypes.iter().any(|d| d.name == "expr"),
            "expected %supertypes from included file in merged grammar"
        );
    }

    #[test]
    /// %conflicts from an included file appears in the merged grammar.
    fn include_merges_conflicts_directive() {
        write_tmp(
            "inc_conf_b.bnf",
            "%conflicts [a, b]\na -> 'x' ;\nb -> 'y' ;",
        );
        let a = write_tmp("inc_conf_a.bnf", "%include \"inc_conf_b.bnf\"\nroot -> a ;");
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(
            grammar.conflicts.iter().any(
                |cg| cg.rules.contains(&"a".to_string()) && cg.rules.contains(&"b".to_string())
            ),
            "expected %conflicts from included file in merged grammar"
        );
    }

    #[test]
    /// %externals from an included file appears in the merged grammar.
    fn include_merges_externals_directive() {
        write_tmp("inc_ext_b.bnf", "%externals tok\nb -> 'y' ;");
        let a = write_tmp("inc_ext_a.bnf", "%include \"inc_ext_b.bnf\"\nroot -> b ;");
        let (grammar, _) = parse_path(&a).unwrap();
        assert!(
            grammar
                .externals
                .contains(&NameOrLiteral::Name("tok".into())),
            "expected %externals from included file in merged grammar"
        );
    }

    // ── %precedences directive ────────────────────────────────────────────────

    #[test]
    /// Name and Literal items in a `%precedences` group are assigned the correct variant.
    fn precedences_directive_name_and_literal() {
        let (g, diags) = parse_source(indoc! {"
            %precedences [foo, 'bar'], [baz]
            root -> foo baz ;
            foo -> /a/ ;
            baz -> /b/ ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.precedences.len(), 2);
        assert_eq!(
            g.precedences[0].items,
            vec![
                NameOrLiteral::Name("foo".into()),
                NameOrLiteral::Literal("'bar'".into()),
            ]
        );
        assert_eq!(
            g.precedences[1].items,
            vec![NameOrLiteral::Name("baz".into())]
        );
    }

    #[test]
    /// A single `%precedences` line with a single group is parsed correctly.
    fn precedences_directive_single_group() {
        let (g, diags) = parse_source(indoc! {"
            %precedences [a, b]
            root -> a b ;
            a -> /x/ ;
            b -> /y/ ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.precedences.len(), 1);
        assert_eq!(
            g.precedences[0].items,
            vec![
                NameOrLiteral::Name("a".into()),
                NameOrLiteral::Name("b".into()),
            ]
        );
    }

    #[test]
    /// Multiple `%precedences` lines are merged additively into grammar.precedences.
    fn precedences_directive_multiple_lines_are_additive() {
        let (g, diags) = parse_source(indoc! {"
            %precedences [a, b]
            %precedences [c]
            root -> a b c ;
            a -> /x/ ;
            b -> /y/ ;
            c -> /z/ ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.precedences.len(), 2);
        assert_eq!(
            g.precedences[0].items,
            vec![
                NameOrLiteral::Name("a".into()),
                NameOrLiteral::Name("b".into()),
            ]
        );
        assert_eq!(
            g.precedences[1].items,
            vec![NameOrLiteral::Name("c".into())]
        );
    }

    #[test]
    /// The source line of a `%precedences` directive is recorded (1-based).
    fn precedences_directive_line_is_recorded() {
        let (g, _) = parse_source("%precedences [a]\na -> /x/ ;").unwrap();
        assert_eq!(g.precedences[0].line, 1);
    }

    // ── %externals directive ────────────────────────────────────────────────

    #[test]
    /// Name and Literal items in a `%externals` directive are assigned the correct variant.
    fn externals_directive_name_and_literal() {
        let (g, diags) = parse_source(indoc! {"
            %externals foo, 'bar'
            root -> foo ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            g.externals,
            vec![
                NameOrLiteral::Name("foo".into()),
                NameOrLiteral::Literal("'bar'".into()),
            ]
        );
    }

    #[test]
    /// Multiple `%externals` lines are merged additively into grammar.externals.
    fn externals_directive_multiple_lines_are_additive() {
        let (g, diags) = parse_source(indoc! {"
            %externals a
            %externals b, c
            root -> a b c ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            g.externals,
            vec![
                NameOrLiteral::Name("a".into()),
                NameOrLiteral::Name("b".into()),
                NameOrLiteral::Name("c".into()),
            ]
        );
    }

    // ── %reserved directive ───────────────────────────────────────────────────

    #[test]
    /// Literal items in `%reserved` entries are assigned the `Literal` variant; an
    /// empty bracket list produces an empty `rule_names`.
    fn reserved_directive_literal_entries() {
        let (g, diags) = parse_source(indoc! {"
            %reserved kw: ['if', 'else'], prop: []
            root -> 'if' ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.reserved_sets.len(), 2);
        assert_eq!(g.reserved_sets[0].set_name, "kw");
        assert_eq!(
            g.reserved_sets[0].rule_names,
            vec![
                NameOrLiteral::Literal("'if'".into()),
                NameOrLiteral::Literal("'else'".into()),
            ]
        );
        assert_eq!(g.reserved_sets[1].set_name, "prop");
        assert_eq!(g.reserved_sets[1].rule_names, vec![]);
    }

    #[test]
    /// Bare nonterminal items in a `%reserved` entry are assigned the `Name` variant.
    fn reserved_directive_nonterminal_entries() {
        let (g, diags) = parse_source(indoc! {"
            %reserved kw2: [if, else]
            root -> if else ;
            if -> 'if' ;
            else -> 'else' ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(
            g.reserved_sets[0].rule_names,
            vec![
                NameOrLiteral::Name("if".into()),
                NameOrLiteral::Name("else".into()),
            ]
        );
    }

    #[test]
    /// A rule-level `(body %reserved setName)` annotation records the referenced set
    /// name, with its own source line, in `grammar.reserved_set_refs`.
    fn reserved_group_records_rule_level_annotation() {
        let (g, diags) = parse_source(indoc! {"
            %reserved kw: []
            id -> (/[a-z]+/ %reserved kw) ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.reserved_set_refs.len(), 1);
        assert_eq!(g.reserved_set_refs[0].name, "kw");
        assert_eq!(g.reserved_set_refs[0].line, 2);
    }

    #[test]
    /// Guards the `"reservedDirective"` dispatch arm in `visit_grammar_inner`, which has
    /// no compiler safety net (silent `_ => {}` fallback): if that arm were ever removed,
    /// this assertion fails loudly instead of the directive silently vanishing.
    fn reserved_directive_dispatch_is_not_silently_skipped() {
        let (g, diags) = parse_source(indoc! {"
            %reserved kw: ['if']
            root -> 'if' ;
        "})
        .unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert!(
            !g.reserved_sets.is_empty(),
            "%reserved directive must not be silently dropped"
        );
    }

    // ── %word directive ───────────────────────────────────────────────────────

    #[test]
    /// Parsing `%word identifier` sets `grammar.word` to `Some("identifier")`.
    fn word_directive_is_recorded() {
        let (g, diags) = parse_source("%word identifier\nidentifier -> /a/ ;\n").unwrap();
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(g.word.as_ref().map(|w| w.name.as_str()), Some("identifier"));
    }

    #[test]
    /// The `%word` directive records its 1-based source line.
    fn word_directive_line_is_recorded() {
        let (g, _) = parse_source("%word ident\nident -> /a/ ;\n").unwrap();
        assert_eq!(g.word.as_ref().map(|w| w.line), Some(1));
    }

    #[test]
    /// A duplicate `%word` produces an error diagnostic and the first wins.
    fn duplicate_word_directive_emits_error() {
        let src = "%word foo\n%word bar\nfoo -> /a/ ;\nbar -> /b/ ;\n";
        let (g, diags) = parse_source(src).unwrap();
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error
                && d.message.contains("%word declared more than once")),
            "expected duplicate-%word error, got {diags:?}"
        );
        assert_eq!(g.word.as_ref().map(|w| w.name.as_str()), Some("foo"));
    }

    #[test]
    /// A `%word` from an included file is adopted when the parent has none.
    fn include_adopts_word_from_included_file() {
        write_tmp("inc_word_b.bnf", "%word ident\nident -> /a/ ;");
        let a = write_tmp(
            "inc_word_a.bnf",
            "%include \"inc_word_b.bnf\"\nroot -> ident ;",
        );
        let (grammar, _) = parse_path(&a).unwrap();
        assert_eq!(
            grammar.word.as_ref().map(|w| w.name.as_str()),
            Some("ident"),
            "word from included file must be adopted when parent has none"
        );
    }
}
