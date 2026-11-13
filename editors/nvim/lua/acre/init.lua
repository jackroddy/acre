-- The acre viewer for a buffer: acre-gui started with --stdio, sent
-- the buffer's text and cursor as JSON lines, and answering with the
-- source's error and with clicks on the render.

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

-- the viewer that is running, at most one
local view = nil

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})
end

local function send(message)
  if view then
    vim.fn.chansend(view.job, vim.json.encode(message) .. "\n")
  end
end

local function send_text()
  local buf = view.buf
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  send({
    type = "text",
    text = table.concat(lines, "\n") .. "\n",
    path = vim.api.nvim_buf_get_name(buf),
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
  end
end

function M.stop()
  if not view then
    return
  end
  local old = view
  view = nil
  vim.api.nvim_clear_autocmds({ group = group })
  vim.fn.jobstop(old.job)
  if old.timer then
    old.timer:close()
  end
  if vim.api.nvim_buf_is_valid(old.buf) then
    vim.diagnostic.reset(ns, old.buf)
  end
end

function M.start()
  M.stop()
  local buf = vim.api.nvim_get_current_buf()
  local pending = ""
  local job = vim.fn.jobstart(M.config.cmd, {
    on_stdout = function(_, data)
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
    end,
    on_exit = function()
      if view and view.job == job then
        M.stop()
      end
    end,
  })
  if job <= 0 then
    vim.notify("acre: cannot start " .. table.concat(M.config.cmd, " "), vim.log.levels.ERROR)
    return
  end
  view = { job = job, buf = buf, timer = vim.uv.new_timer() }

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
  send_text()
  send_cursor()
end

function M.toggle()
  if view then
    M.stop()
  else
    M.start()
  end
end

-- for tests
M._position = position

return M
