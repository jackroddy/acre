-- The plugin with the real viewer: acre-gui --stdio, which opens a
-- window, so this needs a display. It checks that the text reaches the
-- viewer and that its answer shows as a diagnostic or clears one.
--
--   cargo build --release -p acre-gui
--   nvim --clean --headless -l editors/nvim/tests/viewer.lua

local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
local root = vim.fn.fnamemodify(here, ":h:h:h")
vim.opt.runtimepath:prepend(vim.fn.fnamemodify(here, ":h"))

local gui = os.getenv("ACRE_GUI") or (root .. "/target/release/acre-gui")
local acre = require("acre")
acre.setup({ cmd = { gui, "--stdio" }, debounce = 10 })

local failures = 0
local function check(name, ok, detail)
  if ok then
    print("ok    " .. name)
  else
    failures = failures + 1
    print("FAIL  " .. name .. (detail and (": " .. detail) or ""))
  end
end

local function errors()
  return vim.diagnostic.get(0, { severity = vim.diagnostic.severity.ERROR })
end

local broken = { "canvas {", "  size 200 100;", "  box { at 10; }", "}" }
local fixed = { "canvas {", "  size 200 100;", "  box { at 10 20; size 30 30; }", "}" }

vim.cmd("edit " .. vim.fn.fnameescape(vim.fn.tempname() .. ".acre"))
vim.api.nvim_buf_set_lines(0, 0, -1, false, broken)
acre.start()

vim.wait(10000, function()
  return #errors() > 0
end, 20)
local found = errors()
check(
  "a broken file shows an error where it is",
  #found == 1 and found[1].lnum == 2 and found[1].col == 8 and found[1].message:find("`at`", 1, true) ~= nil,
  vim.inspect(found)
)

-- the plugin sends on TextChanged, which a headless nvim with no input
-- never fires, so the test sends the text the way a write does
vim.api.nvim_buf_set_lines(0, 0, -1, false, fixed)
vim.api.nvim_exec_autocmds("BufWritePost", { buffer = 0 })
vim.wait(10000, function()
  return #errors() == 0
end, 20)
check("fixing it clears the error", #errors() == 0, vim.inspect(errors()))

vim.api.nvim_buf_set_lines(0, 0, -1, false, broken)
vim.api.nvim_exec_autocmds("BufWritePost", { buffer = 0 })
vim.wait(10000, function()
  return #errors() > 0
end, 20)
check("breaking it again brings the error back", #errors() == 1, vim.inspect(errors()))

acre.stop()
print(failures == 0 and "all passed" or (failures .. " failed"))
vim.cmd(failures == 0 and "qa!" or "cq!")
