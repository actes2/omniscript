# OmniScript Vim / Neovim Plugin (`vim-omniscript`)

Filetype detection, polyglot syntax highlighting, and brace+colon indentation for `.omni` files.

## Installation

### Native Vim / Neovim Packages (Zero Plugin Manager)
```bash
# Vim
mkdir -p ~/.vim/pack/plugins/start
ln -s "$(pwd)/editors/vim-omniscript" ~/.vim/pack/plugins/start/vim-omniscript

# Neovim
mkdir -p ~/.local/share/nvim/site/pack/plugins/start
ln -s "$(pwd)/editors/vim-omniscript" ~/.local/share/nvim/site/pack/plugins/start/vim-omniscript
```

### `lazy.nvim` (Neovim)
```lua
{ dir = "/path/to/omniscript/editors/vim-omniscript", ft = "omni" }
```

### `vim-plug`
```vim
Plug '/path/to/omniscript/editors/vim-omniscript'
```
