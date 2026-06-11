# Getting started

This tutorial walks you through `ts-bnf-tool` from scratch — what it does, why
it exists, and how to use it to write a working tree-sitter grammar.

## The problem

[Tree-sitter](https://tree-sitter.github.io/) grammars are written in JavaScript.
Not configuration — actual JavaScript, calling a DSL of functions: `seq()`,
`choice()`, `repeat()`, `optional()`, `token()`, and so on. Here is a small
fragment describing arithmetic expressions:

```js
module.exports = grammar({
  name: "expr",

  rules: {
    expr: $ => choice(
      seq($.expr, '+', $.expr),
      seq($.expr, '*', $.expr),
      $.number,
      seq('(', $.expr, ')'),
    ),
    number: $ => /[0-9]+/,
  }
});
```

It works. But the syntactic structure of the language you are describing is
buried under layers of JavaScript boilerplate. For a small grammar it is
manageable; for a real language it quickly becomes hard to read at a glance.

## The solution

`ts-bnf-tool` lets you write the same grammar in a compact BNF dialect and
generates the `grammar.js` for you:

```bnf
expr   -> expr '+' expr
        | expr '*' expr
        | number
        | '(' expr ')'
        ;
number -> /[0-9]+/ ;
```

The language is what you see. The structure is immediately apparent.

## Installing

Install from crates.io:

```sh
cargo install ts-bnf-tool
```

Or build from source:

```sh
make build
# binary is at target/release/ts-bnf-tool after: make release
```

Every invocation follows the same shape:

```sh
ts-bnf-tool <SUBCOMMAND> [OPTIONS] <file.bnf>
```

Pass `-` as the filename to read from stdin.

## A complete first example

Create a file called `expr.bnf`:

```bnf
# arithmetic expressions
expr -> term ('+' term)* ;
term -> /[0-9]+/ | '(' expr ')' ;
```

Run the tool:

```sh
ts-bnf-tool convert expr.bnf
```

Output:

```js
module.exports = grammar({
  name: "expr",

  rules: {
    expr: $ => seq($.term, repeat(seq('+', $.term))),
    term: $ => choice(/[0-9]+/, seq('(', $.expr, ')')),
  }
});
```

That is a ready-to-use `grammar.js`. Every BNF construct maps to exactly one
tree-sitter DSL call — there is no hidden magic.

---

Next: [Syntax walkthrough](02-syntax.md)
