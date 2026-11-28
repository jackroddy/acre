# acre for neovim

Filetype detection, syntax, settings, and `:AcreView`, which opens the acre
viewer beside the buffer.

Install it with lazy.nvim from acre's `nvim` branch, which holds this
directory at its root:

```lua
{ "jackroddy/acre", branch = "nvim", config = function() require("acre").setup({}) end }
```

To run it from a checkout of acre, add this directory to neovim's runtime
path, for example with lazy.nvim's `dir`:

```lua
{ dir = "/path/to/acre/editors/nvim", config = function() require("acre").setup({}) end }
```

`:AcreView` starts `acre-gui --stdio` for the current buffer. Run in another
buffer, it moves the open viewer to that buffer, which the viewer shows in a
tab of its own, or in the tab it already has. Run in the buffer the viewer
shows, it closes the viewer. A click on another tab in the viewer shows that
tab's file in the window of the buffer it showed, loading the file if no
buffer has it, and moves the viewer there. The viewer draws the buffer as you
type. A click on the render moves
the cursor to the node that drew it. Moving the cursor into a node selects
that node in the viewer. An error in the source shows as a
diagnostic. `gggqG` formats the buffer with `acre fmt -`, which reads a whole
file, so `gq` on part of one fails. `acre` and `acre-gui` have to be on
the path. A viewer that exits with an error shows its last lines of
stderr as an error message, and one the person closed says nothing.

A viewer started on its own, without `--stdio`, listens on a socket. Its File
> Edit in neovim opens neovim on the drawing shown, in a terminal, with a
command on neovim's command line that calls `require("acre").connect(path)`
for the socket, so the plugin links that buffer to the viewer instead of
starting one. Neovim started with the socket's path in `ACRE_SOCKET` links the
first acre buffer the same way, and `:AcreView` with `ACRE_SOCKET` set links
to that viewer, or starts a viewer of its own when nothing listens on the
socket. `:AcreView` in the linked buffer closes the link and leaves the viewer
running.

`setup` takes these options:

| option | default | what it sets |
|---|---|---|
| `cmd` | `{ "acre-gui", "--stdio" }` | the viewer and its arguments |
| `render` | `"change"` | `"change"` draws as you type, `"save"` on each write |
| `debounce` | `150` | milliseconds of quiet before a change is sent |

## Tests

`tests/edit.lua` checks the plugin's side of the link with a stand-in viewer.
`tests/socket.lua` checks the link over a socket, with a socket server in
neovim standing in for the viewer.
`tests/viewer.lua` runs the real viewer from `target/release/acre-gui`. It
opens a window and needs a display. Run each from the repository root:

```sh
nvim --clean --headless -l editors/nvim/tests/edit.lua
nvim --clean --headless -l editors/nvim/tests/socket.lua
nvim --clean --headless -l editors/nvim/tests/viewer.lua
```

A failed check prints `FAIL` and makes neovim exit with status 1.
