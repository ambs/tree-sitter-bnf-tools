# BNF → tree-sitter cheat sheet

| BNF | tree-sitter JS | Notes |
|-----|---------------|-------|
| `name -> body ;` | `name: $ => body` | Rule definition |
| `a b c` | `seq(a, b, c)` | Sequence |
| `a \| b \| c` | `choice(a, b, c)` | Alternatives |
| `x*` | `repeat(x)` | Zero or more |
| `x+` | `repeat1(x)` | One or more |
| `x?` | `optional(x)` | Zero or one |
| `(body)` | inline group | No new rule created |
| `'text'` | `'text'` | Literal string |
| `/regex/` | `/regex/` | Pattern |
| `/regex/i` | `/regex/i` | Pattern with JS regex flags |
| `<< body >>` | `token(body)` | Atomic lexer token |
| `<<! body >>` | `token.immediate(body)` | Immediate token (no leading whitespace) |
| `label: sym` | `field('label', sym)` | Named AST field |
| `(body => name)` | `alias(body, $.name)` | Named alias |
| `(body => 'str')` | `alias(body, 'str')` | Anonymous alias |
| `body %prec N` | `prec(N, body)` | Precedence |
| `body %prec.left N` | `prec.left(N, body)` | Left-associative precedence |
| `body %prec.right N` | `prec.right(N, body)` | Right-associative precedence |
| `body %prec.dynamic N` | `prec.dynamic(N, body)` | Dynamic precedence |
| `%axiom r` | *(emits `r` first in `rules:`)* | Explicit start rule |
| `%conflicts [r1, r2]` | `conflicts: $ => [[$.r1, $.r2]]` | Conflict declaration |
| `%inline r` | `inline: $ => [$.r]` | Inline rule |
| `%supertypes r` | `supertypes: $ => [$.r]` | Supertype declaration |
| `%extras /x/, r` | `extras: $ => [/x/, $.r]` | Extra tokens |
| `%include "f.bnf"` | *(merges the file's rules and directives)* | File inclusion |
| `# comment` | *(removed)* | Line comment |

---

Previous: [Grammar-level directives](03-directives.md) · Next: [End-to-end workflow](05-end-to-end.md)
