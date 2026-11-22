# nvim Plugin

The nvim plugin integrates lun with Neovim.

## Installation

### Manual
1. Clone plugin to `~/.config/nvim/plugins/lun`
2. Add to `~/.config/nvim/lua/plugins/lun.lua`:

```lua
require('lun')
```

### Lazy.nvim
```lua
{
  'ColtonKawamura/lun-nvim',
  config = function()
    require('lun').setup()
  end
}
```

## Commands

### :Lun
Launch the TUI from nvim.

```sh
:Lun
```

## Keybindings

### ⌘⇧L
Open links associated with current task.

```lua
vim.keymap.set('n', '<D-S-L>', function()
  require('lun').open_links()
end)
```

## Configuration

### Default Bindings
```lua
require('lun').setup({
  open_links = '<D-S-L>',
})
```

### Custom Paths
```lua
require('lun').setup({
  db_path = '~/.lun/lun.db',
})
```

## Features

### TUI Integration
- Launch TUI from nvim
- Return to nvim when done
- Preserves terminal state

### Link Opening
- `⌘⇧L` opens task links
- Uses macOS `open`
- Works in TUI and normal mode

## Tips

- Use `:Lun` for quick task management
- `⌘⇧L` opens links in browser
- TUI works with nvim terminal
- Commands sync between TUI and CLI
