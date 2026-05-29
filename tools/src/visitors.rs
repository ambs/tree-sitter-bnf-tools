use crate::dom::directive::{ConflictGroup, DirectiveItem};
use crate::dom::GrammarNode::{self, *};
use crate::dom::{Diagnostic, Grammar, ParseError, PrecKind, Production};
use tree_sitter::Node;

/// Groups a source file's text and its filename for use throughout the visitor.
pub struct SourceFile<'a> {
    /// The original source text.
    pub source: &'a str,
    /// The filename associated with this source (empty string if unknown).
    pub filename: &'a str,
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
    };
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
pub fn visit_grammar(
    node: &Node<'_>,
    ctx: &SourceFile<'_>,
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
                        "rule '{}' is defined more than once (line {})",
                        prod.name, prod.line
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
            _ => {}
        }
    }
    let warnings = grammar.check();
    Ok((grammar, warnings))
}

/// Collects every named child of `node` into a [`Vec<DirectiveItem>`], recording the
/// directive's 1-based source line for each entry.
fn collect_directive_items(node: &Node<'_>, ctx: &SourceFile<'_>) -> Vec<DirectiveItem> {
    let line = node.start_position().row + 1;
    (0..node.named_child_count() as u32)
        .map(|i| {
            let name = node
                .named_child(i)
                .expect("named child index in bounds")
                .utf8_text(ctx.source.as_bytes())
                .expect("valid UTF-8")
                .to_string();
            DirectiveItem { name, line }
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
            groups.push(ConflictGroup { rules, line });
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
) -> Result<(PrecKind, Option<u32>), ParseError> {
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
            text.parse::<u32>()
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
    let name = visit(grammar, &name_child, ctx)?;
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
