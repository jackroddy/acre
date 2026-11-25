vim.api.nvim_create_user_command("AcreView", function()
  require("acre").toggle()
end, { desc = "show or hide the acre viewer for this buffer" })

-- neovim opened by the viewer's "Edit in neovim" links the first acre
-- buffer to that viewer
if vim.env.ACRE_SOCKET and vim.env.ACRE_SOCKET ~= "" then
  vim.api.nvim_create_autocmd("FileType", {
    pattern = "acre",
    once = true,
    callback = function()
      vim.schedule(function()
        require("acre").connect(vim.env.ACRE_SOCKET)
      end)
    end,
  })
end
