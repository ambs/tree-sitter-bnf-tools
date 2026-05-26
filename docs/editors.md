# Editor Setup

This guide covers how to get syntax highlighting, indentation, and code folding
for `.bnf` files in Neovim and Helix.

---

## Neovim (nvim-treesitter)

### 1 — Register the parser

Add the following to your Neovim config (e.g. `init.lua`) **before** the
`nvim-treesitter` setup call:

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()

parser_config.bnf = {
  install_info = {
    url = "https://github.com/ambs/tree-sitter-bnf-tools",
    files = { "tree-sitter-bnf/src/parser.c" },
    branch = "main",
  },
  filetype = "bnf",
}
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

### 4 — Install the query files

Copy (or symlink) the query files from the repository into your Neovim runtime:

```sh
QUERIES_SRC="<path-to-repo>/tree-sitter-bnf/queries"
QUERIES_DEST="$HOME/.config/nvim/queries/bnf"

mkdir -p "$QUERIES_DEST"
cp "$QUERIES_SRC/highlights.scm" "$QUERIES_DEST/"
cp "$QUERIES_SRC/indents.scm"    "$QUERIES_DEST/"
cp "$QUERIES_SRC/folds.scm"      "$QUERIES_DEST/"
```

### 5 — Enable folding (optional)

To use tree-sitter-based folding, add this to your config or a
`ftplugin/bnf.lua` file:

```lua
vim.opt_local.foldmethod = "expr"
vim.opt_local.foldexpr   = "nvim_treesitter#foldexpr()"
vim.opt_local.foldenable  = false   -- open all folds by default
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
