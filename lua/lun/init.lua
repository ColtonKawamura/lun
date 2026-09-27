-- lun.nvim (Phase 8) — nvim integration for lun.
--
-- Provides:
--   :Lun            Launch the lun TUI for the current directory
--                   (`vim.cmd('terminal lun')` — the alternate-screen TUI
--                   takes over the window; <C-\><C-\> or `q` in the TUI's
--                   own keymap exits back to nvim).
--   <D-L> (⌘⇧L)     Open the markdown link under the cursor via
--                   `lun open-uri <uri>`, which hands the URI to macOS
--                   `open` and reports back in the message line.
--
-- Options (lazy-loaded defaults; set in your config to override):
--   lun.bin   -- path to the lun binary (default: "lun" on PATH)
--
-- Install: add this repo to your package manager's path (the `lua/`
-- directory is the plugin root) and let it load — or simply
-- `vim.opt.rtp:prepend("<path to this repo>")`.

local M = {}

local function bin()
  return (vim.g.lun_bin or "lun")
end

local function has_bin()
  return vim.fn.executable(bin()) == 1
end

-- Exposed (read-only intent) so configs/tests can resolve the binary and
-- check availability.
M.bin = bin
M.has_bin = has_bin

--- Find the markdown-style link under the cursor in the current buffer
--- line: `[label](uri)`.
---
--- Scans every markdown link on the line and returns the uri of the
--- first one whose span covers the cursor byte column (anywhere over the
--- label, the paren, or the uri counts — per the plan's "anywhere over
--- the link" behavior). `uri` is everything up to the first `)` after
--- the opening `(` — lun records link uris verbatim (Phase 4), and the
--- spec's example contains `?`, `&`, `%` but no `)`. Returns the uri, or
--- nil when the cursor is not on a link.
function M.uri_under_cursor()
  local line = vim.api.nvim_get_current_line()
  local col = vim.api.nvim_win_get_cursor(0)[2] -- 0-based byte column
  local p = 1
  while true do
    local s, e, label, uri = line:find("%[(.-)%]%((.-)%)", p)
    if not s then
      return nil
    end
    -- Lua strings are 1-based; the cursor column is a 0-based cell.
    -- The link occupies cells s-1 .. e-1, so the cursor is on the link
    -- when its cell overlaps that range (a cursor parked in the gap
    -- right after the closing ')' is NOT on the link).
    if col >= s - 1 and col < e then
      return uri
    end
    p = e + 1
  end
end

--- Open the link under the cursor (the ⌘⇧L handler).
function M.open_link_under_cursor()
  local uri = M.uri_under_cursor()
  if not uri then
    vim.notify("lun: no markdown link [label](uri) under the cursor", vim.log.levels.WARN)
    return
  end
  if not has_bin() then
    vim.notify("lun: binary not found (set lun.bin to its path)", vim.log.levels.ERROR)
    return
  end
  -- Run synchronously so the result lands in the message line; `open`
  -- returns immediately after launching the target.
  local ok, out = pcall(vim.fn.system, { bin(), "open-uri", uri })
  if not ok then
    vim.notify("lun: failed to run `" .. bin() .. " open-uri`", vim.log.levels.ERROR)
    return
  end
  -- system() returns {exit, signal, output} for a list arg in nvim >= 0.10.
  local exit, output
  if type(out) == "table" then
    exit, output = out[1], out[3]
  else
    exit, output = 0, out
  end
  output = (output or ""):gsub("\n%s*$", "")
  if exit == 0 then
    vim.notify("lun: " .. output, vim.log.levels.INFO)
  else
    vim.notify("lun: " .. output, vim.log.levels.ERROR)
  end
end

--- Launch the lun TUI (the :Lun handler).
function M.launch(args)
  if not has_bin() then
    vim.notify("lun: binary not found (set lun.bin to its path)", vim.log.levels.ERROR)
    return
  end
  local cmd = bin()
  if args and args ~= "" then
    cmd = cmd .. " " .. args
  end
  vim.cmd("terminal " .. cmd)
end

local function setup_mappings()
  -- Mac-only key: ⌘⇧L arrives at nvim as <D-L>.
  if vim.fn.has("mac") == 1 then
    vim.api.nvim_set_keymap("n", "<D-L>", "<Cmd>lua require'lun'.open_link_under_cursor()<CR>", {
      noremap = true,
      silent = true,
      desc = "lun: open the link under the cursor (⌘⇧L)",
    })
  end
end

local function setup_commands()
  vim.api.nvim_create_user_command("Lun", function(cmd)
    M.launch(cmd.args)
  end, {
    nargs = "?",
    desc = "Launch the lun TUI for the current directory",
  })
end

--- Idempotent setup (safe to call more than once).
function M.setup()
  setup_commands()
  setup_mappings()
end

M.setup()
return M
