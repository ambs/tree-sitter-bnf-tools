# Formatting and refactoring

## Canonical formatting

`ts-bnf-tool format` re-emits a grammar in a canonical style: consistent
spacing, one alternative per line when a rule exceeds 80 characters, and
directives sorted to the top. Use it to keep grammar files uniform across
contributors.

```sh
ts-bnf-tool format grammar.bnf          # print formatted grammar to stdout
ts-bnf-tool format -i grammar.bnf       # rewrite in place (atomic)
ts-bnf-tool format -o clean.bnf grammar.bnf  # write to a different file
```

Pass `--check` instead of reformatting to verify that a file is already
formatted — useful in CI:

```sh
ts-bnf-tool format --check grammar.bnf
echo $?   # 0 if already formatted, 1 otherwise
```

### Merging `%include` files into a single file

`format` inlines all `%include` directives and emits the fully-merged grammar.
Use this to collapse a multi-file grammar into one canonical `.bnf` file:

```sh
ts-bnf-tool format main.bnf > merged.bnf
```

The `%include` directives do not appear in the output — only the combined rules
and directives from all files are emitted.

### `format` options

```
  -i, --in-place         Rewrite the file in place
  --check                Exit non-zero if the file is not already formatted
  --strip-comments       Strip # comments from output (default)
  --no-strip-comments    Preserve # comments in output
```

## Renaming a rule

`ts-bnf-tool rename` performs a safe, mechanical rename of one rule throughout
the entire grammar — its definition, every reference in rule bodies, and every
mention in `%axiom`, `%inline`, `%supertypes`, `%extras`, and `%conflicts`
directives — in a single pass. The result is re-emitted in canonical format.

```sh
ts-bnf-tool rename grammar.bnf expr expression        # print to stdout
ts-bnf-tool rename -i grammar.bnf expr expression     # rewrite in place (atomic)
ts-bnf-tool rename grammar.bnf expr expression -o out.bnf
```

For example, given:

```bnf
%inline expr
expr   -> term ('+' term)* ;
term   -> factor ('*' factor)* ;
factor -> /[0-9]+/ | '(' expr ')' ;
```

Running `ts-bnf-tool rename -i grammar.bnf term node` produces:

```bnf
%inline expr
expr   -> node ('+' node)* ;
node   -> factor ('*' factor)* ;
factor -> /[0-9]+/ | '(' expr ')' ;
```

`rename` exits non-zero if the source rule does not exist or the target name is
already taken, so it is safe to use in scripts.

### `rename` options

```
  -i, --in-place    Rewrite the file in place (cannot be used with stdin)
  -o <FILE>         Write output to this file instead of stdout
```

---

Previous: [Analysing a grammar](06-analysing.md) · Next: [Visualising a grammar](08-visualising.md)
