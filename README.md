# acre for neovim

Filetype detection, syntax, settings, and `:AcreView`, which opens the acre
viewer beside the buffer.

Add this directory to neovim's runtime path, for example with lazy.nvim:

```lua
{ dir = "/path/to/acre/editors/nvim", config = function() require("acre").setup({}) end }
```

`:AcreView` starts `acre-gui --stdio` for the current buffer, and runs again to
close it. The viewer draws the buffer as you type. A click on the render moves
the cursor to the node that drew it, and an error in the source shows as a
diagnostic. `gggqG` formats the buffer with `acre fmt -`, which reads a whole
file, so `gq` on part of one fails. `acre` and `acre-gui` have to be on
the path.

`setup` takes these options:

| option | default | what it sets |
|---|---|---|
| `cmd` | `{ "acre-gui", "--stdio" }` | the viewer and its arguments |
| `render` | `"change"` | `"change"` draws as you type, `"save"` on each write |
| `debounce` | `150` | milliseconds of quiet before a change is sent |
