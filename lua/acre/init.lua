-- The acre viewer for a buffer: acre-gui started with --stdio, or one
-- already running whose socket is in $ACRE_SOCKET, sent the buffer's
-- text and cursor as JSON lines, and answering with the source's error,
-- with clicks on the render, and with edits to the source.

local M = {}

M.config = {
  -- the viewer and its arguments
  cmd = { "acre-gui", "--stdio" },
  -- "change" renders as you type, "save" on each write
  render = "change",
  -- milliseconds of quiet before a change is sent
  debounce = 150,
}

local ns = vim.api.nvim_create_namespace("acre")
local group = vim.api.nvim_create_augroup("acre_view", { clear = true })

-- the viewer linked, at most one: `chan` is the channel to it, a job
-- when `socket` is false
local view = nil

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})
end

local function send(message)
  if view then
    vim.fn.chansend(view.chan, vim.json.encode(message) .. "\n")
  end
end

local function send_text()
  local buf = view.buf
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  send({
    type = "text",
    text = table.concat(lines, "\n") .. "\n",
    path = vim.api.nvim_buf_get_name(buf),
    tick = vim.api.nvim_buf_get_changedtick(buf),
  })
end

local function send_cursor()
  local win = vim.fn.bufwinid(view.buf)
  if win == -1 or not vim.api.nvim_buf_is_loaded(view.buf) then
    return
  end
  local row, col = unpack(vim.api.nvim_win_get_cursor(win))
  local ok, start = pcall(vim.api.nvim_buf_get_offset, view.buf, row - 1)
  if ok then
    send({ type = "cursor", offset = start + col })
  end
end

-- the row (1-based) and column (0-based, in bytes) of byte `offset`
local function position(buf, offset)
  local rows = vim.api.nvim_buf_line_count(buf)
  local row = 1
  while row < rows and vim.api.nvim_buf_get_offset(buf, row) <= offset do
    row = row + 1
  end
  return row, offset - vim.api.nvim_buf_get_offset(buf, row - 1)
end

-- the row and column, both 0-based, of byte `offset`
local function place(buf, offset)
  local row, col = position(buf, offset)
  return row - 1, col
end

-- make `edits`, each {start, end, text} in bytes of the text sent,
-- as one undo step
local function apply(buf, edits)
  local rows = vim.api.nvim_buf_line_count(buf)
  local last = vim.api.nvim_buf_get_lines(buf, rows - 1, rows, true)[1]
  local final_newline = vim.api.nvim_buf_get_offset(buf, rows - 1) + #last
  local changes = {}
  for _, edit in ipairs(edits) do
    local start, finish, text = edit.start, edit["end"], edit.text
    -- no buffer position follows the newline that ends the text, so a
    -- range reaching past it ends before it and takes one less newline
    if finish > final_newline then
      finish = final_newline
      text = text:gsub("\n$", "")
      if start > final_newline then
        start = final_newline
        text = "\n" .. text
      end
    end
    local start_row, start_col = place(buf, start)
    local end_row, end_col = place(buf, finish)
    table.insert(changes, {
      at = start,
      args = { start_row, start_col, end_row, end_col, vim.split(text, "\n", { plain = true }) },
    })
  end
  -- the last range first, so the offsets of the earlier ones still hold
  table.sort(changes, function(a, b)
    return a.at > b.at
  end)
  vim.api.nvim_buf_call(buf, function()
    -- starts a new undo step, so an edit that comes in insert mode
    -- does not join what was typed
    vim.cmd("let &l:undolevels = &l:undolevels")
  end)
  for _, change in ipairs(changes) do
    vim.api.nvim_buf_set_text(buf, unpack(change.args))
  end
end

-- the buffer whose name is `path`, if one has it
local function buffer_of(path)
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_get_name(b) == path then
      return b
    end
  end
end

local function receive(message)
  local buf = view.buf
  if message.type == "goto" then
    local win = vim.fn.bufwinid(buf)
    if win ~= -1 then
      local row, col = position(buf, message.offset)
      vim.api.nvim_win_set_cursor(win, { row, col })
      vim.api.nvim_set_current_win(win)
    end
  elseif message.type == "error" then
    local row, col = position(buf, message.offset)
    vim.diagnostic.set(ns, buf, {
      {
        lnum = row - 1,
        col = col,
        message = message.message,
        severity = vim.diagnostic.severity.ERROR,
        source = "acre",
      },
    })
  elseif message.type == "ok" then
    vim.diagnostic.reset(ns, buf)
  elseif message.type == "edit" then
    -- an edit made for an older text would land in the wrong place,
    -- so it is dropped and the viewer is sent the text as it is
    if message.tick == vim.api.nvim_buf_get_changedtick(buf) then
      local ok, err = pcall(apply, buf, message.edits)
      if not ok then
        vim.notify("acre: cannot make the edit: " .. err, vim.log.levels.ERROR)
      end
    end
    send_text()
  elseif message.type == "open" then
    -- the person switched tabs in the viewer: show that file in the
    -- window of the buffer shown so far, and move the viewer to it,
    -- which sends its text back for the tab to show
    local win = vim.fn.bufwinid(buf)
    if win ~= -1 then
      vim.api.nvim_set_current_win(win)
    end
    local target = buffer_of(message.path)
    local ok, err = pcall(function()
      if target and vim.api.nvim_buf_is_loaded(target) then
        vim.cmd.buffer(target)
      else
        vim.cmd.edit(vim.fn.fnameescape(message.path))
      end
    end)
    if not ok then
      vim.notify("acre: cannot open " .. message.path .. ": " .. err, vim.log.levels.ERROR)
      return
    end
    M.switch()
  end
end

-- Wire the autocmds that keep the viewer up to date with `buf`.
local function follow(buf)
  vim.api.nvim_clear_autocmds({ group = group })
  local changed = function()
    view.timer:stop()
    view.timer:start(M.config.debounce, 0, vim.schedule_wrap(function()
      if view then
        send_text()
      end
    end))
  end
  local events = M.config.render == "change" and { "TextChanged", "TextChangedI" } or {}
  if #events > 0 then
    vim.api.nvim_create_autocmd(events, { group = group, buffer = buf, callback = changed })
  end
  vim.api.nvim_create_autocmd("BufWritePost", { group = group, buffer = buf, callback = send_text })
  vim.api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI" }, {
    group = group,
    buffer = buf,
    callback = send_cursor,
  })
  vim.api.nvim_create_autocmd({ "BufWipeout", "VimLeavePre" }, {
    group = group,
    buffer = buf,
    callback = M.stop,
  })
end

function M.stop()
  if not view then
    return
  end
  local old = view
  view = nil
  vim.api.nvim_clear_autocmds({ group = group })
  -- a socket's viewer keeps running, without this buffer
  if old.socket then
    vim.fn.chanclose(old.chan)
  else
    vim.fn.jobstop(old.chan)
  end
  if old.timer then
    old.timer:close()
  end
  if vim.api.nvim_buf_is_valid(old.buf) then
    vim.diagnostic.reset(ns, old.buf)
  end
end

-- A callback for a channel's output that hands each whole line to
-- `receive` as a message.
local function lines()
  local pending = ""
  return function(_, data)
    -- data holds pieces split at newlines, the last one unfinished
    pending = pending .. data[1]
    for i = 2, #data do
      if pending ~= "" then
        local ok, message = pcall(vim.json.decode, pending)
        if ok and view then
          receive(message)
        end
      end
      pending = data[i]
    end
  end
end

-- Show `buf` in the viewer at the other end of `chan`.
local function open(chan, buf, socket)
  view = { chan = chan, buf = buf, socket = socket, timer = vim.uv.new_timer() }
  follow(buf)
  send_text()
  send_cursor()
end

-- Link the current buffer to the viewer listening on the socket at
-- `path`. True when it connected.
local function connect(path)
  local buf = vim.api.nvim_get_current_buf()
  local on_lines = lines()
  local chan
  local ok, result = pcall(vim.fn.sockconnect, "pipe", path, {
    on_data = function(id, data)
      -- a lone empty piece is the end of the stream
      if #data == 1 and data[1] == "" then
        if view and view.chan == chan then
          M.stop()
        end
        return
      end
      on_lines(id, data)
    end,
  })
  if not ok or result <= 0 then
    return false
  end
  chan = result
  open(chan, buf, true)
  return true
end

-- Link the current buffer to the viewer listening on the socket at
-- `path`, or say why not.
function M.connect(path)
  M.stop()
  if not connect(path) then
    vim.notify("acre: cannot connect to the viewer at " .. path, vim.log.levels.ERROR)
  end
end

function M.start()
  M.stop()
  -- a stale socket, whose viewer has closed, falls back to a viewer of
  -- this buffer's own
  local socket = vim.env.ACRE_SOCKET
  if socket and socket ~= "" and connect(socket) then
    return
  end
  local buf = vim.api.nvim_get_current_buf()
  local errors = {}
  local job
  job = vim.fn.jobstart(M.config.cmd, {
    on_stdout = lines(),
    on_stderr = function(_, data)
      for _, line in ipairs(data) do
        if line ~= "" then
          table.insert(errors, line)
          if #errors > 20 then
            table.remove(errors, 1)
          end
        end
      end
    end,
    on_exit = function(_, status)
      -- a job that M.stop already stopped is no longer the open view
      local shown = view and view.chan == job
      if shown then
        M.stop()
      end
      if shown and status ~= 0 then
        local message = "acre: the viewer exited with status " .. status
        if #errors > 0 then
          message = message .. "\n" .. table.concat(errors, "\n")
        end
        vim.notify(message, vim.log.levels.ERROR)
      end
    end,
  })
  if job <= 0 then
    vim.notify("acre: cannot start " .. table.concat(M.config.cmd, " "), vim.log.levels.ERROR)
    return
  end
  open(job, buf, false)
end

-- Show the current buffer in the running viewer, in place of the
-- buffer it showed.
function M.switch()
  local buf = vim.api.nvim_get_current_buf()
  if vim.api.nvim_buf_is_valid(view.buf) then
    vim.diagnostic.reset(ns, view.buf)
  end
  view.timer:stop()
  view.buf = buf
  follow(buf)
  send_text()
  send_cursor()
end

function M.toggle()
  if not view then
    M.start()
  elseif view.buf == vim.api.nvim_get_current_buf() then
    M.stop()
  else
    M.switch()
  end
end

-- for tests
M._position = position
M._receive = receive

return M
