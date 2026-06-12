use crate::dom::directive::{loc, ConflictGroup, DirectiveItem};
use crate::dom::GrammarNode::{self, *};
use crate::dom::ParseError::SyntaxError;
use crate::dom::{Diagnostic, Grammar, ParseError, PrecKind, Production};
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
/// Seeds the cycle-detection set with the current file's path before delegating
/// to [`visit_grammar_inner`].
pub fn visit_grammar(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<(Grammar, Vec<Diagnostic>), ParseError> {
    let mut seen = HashSet::new();
    if let Some(path) = &ctx.path {
        seen.insert(path.clone());
    }
    visit_grammar_inner(node, ctx, &mut seen)
}

/// Inner implementation of [`visit_grammar`], carrying the cycle-detection set.
fn visit_grammar_inner(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
    seen: &mut HashSet<PathBuf>,
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
                grammar.productions.insert(prod.name.clone(), prod);
            }
            "conflictsDirective" => {
                grammar
                    .conflicts
                    .extend(visit_conflicts_directive(&child, ctx)?);
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
            "axiomDirective" => {
                let item = visit_axiom_directive(&child, ctx);
                if let Some(diag) = grammar.declare_axiom(item) {
                    grammar.parse_diagnostics.push(diag);
                }
            }
            "includeDirective" => {
                visit_include_directive(&mut grammar, &child, ctx, seen)?;
            }
            _ => {}
        }
    }
    let warnings = grammar.check();
    Ok((grammar, warnings))
}

/// Resolves a `%include` directive, parses the referenced file, and merges it into `grammar`.
///
/// `seen` is the set of canonical paths on the current include stack; it is used to detect
/// cycles (e.g. A includes B which includes A again). After the recursive call returns,
/// the path is removed from `seen` so that diamond includes (two files independently
/// including the same third file) are allowed — they produce duplicate-rule warnings
/// rather than a cycle error.
///
/// Check diagnostics for each included file in isolation are discarded — cross-reference
/// checks run on the fully-merged grammar once the top-level `visit_grammar` call returns.
fn visit_include_directive(
    grammar: &mut Grammar,
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
    seen: &mut HashSet<PathBuf>,
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

    // Cycle detection: canonicalize after confirming the file exists (read above).
    let canonical = resolved.canonicalize().ok();
    if let Some(ref canon) = canonical {
        if seen.contains(canon) {
            return Err(ParseError::IncludeCycle(resolved.display().to_string()));
        }
        seen.insert(canon.clone());
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

    let (included, _) = visit_grammar_inner(&tree.root_node(), &included_ctx, seen)?;

    // Backtrack: remove from the stack so diamond includes are not misreported as cycles.
    if let Some(ref canon) = canonical {
        seen.remove(canon);
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

/// Converts an `axiomDirective` node into a single [`DirectiveItem`] for the named root rule.
fn visit_axiom_directive(node: &Node<'_>, ctx: &SourceFile<'_>) -> DirectiveItem {
    let line = node.start_position().row + 1;
    let name = node
        .named_child(0)
        .expect("axiomDirective has a nonTerminal child")
        .utf8_text(ctx.source.as_bytes())
        .expect("valid UTF-8")
        .to_string();
    DirectiveItem {
        name,
        line,
        filename: ctx.filename.to_string(),
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

/// Extracts the precedence kind and optional numeric level from a `precAnnotation` node.
fn parse_prec_annotation(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
) -> Result<(PrecKind, Option<i32>), ParseError> {
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
        Some(
            text.parse::<i32>()
                .map_err(|_| ParseError::MalformedProduction)?,
        )
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
        Some((kind, level)) => Prec(kind, level, Box::new(body)),
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
    Ok(Prec(kind, level, Box::new(body)))
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
        "ruleBodyInner" => visit_rule_body(grammar, node, ctx),
        "symbolSeqInner" => visit_symbol_seq(grammar, node, ctx),
        kind => Err(ParseError::UnknownNodeKind(kind.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::{ParseError, Severity};
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

    // ── duplicate %axiom error ────────────────────────────────────────────────

    #[test]
    /// Two %axiom declarations across included files produce an error diagnostic.
    fn include_duplicate_axiom_emits_error() {
        write_tmp("inc_axiom_b.bnf", "%axiom b\nb -> 'y' ;");
        let a = write_tmp(
            "inc_axiom_a.bnf",
            "%axiom a\na -> 'x' ;\n%include \"inc_axiom_b.bnf\"",
        );
        let (_, diags) = parse_path(&a).unwrap();
        assert!(
            diags.iter().any(|d| d.severity == Severity::Error
                && d.message.contains("%axiom declared more than once")),
            "expected duplicate-%axiom error, got {diags:?}"
        );
    }

    // ── axiom directive bookkeeping ───────────────────────────────────────────

    #[test]
    /// The %axiom directive records its 1-based source line for diagnostics.
    fn axiom_directive_line_is_recorded() {
        let (g, _) = parse_source("%axiom root\nroot -> 'x' ;\n").unwrap();
        assert_eq!(g.axiom_directive().map(|a| a.line), Some(1));
    }

    // ── axiom from included file ──────────────────────────────────────────────

    #[test]
    /// When the parent has no %axiom but the included file does, the included
    /// axiom is adopted (the else-branch of merge_from's axiom handling).
    fn include_adopts_axiom_from_included_file() {
        write_tmp("inc_ax_b.bnf", "%axiom b\nb -> 'y' ;");
        let a = write_tmp("inc_ax_a.bnf", "%include \"inc_ax_b.bnf\"\na -> b ;");
        let (grammar, _) = parse_path(&a).unwrap();
        assert_eq!(
            grammar.axiom_directive().map(|ax| ax.name.as_str()),
            Some("b"),
            "axiom from included file must be adopted when parent has none"
        );
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
        assert!(err
            .to_string()
            .contains("syntax error at line 1:1 near 'root => 'a' ;'"));
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
}
