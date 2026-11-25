# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The toolbar has a Lock toggle next to Snap and Grid. While it is on,
  acre-gui refuses every edit to the source. A drag on an object only
  selects it. No handles show. The creation tools and the arrange buttons
  are disabled. Delete, paste, Duplicate, the arrow-key moves and a
  double-click on text change nothing. The status line then says
  "locked". The properties panel shows values with its controls
  disabled. Selection, box selection, pan, zoom, Ctrl+C and playback
  still work.
- `acre-gui --locked` starts the viewer locked.
- Without `--stdio`, acre-gui listens for an editor on a Unix socket at
  `$XDG_RUNTIME_DIR/acre/<pid>.sock`. Without `XDG_RUNTIME_DIR`, the
  socket goes in `acre-$USER` in the temporary folder. An editor that
  connects talks to the viewer as one on stdin and stdout does, one
  editor at a time. When it disconnects, the viewer keeps running and
  watches its files again. acre-gui removes the socket when it closes.
- File > Edit in neovim opens neovim on the drawing shown, with the
  socket's path in `ACRE_SOCKET`, and the plugin links to the viewer.
  Inside decade, neovim opens in ghostty in a pane split beside the
  viewer. Elsewhere it opens in the terminal `xdg-terminal-exec` starts.
  The item is disabled while an editor is linked and for a drawing with
  no file.
- Hovering a file under File > Open Recent shows a small render of it
  at time 0 beside the menu. A spinner shows while the render is built.
  For a missing file or one with an error, the menu shows the first
  line of the error instead. acre-gui keeps each render for the session
  and builds it again when the file changes.
- acre-gui holds several drawings in tabs, in a bar under the menu. A
  tab shows the file's name, and its full path on hover. Each tab keeps
  its own selection, zoom and playback, and without `--stdio` watches its
  own file. File > Open and Open Recent open a file in a new tab, or show
  its tab when one has it. A tab's × asks before it closes the tab.
  Closing the last tab leaves an empty viewer that says to open a file.
  Switching tabs drops a text field left open in the tab left, without
  writing it.
- With `--stdio`, a text for another path opens that path in a tab, or
  shows its tab, in place of the drawing shown. An unnamed buffer's tab
  is "untitled". A click on a tab sends `{"type":"open","path":...}`,
  which asks the editor to show that file and send its text. Until it
  does, acre-gui refuses edits in that tab.

### Changed

- The File menu is the first button of the toolbar, and the tab bar sits
  under the toolbar. The File menu's own bar is gone. With `--stdio` the
  toolbar has no File menu, as before.

## [0.2.0] - 2026-11-24

### Added

- A × button at the right end of the toolbar closes acre-gui after a
  dialog asks to confirm. Cancel, or a click outside the dialog, keeps it
  open.
- Dragging one child of a stack or a grid reorders it among its siblings.
  The viewer outlines the child under the pointer. A line marks the place
  it would land: across the stack at the nearest gap, or along the
  nearest side of a grid cell in reading order. On release the child's
  source moves there and the child stays selected. Before, the viewer
  refused the drag. A drag on several selected nodes that includes a
  child of a stack or grid is refused, and the status line gives the
  reason.

### Changed

- acre-gui draws no title bar of its own. Inside decade, which leaves
  decorations to the client, winit drew one whose close button did
  nothing.
- When the editor sends a source with another path, the viewer pauses at 0,
  as it does when it opens a file.
- The sections panel and the properties panel lie over the render instead
  of narrowing it. Opening or closing either leaves the zoom and the
  position of the drawing as they were, and the part of the drawing under
  an open panel is hidden. A click, drag or scroll over a panel goes to the
  panel. The panels no longer resize by dragging their edge.
- The editor's cursor selects a node, as a click does, in place of the orange
  outline it drew. The node selected is the innermost one below the canvas
  that holds the cursor and draws something. A node holds the cursor from the
  first letter of its kind to its closing brace. The cursor between nodes or
  on the canvas's own lines clears the selection. The cursor in a line's
  indentation counts as on the line's first character. The cursor in the
  spaces after a line's last character counts as on that character, so the
  line of a closing brace selects the node the brace closes. A blank line
  inside a stack or group selects the stack or group. The cursor in a child of
  a selected node selects the child. After a click, the viewer ignores the one
  cursor message that answers its goto. The clicked node then stays selected
  and its properties panel stays open. The properties panel opens only for a
  selection made in the viewer by a click, a box or a new node. A selection
  the cursor made leaves it closed.

## [0.1.0] - 2026-11-24

### Added

- The `acre-gui` viewer. It renders a drawing and draws it again when the
  source changes. The last good render stays up while the source has an
  error, and the error's line, column and message show in the status line.
- `acre-gui FILE` shows a file and reads it again whenever it changes on disk.
  Without a file it shows the last one opened. File > Open and Open Recent
  pick another.
- `acre-gui --stdio` renders the source an editor sends as JSON lines on
  stdin, and answers on stdout with the source's error and with clicks on the
  render. It closes when stdin closes. `src/link.rs` lists the messages. A
  `text` message carries a `tick`, the editor's number for that version of the
  source. The outgoing message `edit` holds a list of `{start, end, text}`
  byte ranges in the source of a tick, and the editor makes the edit only
  while its own tick still matches. While an edit is with the editor, the
  viewer refuses another with "waiting for the editor; try again", and a
  refused drag snaps back.
- The viewer reads the font files a drawing lists in `fonts` and the data
  files it names with `data`, each relative to the drawing's file. It builds
  the drawing again when one of them changes.
- The viewer prints a warning on stderr for each character in a drawing's
  text that no font has, whenever the set of those characters changes. The
  status line shows the first of them. A source error shows ahead of the
  warning, and so does a notice set after the last build, such as the reason
  a drag was refused. A new build clears the notice.
- The render is drawn at the pane's own pixels, so it stays sharp at any size.
  The scroll wheel zooms about the pointer, a drag with the middle button
  pans, and a double-click on empty canvas fits the canvas to the pane.
  Buttons zoom out, zoom in, fit, and show the canvas at 100%. The status line
  shows the zoom and the canvas point under the pointer.
- A pan or a zoom moves and scales the last drawn image, which reaches a
  quarter of the pane past each edge. acre-gui draws the scene again 150 ms
  after the person stops, or sooner when the image no longer covers the pane.
  acre-gui shrinks the margin to keep the image within the GPU's texture
  limit, which it reads at run time. When the pane alone is larger than the
  limit, acre-gui draws it at a lower resolution.
- A click on the render sends the editor to the node that drew the object
  under the pointer. Without an editor, the status line names the node's
  line. The render outlines in orange what the node under the editor's cursor
  drew.

- A drawing with a `timeline` plays in the viewer. A bar under the render
  holds a Play and Pause button, a time bar over the drawing's length, the
  time as `1.25 s / 5.00 s`, and a Loop toggle that starts on. A press or a
  drag on the time bar goes to that time. Space plays and pauses. With
  nothing selected, Left and Right step one frame. A file opens paused at 0.
  A reload keeps the time and whether it plays, and cuts the time to the new
  length. A still drawing shows no bar.
- The time bar shows a drawing's sections. A line marks where each one starts,
  and its title sits in its stretch of the bar, cut short when it does not
  fit. A short tick marks where each subsection starts, and hovering the bar
  names the section and subsection under the pointer. A Sections toggle
  beside Loop opens a left-hand panel, open at start, that lists each section
  with its start, marks the one playing, and goes to a section's start on a
  click. In the panel, a section with subsections has a fold arrow and starts
  folded. Opened, it lists its subsections, marks the one playing, and goes
  to one's start on a click.

- A click on an object selects the node that drew it. The viewer outlines
  the node's objects in blue at their bounds at the time shown, outside the
  orange outline that follows the editor's cursor. A click on empty canvas
  clears the selection, and so does opening another file. After an edit, the
  selection stays on the node at the same place in the source tree.
- A drag with the primary button that starts on empty canvas draws a
  translucent selection box. On release, the viewer selects each node whose
  objects all lie inside the box. When a whole group lies inside, the viewer
  selects the group and not its children. When only one child of a group
  lies inside, the viewer selects that child. A box around no whole node
  clears the selection.
- A drag with the primary button that starts on an object selects its node
  and moves it. A drag on any selected node, or on an object inside one,
  moves every selected node by the same offset. The viewer draws the nodes at
  the pointer's offset during the drag. On release it works out the source
  edits, sends them to the editor with `--stdio`, and writes them to the file
  without. A node inside another selected node moves with that node. A node
  placed `at` a selected node moves with it once, and so does an arrow or
  line whose ends name selected nodes. When acre cannot move one of the
  nodes, none moves, and the status line gives that node's reason.
- A dragged object snaps to other objects. Its left edge, centre and right
  edge snap to those of any other object, and so do its top, middle and
  bottom. Each axis snaps on its own. A snap reaches 6 screen points at any
  zoom, and the object comes free once the pointer moves past that. A thin
  line marks each snap that holds. The written numbers land on the snap,
  to two decimals. Without a snap, a move is in whole pixels. Several nodes
  snap as one, on the box around all their objects.
- The toolbar has a `Snap` toggle, on at start, that turns snapping on and
  off. A `Grid` toggle, off at start, draws a faint grid over the canvas.
  The field beside it sets the grid's spacing in canvas pixels, 10 at
  start. When zoomed far out, the viewer draws every second line, or
  fewer. With the grid on, an axis that snaps to no object snaps the
  dragged object's top-left corner to the grid.
- With a selection, each press of an arrow key moves the selected nodes
  one canvas pixel, without snapping. Holding the key keeps moving them.
  With `--stdio`, the presses made while an edit is with the editor add up
  and go out as one edit once the editor sends its text back. With nothing
  selected, Up and Down do nothing.
- Delete deletes the selected nodes and clears the selection. When acre
  cannot delete them, the status line gives the reason.
- Ctrl+C puts the selected nodes' source on the clipboard. Ctrl+V adds the
  nodes on the clipboard to the canvas, 10 pixels right of and below where
  they were, and selects them. When the clipboard holds no acre nodes, or
  acre cannot add them, the status line gives the reason.
- A right-click on an object selects its node and opens a menu with Delete
  and Duplicate. Duplicate adds a copy 10 pixels right of and below the
  node, as a copy and a paste do.
- A double-click on an object opens a field over it that holds its node's
  text as the file writes it, markup included. A box, an ellipse or a
  connector with no text opens an empty field, and typing writes its `text`
  or `label`. The field shows as many lines as the text has, and Shift+Enter
  starts a new line. The viewer writes the text when the field loses focus:
  on Enter, Escape, Tab or a click elsewhere. An unchanged or empty text
  writes nothing. The edit goes to the editor over the link, or into the
  file, as a drag's does. The double-click selects the node, and the
  selection stays on it after the edit. When the node's kind takes no text,
  or its text comes from a `let`, a style, a `repeat` or the timeline, the
  status line says why.
- While a text field has focus, none of the viewer's keys acts, except
  Shift+Enter in the field.

- The toolbar has a palette of tools: Select, Box, Ellipse, Text, Line,
  Arrow, Polygon and Polyline. After a tool adds a node, the palette goes
  back to Select. acre-gui adds each node at the end of the canvas, before
  its timeline, as one edit, and selects it. When the source with the new
  node would not build, acre-gui writes nothing and the status line gives
  the build's error.
- With Box or Ellipse, a drag from one corner to the other draws the
  outline, and the dragged corner snaps to other objects and the grid as a
  moved object does. A click adds one 80 by 48 pixels at the click.
- With Text, a click adds `text "text"` there and opens its text field.
  With `--stdio`, the field opens once the editor sends its text back.
- With Line or Arrow, a drag runs from one end to the other. A box,
  ellipse, polygon, polyline, row, matrix or code under the pointer shows
  nine dots: its centre, the middles of its sides and its corners. An
  ellipse's corner dots sit where the diagonals of its box cross the
  ellipse, which is where acre ends a line there. Polygons and polylines
  take the dots of the box around them, as their ends do. An end within 8
  screen points of a dot snaps to it and writes `a top`, `a top-left` or,
  for the centre, the bare `a`. Elsewhere within 8 points of the outline,
  the end writes `a top F` for the nearest side, with F rounded to 2
  decimals, and a dot marks the spot. Inside the thing and away from its
  outline, the end writes the bare name, which ends on the edge facing the
  other end. The same edit names the thing when it has no name. An end
  anywhere else is a point. A copy a `repeat` draws shows no dots. acre-gui
  refuses a line with both ends on one thing or at one point.
- With Polygon or Polyline, each click adds a point. A click on the first
  point closes a polygon, and a click on the last point again ends a
  polyline. While a path is drawn, the toolbar shows Done and Cancel.

- One selected box or ellipse shows eight handles, at its corners and the
  middles of its sides. A drag on a handle draws the new outline and
  resizes the node on release. The dragged edges snap to the edges and
  centres of other objects and to the grid. An edge dragged past the one
  across from it stops 1 pixel short. A left or top handle also moves
  `at`, so the edge across stays put. A node its text sizes gets a
  `size`.
- One selected polygon or polyline shows a handle on each vertex. A drag
  on a handle moves that vertex and rewrites its pair in `points`.
- One selected line or arrow shows a handle on each end. While a handle
  is dragged, the thing under the pointer shows the same dots as for the
  Line tool. On release acre-gui rewrites that end's `from` or `to`, and
  names the thing when it has no name. An end dropped on empty canvas
  becomes a point. An end inside a `repeat`, or one given by a style, a
  `let` or `( ... )`, is refused. A `merge`'s ends have no handles.
- Over a handle the pointer changes to a resize or move cursor.
- With two or more nodes selected, the toolbar shows Left, Centre, Right,
  Top, Middle and Bottom. Each lines up the selected nodes on that edge or
  centre of the box around them all. With three or more, Space across and
  Space down sort the nodes by their left or top edge. The first and last
  stay put, and the others move so the gaps between neighbours are equal.
  Each node's box holds everything it draws at the time shown. A node
  inside another selected node moves with it. The offsets are written with
  up to two decimals, as for a snapped drag, and go out as one edit. A
  node a stack or grid places is refused, as for a drag.
- With one or more nodes selected, a panel on the right lists the
  properties every selected node's kind takes among `fill`, `stroke`,
  `stroke-width`, `dash`, `color`, `text-color`, `text-size` and `font`.
  Each row shows the value and whether the node, a style or the default
  sets it, or "mixed" where the nodes differ. A color has a color button,
  and a `none` toggle where the property takes `none`. A size has a number
  field, and `font` a list of the five built-in fonts and those `fonts`
  registers. acre-gui writes a change to every selected node as one edit.
  It writes a drag in the color picker or on a number once, when the mouse
  button is released. The × beside a node's own value takes it away. For
  a refused change acre-gui writes nothing, and the status line gives the
  reason.

[Unreleased]: https://github.com/jackroddy/acre/compare/acre-gui-v0.2.0...HEAD
[0.2.0]: https://github.com/jackroddy/acre/compare/acre-gui-v0.1.0...acre-gui-v0.2.0
[0.1.0]: https://github.com/jackroddy/acre/releases/tag/acre-gui-v0.1.0
