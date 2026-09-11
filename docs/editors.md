---
title: Editor Setup
nav_order: 13
---

# Editor Setup

This guide covers how to get syntax highlighting, indentation, and code folding
for `.bnf` files in Neovim, Helix, Emacs, and VS Code.

---

## Neovim (nvim-treesitter)

This section targets the current ("main" branch) nvim-treesitter, which
registers parsers inside a `User TSUpdate` autocmd rather than through the
old `get_parser_configs()` table. If you're on nvim-treesitter's legacy
"master" branch, adapt accordingly or use the [plugin-free
alternative](#neovim-plugin-free-alternative) below instead.

### 1 — Register the parser

Add the following to your Neovim config (e.g. `init.lua`) **before** the
`nvim-treesitter` setup call:

```lua
vim.api.nvim_create_autocmd('User', { pattern = 'TSUpdate',
  callback = function()
    require('nvim-treesitter.parsers').bnf = {
      install_info = {
        url = 'https://github.com/ambs/tree-sitter-bnf-tools',
        location = 'tree-sitter-bnf', -- repo is a monorepo; parser lives in this subdir
        queries = 'queries',          -- symlinks queries/ automatically, see step 4
      },
    }
  end })
```

For a local checkout instead of cloning from GitHub, replace `url` /
`location` with `path`:

```lua
      install_info = {
        path = '<path-to-repo>/tree-sitter-bnf',
        queries = 'queries',
      },
```

### 2 — Install the parser

Inside Neovim, run:

```
:TSInstall bnf
```

### 3 — Register the filetype

Neovim does not associate `.bnf` files with the `bnf` filetype automatically.
Add this to your config:

```lua
vim.filetype.add({ extension = { bnf = "bnf" } })
```

### 4 — Enable highlighting

`install_info.queries` from step 1 already symlinks the query directory for
you, so no manual copying is needed. The current nvim-treesitter does not
start the highlighter automatically — you must call `vim.treesitter.start()`
yourself, e.g.:

```lua
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'bnf',
  callback = function() vim.treesitter.start() end,
})
```

Without this step, the parser and queries are installed but nothing will
visibly highlight.

### 5 — Enable folding (optional)

To use tree-sitter-based folding, add this to your config or a
`ftplugin/bnf.lua` file:

```lua
vim.opt_local.foldmethod = "expr"
vim.opt_local.foldexpr   = "nvim_treesitter#foldexpr()"
vim.opt_local.foldenable  = false   -- open all folds by default
```

---

## Neovim (plugin-free alternative)

Neovim's built-in `vim.treesitter` doesn't require the nvim-treesitter
plugin at all — it can load a compiled parser and query files directly,
which avoids the nvim-treesitter API/branch concerns above entirely.

### 1 — Build the parser

```sh
cd tree-sitter-bnf
tree-sitter build -o bnf.so
```

### 2 — Install the parser and queries

Drop the result into any directory on Neovim's `&runtimepath`, e.g.
`~/.config/nvim`:

```sh
RUNTIME_DEST="$HOME/.config/nvim"

mkdir -p "$RUNTIME_DEST/parser" "$RUNTIME_DEST/queries/bnf"
cp bnf.so "$RUNTIME_DEST/parser/"
cp queries/*.scm "$RUNTIME_DEST/queries/bnf/"
```

### 3 — Register the filetype and enable highlighting

Same as steps 3 and 4 above:

```lua
vim.filetype.add({ extension = { bnf = "bnf" } })

vim.api.nvim_create_autocmd('FileType', {
  pattern = 'bnf',
  callback = function() vim.treesitter.start() end,
})
```

---

## Helix

### 1 — Build the parser

Clone the repository and compile the parser:

```sh
git clone https://github.com/ambs/tree-sitter-bnf-tools
cd tree-sitter-bnf-tools/tree-sitter-bnf
tree-sitter generate   # only needed if grammar.js changed
gcc -shared -o bnf.so -fPIC src/parser.c
```

### 2 — Install the parser

Place the compiled shared library where Helix expects it:

```sh
mkdir -p ~/.config/helix/runtime/grammars
cp bnf.so ~/.config/helix/runtime/grammars/
```

### 3 — Install the query files

```sh
QUERIES_DEST="$HOME/.config/helix/runtime/queries/bnf"
QUERIES_SRC="<path-to-repo>/tree-sitter-bnf/queries"

mkdir -p "$QUERIES_DEST"
cp "$QUERIES_SRC/highlights.scm" "$QUERIES_DEST/"
cp "$QUERIES_SRC/indents.scm"    "$QUERIES_DEST/"
```

### 4 — Register the language

Add the following to `~/.config/helix/languages.toml`:

```toml
[[language]]
name        = "bnf"
scope       = "source.bnf"
file-types  = ["bnf"]
roots       = []
comment-token = "#"

[[grammar]]
name   = "bnf"
source = { path = "<path-to-repo>/tree-sitter-bnf" }
```

Open a `.bnf` file and run `:lang-support` to confirm the language is active.

---

## Emacs (treesit)

This targets Emacs 29+, which has `treesit` built in. `treesit-install-language-grammar`
cannot be used here to clone-and-compile the grammar automatically: this repo's
generated parser sources (`tree-sitter-bnf/src/`) are gitignored, so a fresh
clone has no `src/parser.c` until `tree-sitter generate` creates it — build
the grammar by hand instead.

### 1 — Build the parser

```sh
git clone https://github.com/ambs/tree-sitter-bnf-tools
cd tree-sitter-bnf-tools/tree-sitter-bnf
tree-sitter generate
```

### 2 — Install the parser

Emacs loads compiled grammars from `~/.emacs.d/tree-sitter/`, and expects the
filename to match `libtree-sitter-<language>` exactly:

```sh
mkdir -p ~/.emacs.d/tree-sitter

# Linux
gcc -shared -fPIC -o ~/.emacs.d/tree-sitter/libtree-sitter-bnf.so \
    -I./src src/parser.c

# macOS
gcc -shared -fPIC -o ~/.emacs.d/tree-sitter/libtree-sitter-bnf.dylib \
    -I./src src/parser.c
```

Verify Emacs can load it:
```
M-: (treesit-language-available-p 'bnf)
```
Should return `t`. If it returns `nil`, the `.so`/`.dylib` file is missing or
misnamed.

### 3 — Install the major mode

This repository ships a ready-made major mode at
[`editors/emacs/bnf-ts-mode.el`](https://github.com/ambs/tree-sitter-bnf-tools/blob/main/editors/emacs/bnf-ts-mode.el) —
copy it somewhere on your `load-path` and require it:

```sh
mkdir -p ~/.emacs.d/lisp
cp editors/emacs/bnf-ts-mode.el ~/.emacs.d/lisp/
```

```elisp
(add-to-list 'load-path "~/.emacs.d/lisp")
(require 'bnf-ts-mode)
```

`bnf-ts-mode` provides:
- Syntax highlighting, translated from `tree-sitter-bnf/queries/highlights.scm`
  into Emacs font-lock faces
- Structural navigation (`C-M-a` / `C-M-e` jump between rule definitions)
- Imenu / `consult-imenu` integration — all rule names as jumpable entries
- Indentation — `TAB` on a `|` or `;` line aligns it under the `>` of the
  enclosing `->`
- `.bnf` files are associated with the mode automatically

### 4 — Try it

Open (or create) a `.bnf` file — the mode name in the modeline should show
`BNF`. Run `M-x treesit-explore-mode` to see the live syntax tree alongside
your file, useful if you want to extend the mode's font-lock rules.

### Updating the grammar

When the grammar changes upstream, regenerate and recompile:

```sh
cd tree-sitter-bnf-tools/tree-sitter-bnf
git pull
tree-sitter generate
gcc -shared -fPIC -o ~/.emacs.d/tree-sitter/libtree-sitter-bnf.so \
    -I./src src/parser.c
```

Then restart Emacs (or run `M-x treesit-parser-delete` on the current buffer
and reopen the file).

---

## VS Code

VS Code has no built-in tree-sitter support, so highlighting comes from a
TextMate grammar instead. This repository ships one — a minimal local
extension at
[`editors/vscode/`](https://github.com/ambs/tree-sitter-bnf-tools/tree/main/editors/vscode) —
approximating the BNF dialect: rule names, `->`/`=>`, literals, patterns,
directives, and comments. It doesn't provide indentation or folding (those
need a real tree-sitter-backed extension — out of scope here).

### 1 — Get the extension folder

```sh
git clone https://github.com/ambs/tree-sitter-bnf-tools
```

The extension lives entirely in `tree-sitter-bnf-tools/editors/vscode/`.

### 2 — Install it as an unpacked extension

Copy (or symlink) that folder into VS Code's extensions directory:

```sh
# Linux / macOS
mkdir -p ~/.vscode/extensions
cp -r tree-sitter-bnf-tools/editors/vscode ~/.vscode/extensions/bnf-syntax

# Windows (PowerShell)
Copy-Item -Recurse tree-sitter-bnf-tools\editors\vscode "$env:USERPROFILE\.vscode\extensions\bnf-syntax"
```

### 3 — Reload VS Code

Reload the window (`Developer: Reload Window` from the Command Palette, or
just restart VS Code). Open a `.bnf` file — the language mode in the bottom
status bar should read `BNF`, and highlighting should be active.

### Trying it without installing

To try the grammar without copying anything into your extensions folder,
launch VS Code in Extension Development Host mode against the folder
directly:

```sh
code --extensionDevelopmentPath=tree-sitter-bnf-tools/editors/vscode <some-file>.bnf
```

### Updating the grammar

The TextMate grammar (`editors/vscode/syntaxes/bnf.tmLanguage.json`) is
maintained by hand and only approximates the dialect — it does not track
`tree-sitter-bnf/grammar.js` automatically. If the grammar changes in ways
that affect highlighting (new directives, changed terminal syntax), update
it manually and re-copy the folder, or `git pull` if you installed via
symlink.
