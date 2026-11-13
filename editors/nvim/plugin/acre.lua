vim.api.nvim_create_user_command("AcreView", function()
  require("acre").toggle()
end, { desc = "show or hide the acre viewer for this buffer" })
