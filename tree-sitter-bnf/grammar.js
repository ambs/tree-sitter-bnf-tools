/**
 * @file A simple BNF syntax parser
 * @author Alberto Simões <ambs@cpan.org>
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: "bnf",

  rules: {
    grammar: $ => repeat1($.rule),
    rule: $ => seq($.nonTerminal, '->', $.ruleBody, ';'),
    ruleBody: $ => seq($.symbolSeq, repeat(seq('|', $.symbolSeq))),
    symbolSeq: $ => repeat1($.symbol),
    symbol: $ => seq(choice($.nonTerminal, $._terminal, $.subSeq), optional($._kleeneOp)),
    _kleeneOp: $ => choice($.plus, $.asterisk),
    plus: $ => '+',
    asterisk: $ => '*',
    subSeq: $ => seq('(', $.ruleBody, ')'),
    _terminal: $ => choice($.pattern, $.literal),
    pattern: $ => /\/([^/]|\[[^]]+\]|\\\/)+\//,
    literal: $ => /'([^']|\\')+'/,
    nonTerminal: $ => /[A-Za-z_][A-Za-z0-9_]*/,
  }
});
