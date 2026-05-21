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

  rules: {
    grammar: $ => repeat1($.rule),
    rule: $ => seq(field('name', $.nonTerminal), '->', field('body', $.ruleBody), ';'),
    ruleBody: $ => seq($.symbolSeq, repeat(seq('|', $.symbolSeq))),
    symbolSeq: $ => repeat1($.symbol),
    symbol: $ => seq(optional(field('label', $.fieldLabel)), field('content', $._symbolContent), optional(field('kleene', $._kleeneOp))),
    _symbolContent: $ => choice($.nonTerminal, $._terminal, $.subSeq, $.aliasGroup, $.tokenExpr),
    fieldLabel: $ => /[A-Za-z_][A-Za-z0-9_]*:/,
    _kleeneOp: $ => choice($.plus, $.asterisk, $.questionMark),
    plus: $ => '+',
    asterisk: $ => '*',
    questionMark: $ => '?',
    subSeq: $ => seq('(', field('body', $.ruleBody), ')'),
    aliasGroup: $ => seq('(', field('body', $.ruleBody), '=>', field('alias', $.aliasName), ')'),
    aliasName: $ => choice($.nonTerminal, $.literal),
    tokenExpr: $ => seq('<<', field('body', $.ruleBody), '>>'),
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
