# Editor Setup

This guide covers how to get syntax highlighting, indentation, and code folding
for `.bnf` files in Neovim and Helix.

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
