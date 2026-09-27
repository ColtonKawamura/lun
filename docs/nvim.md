# lun.nvim (Phase 8)

A small single-file nvim plugin that connects nvim to lun:

- `:Lun` — launches the lun TUI for the current directory
  (`terminal lun`; the alternate-screen TUI takes over the window — quit
  the TUI with its own `q` to come back).
- `⌘⇧L` (`<D-L>`, Mac-only) — opens the markdown link under the cursor:
  the plugin extracts the uri from a `[label](uri)` link on the current
  line and hands it to `lun open-uri <uri>`, which launches macOS `open`
  and reports the result in nvim's message line. Anywhere over the link
  counts (label, paren, or uri).

## Install

Add this repo to nvim's runtimepath (the plugin root is `lua/`):

```vim
" lazy.nvim
{ "ColtonKawamura/lun", config = true }   -- auto-loads lua/lun/init.lua

" plain init
vim.opt.rtp:prepend("/path/to/lun")
```

The plugin self-setups on load (idempotent) — no `:LunSetup` needed.

## Options

| option     | default | meaning                                   |
| ---------- | ------- | ----------------------------------------- |
| `lun.bin`  | `"lun"` | path to the lun binary (must be on PATH or absolute) |

## LINK_OPENED logging (optional)

`lun open-uri <uri> --on <task|project> <key|title>` also writes a
`LINK_OPENED` log entry on the given entity, so "I opened this link"
shows up in `lun log <entity>`. The plugin's default ⌘⇧L flow does not
pass `--on` (nvim has no lun entity in scope by default); if you want the
log, wrap the call:

```lua
vim.keymap.set("n", "<D-L>", function()
  local uri = require("lun").uri_under_cursor()
  if uri then
    vim.fn.system({ "lun", "open-uri", uri, "--on", "task", "T-001" })
  end
end)
```

## Verification (headless)

The link-extraction logic is exercised by the repo's nvim headless checks
(see the Phase 8 PR description / commit notes):

```sh
nvim --headless -u NONE --cmd "set rtp+=/path/to/lun" \
  -c 'lua local u = require("lun").uri_under_cursor() io.write(tostring(u))' \
  -c 'qa!'
```
