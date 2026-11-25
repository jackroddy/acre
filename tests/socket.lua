-- The plugin linked to a viewer over a socket, with a pipe server in
-- this neovim standing in for the viewer. The test reads what the
-- plugin sends and answers as the viewer would.
--
--   nvim --clean --headless -l editors/nvim/tests/socket.lua

local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
vim.opt.runtimepath:prepend(vim.fn.fnamemodify(here, ":h"))

local acre = require("acre")
-- a viewer started in place of the socket's would fail the test
acre.setup({ cmd = { "false" }, debounce = 10 })

local failures = 0
local function check(name, ok, detail)
  if ok then
    print("ok    " .. name)
  else
    failures = failures + 1
    print("FAIL  " .. name .. (detail and (": " .. detail) or ""))
  end
end

-- the stand-in viewer: one connection at a time, its lines kept
local path = vim.fn.tempname() .. ".sock"
local server = vim.uv.new_pipe(false)
assert(server:bind(path))
local lines_in = {}
local connections = 0
local client = nil
local pending = ""
server:listen(1, function()
  client = vim.uv.new_pipe(false)
  server:accept(client)
  connections = connections + 1
  client:read_start(function(_, data)
    if data then
      pending = pending .. data
      for line in pending:gmatch("([^\n]*)\n") do
        table.insert(lines_in, line)
      end
      pending = pending:match("[^\n]*$")
    elseif client then
      client:close()
      client = nil
    end
  end)
end)

local function texts()
  local out = {}
  for _, line in ipairs(lines_in) do
    local message = vim.json.decode(line)
    if message.type == "text" then
      table.insert(out, message)
    end
  end
  return out
end

local function wait_for_texts(n)
  vim.wait(2000, function()
    return #texts() >= n
  end, 10)
  return texts()
end

local function answer(message)
  client:write(vim.json.encode(message) .. "\n")
end

local function hooks(b)
  return #vim.api.nvim_get_autocmds({ group = "acre_view", buffer = b })
end

-- neovim as the viewer opens it: ACRE_SOCKET set before the plugin
-- loads, and the first acre buffer links
vim.env.ACRE_SOCKET = path
vim.cmd.runtime("plugin/acre.lua")
vim.cmd("edit " .. vim.fn.fnameescape(vim.fn.tempname() .. ".acre"))
local buf = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "canvas { size 200 100 }", "box { at 10 20 }" })
vim.bo.filetype = "acre"

local sent = wait_for_texts(1)
check("the first acre buffer connects to the socket", connections == 1, tostring(connections))
check(
  "and sends its text",
  sent[1] and sent[1].text == "canvas { size 200 100 }\nbox { at 10 20 }\n" and sent[1].path == vim.api.nvim_buf_get_name(buf),
  vim.inspect(sent[1])
)

-- "10 20" is bytes 33..38
answer({
  type = "edit",
  tick = vim.api.nvim_buf_get_changedtick(buf),
  edits = { { start = 33, ["end"] = 38, text = "15 25" } },
})
vim.wait(2000, function()
  return vim.api.nvim_buf_get_lines(buf, 1, 2, false)[1] == "box { at 15 25 }"
end, 10)
check("an edit over the socket changes the buffer", vim.api.nvim_buf_get_lines(buf, 1, 2, false)[1] == "box { at 15 25 }")
sent = wait_for_texts(2)
check(
  "and the text goes back with its new tick",
  sent[2] and sent[2].tick == vim.api.nvim_buf_get_changedtick(buf) and sent[2].text:find("15 25", 1, true) ~= nil,
  vim.inspect(sent[2])
)

answer({ type = "error", offset = 33, message = "`box` needs `size`" })
local function errors()
  return vim.diagnostic.get(buf, { severity = vim.diagnostic.severity.ERROR })
end
vim.wait(2000, function()
  return #errors() > 0
end, 10)
local found = errors()
check("an error over the socket shows as a diagnostic", #found == 1 and found[1].lnum == 1 and found[1].col == 9, vim.inspect(found))

acre.stop()
vim.wait(2000, function()
  return client == nil
end, 10)
check("stopping closes the socket", client == nil)
check("and unhooks the buffer", hooks(buf) == 0)

acre.toggle()
vim.wait(2000, function()
  return connections == 2 and #texts() >= 3
end, 10)
check(":AcreView with ACRE_SOCKET connects again", connections == 2 and #texts() >= 3, tostring(connections))

client:close()
client = nil
vim.wait(2000, function()
  return hooks(buf) == 0
end, 10)
check("a viewer that closes the socket unlinks the buffer", hooks(buf) == 0)

server:close()
vim.fn.delete(path)
print(failures == 0 and "all passed" or (failures .. " failed"))
vim.cmd(failures == 0 and "qa!" or "cq!")
