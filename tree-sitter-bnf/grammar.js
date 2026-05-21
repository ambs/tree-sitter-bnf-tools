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
    symbol: $ => seq(optional(field('label', $.fieldLabel)), field('content', choice($.nonTerminal, $._terminal, $.subSeq, $.tokenExpr)), optional(field('kleene', $._kleeneOp))),
    fieldLabel: $ => /[A-Za-z_][A-Za-z0-9_]*:/,
    _kleeneOp: $ => choice($.plus, $.asterisk, $.questionMark),
    plus: $ => '+',
    asterisk: $ => '*',
    questionMark: $ => '?',
    subSeq: $ => seq('(', field('body', $.ruleBody), ')'),
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
