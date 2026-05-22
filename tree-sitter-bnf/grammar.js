/**
 * @file A simple BNF syntax parser
 * @author Alberto Simões <ambs@cpan.org>
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: "bnf",

  extras: $ => [/\s/, $.comment],

  conflicts: $ => [[$.symbolSeq, $.symbolSeqInner]],

  rules: {
    grammar: $ => repeat1(choice($.rule, $.conflictsDirective)),
    conflictsDirective: $ => seq('%conflicts', $.conflictGroup, repeat(seq(',', $.conflictGroup))),
    conflictGroup: $ => seq('[', $.nonTerminal, repeat(seq(',', $.nonTerminal)), ']'),
    rule: $ => seq(field('name', $.nonTerminal), '->', field('body', $.ruleBody), ';'),
    ruleBody: $ => seq($.symbolSeq, repeat(seq('|', $.symbolSeq))),
    symbolSeq: $ => seq(repeat1($.symbol), optional(field('prec', $.precAnnotation))),
    ruleBodyInner: $ => seq($.symbolSeqInner, repeat(seq('|', $.symbolSeqInner))),
    symbolSeqInner: $ => repeat1($.symbol),
    symbol: $ => seq(optional(field('label', $.fieldLabel)), field('content', $._symbolContent), optional(field('kleene', $._kleeneOp))),
    _symbolContent: $ => choice($.nonTerminal, $._terminal, $.subSeq, $.aliasGroup, $.tokenExpr, $.precGroup),
    fieldLabel: $ => /[A-Za-z_][A-Za-z0-9_]*:/,
    _kleeneOp: $ => choice($.plus, $.asterisk, $.questionMark),
    plus: $ => '+',
    asterisk: $ => '*',
    questionMark: $ => '?',
    subSeq: $ => seq('(', field('body', $.ruleBodyInner), ')'),
    aliasGroup: $ => seq('(', field('body', $.ruleBody), '=>', field('alias', $.aliasName), ')'),
    aliasName: $ => choice($.nonTerminal, $.literal),
    tokenExpr: $ => seq('<<', field('body', $.ruleBody), '>>'),
    precGroup: $ => seq('(', field('body', $.ruleBodyInner), field('annotation', $.precAnnotation), ')'),
    precAnnotation: $ => seq('%', field('kind', $.precKind), optional(field('level', $.integer))),
    precKind: $ => choice('prec.dynamic', 'prec.left', 'prec.right', 'prec'),
    integer: $ => /[0-9]+/,
    _terminal: $ => choice($.pattern, $.literal),
    pattern: $ => /\/([^/\[\\]|\[[^\]]*\]|\\.)+\//,
    literal: $ => token(choice(
      /'([^'\\]|\\.)*'/,
      /"([^"\\]|\\.)*"/,
    )),
    nonTerminal: $ => /[A-Za-z_][A-Za-z0-9_]*/,
    comment: $ => token(seq('#', /.*/)),
  }
});
