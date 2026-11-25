-- The plugin's side of the link, with `sh -c 'cat > FILE'` standing in
-- for the viewer: what the plugin sends lands in FILE, and the test
-- hands the plugin the viewer's messages itself. The stand-in does not
-- read the text, so it need not be acre.
--
--   nvim --clean --headless -l editors/nvim/tests/edit.lua

local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
vim.opt.runtimepath:prepend(vim.fn.fnamemodify(here, ":h"))

local sent_file = vim.fn.tempname()
local acre = require("acre")
acre.setup({ cmd = { "sh", "-c", "cat > " .. vim.fn.shellescape(sent_file) }, debounce = 10 })

local failures = 0
local function check(name, ok, detail)
  if ok then
    print("ok    " .. name)
  else
    failures = failures + 1
    print("FAIL  " .. name .. (detail and (": " .. detail) or ""))
  end
end

-- the text messages sent so far
local function texts()
  local out = {}
  if vim.fn.filereadable(sent_file) == 1 then
    for _, line in ipairs(vim.fn.readfile(sent_file)) do
      local message = vim.json.decode(line)
      if message.type == "text" then
        table.insert(out, message)
      end
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

local function lines()
  return vim.api.nvim_buf_get_lines(0, 0, -1, false)
end

local original = {
  "canvas { size 200 100 }",
  "box { at 10 20 size 30 30 }",
  "circle { at 50 50 r 5 }",
}
vim.api.nvim_buf_set_lines(0, 0, -1, false, original)
local buf = vim.api.nvim_get_current_buf()
acre.start()

local sent = wait_for_texts(1)
local tick = vim.api.nvim_buf_get_changedtick(buf)
check("text carries the buffer's changedtick", sent[1] and sent[1].tick == tick, vim.inspect(sent[1]))
check("text is the lines joined with newlines", sent[1] and sent[1].text == table.concat(original, "\n") .. "\n")

-- "box { at 10 20" starts at byte 24; "10 20" is bytes 33..38.
-- "circle { at 50 50" starts at byte 52; "50 50" is bytes 64..69.
local text = sent[1].text
check("fixture offsets", text:sub(34, 38) == "10 20" and text:sub(65, 69) == "50 50")

acre._receive({
  type = "edit",
  tick = tick,
  edits = {
    { start = 33, ["end"] = 38, text = "15 25" },
    { start = 64, ["end"] = 69, text = "60 40" },
  },
})
check("an edit of the current tick changes the buffer", vim.deep_equal(lines(), {
  "canvas { size 200 100 }",
  "box { at 15 25 size 30 30 }",
  "circle { at 60 40 r 5 }",
}), vim.inspect(lines()))
sent = wait_for_texts(2)
check(
  "the text goes back with its new tick",
  sent[2] and sent[2].tick == vim.api.nvim_buf_get_changedtick(buf) and sent[2].text:find("at 60 40", 1, true) ~= nil,
  vim.inspect(sent[2])
)

vim.cmd("undo")
check("one undo takes back the whole edit", vim.deep_equal(lines(), original), vim.inspect(lines()))

-- an insertion with a newline, one at the end of the buffer, and an
-- edit given out of order
tick = vim.api.nvim_buf_get_changedtick(buf)
acre._receive({
  type = "edit",
  tick = tick,
  edits = {
    { start = #text, ["end"] = #text, text = "dot { at 1 1 }\n" },
    { start = 24, ["end"] = 24, text = "group {\n}\n" },
  },
})
check("insertions, one past the last line", vim.deep_equal(lines(), {
  "canvas { size 200 100 }",
  "group {",
  "}",
  "box { at 10 20 size 30 30 }",
  "circle { at 50 50 r 5 }",
  "dot { at 1 1 }",
}), vim.inspect(lines()))
vim.cmd("undo")
check("one undo takes back the insertions", vim.deep_equal(lines(), original), vim.inspect(lines()))

-- the last line and the newline after it replaced
tick = vim.api.nvim_buf_get_changedtick(buf)
acre._receive({
  type = "edit",
  tick = tick,
  edits = { { start = 52, ["end"] = #text, text = "dot {}\n" } },
})
check("a range to the end of the text", vim.deep_equal(lines(), {
  "canvas { size 200 100 }",
  "box { at 10 20 size 30 30 }",
  "dot {}",
}), vim.inspect(lines()))
vim.cmd("undo")

-- let any text still on its way arrive
vim.wait(100)
local before = #texts()
acre._receive({
  type = "edit",
  tick = vim.api.nvim_buf_get_changedtick(buf) - 1,
  edits = { { start = 0, ["end"] = 6, text = "nope" } },
})
check("an edit of an older tick is dropped", vim.deep_equal(lines(), original), vim.inspect(lines()))
sent = wait_for_texts(before + 1)
check(
  "and the text is sent again",
  #sent > before and sent[#sent].tick == vim.api.nvim_buf_get_changedtick(buf),
  vim.inspect(sent[#sent])
)

-- :AcreView in another buffer moves the running viewer to it. A new
-- stand-in would truncate the file, so the count only grows if the
-- same viewer gets the second buffer.
vim.wait(100)
before = #texts()
vim.cmd("enew")
local second = vim.api.nvim_get_current_buf()
vim.api.nvim_buf_set_name(second, "second.acre")
vim.api.nvim_buf_set_lines(second, 0, -1, false, { "canvas { size 10 10 }" })
acre.toggle()
sent = wait_for_texts(before + 1)
check(
  "toggle in another buffer sends that buffer to the same viewer",
  #sent > before
    and sent[#sent].text == "canvas { size 10 10 }\n"
    and sent[#sent].path:match("second%.acre$") ~= nil,
  vim.inspect(sent[#sent])
)
local function hooks(b)
  return #vim.api.nvim_get_autocmds({ group = "acre_view", buffer = b })
end
check("the first buffer's autocmds are gone", hooks(buf) == 0, tostring(hooks(buf)))
check("the second buffer has them", hooks(second) > 0, tostring(hooks(second)))
acre.toggle()
check("toggle in the shown buffer closes the viewer", hooks(second) == 0)

-- a viewer that exits: a failure is reported with its stderr, a clean exit is silent
acre.stop()
local notes = {}
local real_notify = vim.notify
vim.notify = function(message, level)
  table.insert(notes, { message = message, level = level })
end
local function run_viewer(script)
  notes = {}
  acre.setup({ cmd = { "sh", "-c", script }, debounce = 10 })
  acre.start()
  vim.wait(2000, function()
    return #notes > 0
  end)
  vim.wait(200)
end
run_viewer("echo boom >&2; exit 3")
check(
  "a failing viewer is reported with its stderr",
  #notes == 1
    and notes[1].level == vim.log.levels.ERROR
    and notes[1].message:find("acre: the viewer exited with status 3", 1, true) ~= nil
    and notes[1].message:find("boom", 1, true) ~= nil,
  vim.inspect(notes)
)
run_viewer("exit 0")
check("a viewer that exits cleanly reports nothing", #notes == 0, vim.inspect(notes))
vim.notify = real_notify

acre.stop()
vim.fn.delete(sent_file)
print(failures == 0 and "all passed" or (failures .. " failed"))
vim.cmd(failures == 0 and "qa!" or "cq!")
