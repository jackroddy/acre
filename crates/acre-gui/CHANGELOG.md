# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `acre-gui` editor. It shows a drawing's source beside its render and
  redraws the render on every edit. The source is colored by token. An error is
  underlined in the source, and its message shows in the status line. The last
  good render stays up until the error is fixed.
- A File menu with New, Open, Open Recent, Save and Save As. acre-gui asks
  before it throws away unsaved changes.
- `acre-gui FILE` opens a file. Without a file it reopens the last one.
- `~/.config/acre/config.toml` sets `editor-font-size`.
- An Edit menu with Format, also on Ctrl+Shift+F, which rewrites the source in
  acre's one layout.
- The source shows each color on a swatch of that color, including each `let`
  name that stands for a color. Markup signs inside strings and `let` names
  have their own colors.
- The render is drawn at the pane's own pixels, so it stays sharp at any size.
  The scroll wheel zooms about the pointer, a drag pans, and a double-click
  fits the canvas to the pane. A View menu zooms in and out (Ctrl+= and
  Ctrl+-), fits (Ctrl+0) and shows actual size (Ctrl+1). The status line shows
  the zoom.
- A pan or a zoom moves and scales the last drawn image, which reaches a
  quarter of the pane past each edge. acre-gui draws the scene again 150 ms
  after the person stops, or sooner when the image no longer covers the pane.
- acre-gui reads its file again when it changes on disk and has no unsaved
  edits. With unsaved edits it keeps them and says the file changed.
- A click on the render selects, in the editor, the node that drew the object
  under the pointer. With the editor's cursor inside a node, the render
  outlines what that node drew. The status line shows the canvas point under
  the pointer.
- The editor shows line numbers and no longer wraps lines. Enter keeps the
  line's indent, one more after a `{`. A `}` typed first on a line moves one
  indent left. The brace beside the cursor and its pair are shaded.
- Edit menu entries for Find (Ctrl+F), with Next, Replace and replace All;
  Toggle Comment (Ctrl+/); and Complete (Ctrl+Space), which completes a
  property, node kind or value word from the schema and lists the choices in
  the status line.
- A reference panel (F1, or View > Reference) describes the node kind the
  cursor sits in and each property it takes, with the one under the cursor
  marked.
