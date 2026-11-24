# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A dragged object snaps to other objects. Its left edge, centre and right
  edge snap to those of any other object, and so do its top, middle and
  bottom. Each axis snaps on its own. A snap reaches 6 screen points at any
  zoom, and the object comes free once the pointer moves past that. A thin
  line marks each snap that holds. The written numbers land on the snap,
  to two decimals. Without a snap, a move is in whole pixels.
- The toolbar has a `Snap` toggle, on at start, that turns snapping on and
  off. A `Grid` toggle, off at start, draws a faint grid over the canvas.
  The field beside it sets the grid's spacing in canvas pixels, 10 at
  start. When zoomed far out, the viewer draws every second line, or
  fewer. With the grid on, an axis that snaps to no object snaps the
  dragged object's top-left corner to the grid.
- A click on an object selects the node that drew it. The viewer outlines
  the node's objects in blue at their bounds at the time shown. The blue
  outline sits outside the orange one that follows the editor's cursor.
  The selection lasts until a click on empty canvas or on another object.
  A drag selects the node it moves. After an edit, the selection stays on
  the node at the same place in the source tree. The selection ends when
  the viewer opens another file.
- A drag with the primary button that starts on an object moves the object.
  While the pointer moves, the viewer draws the object at the pointer's
  offset with an outline. On release the viewer works out the source edits
  for the move. With `--stdio` it sends them to the editor as an `edit`.
  Without, it writes them to the file and shows the result. When acre cannot
  move the object, the object goes back and the status line gives the reason.
  A drag that starts on empty canvas pans as before. A drag with the middle
  button always pans. A click still goes to the node under the pointer.
- With `--stdio`, a `text` message carries a `tick`. The tick is the
  editor's number for that version of the source. A new outgoing message
  `edit` holds a list of `{start, end, text}` byte ranges in the source of a
  tick. The editor makes the edit only while its own tick still matches.
- The viewer reads the data files a drawing names with `data`. A path is
  relative to the drawing's file. The viewer builds the drawing again when
  one of them changes.
- The time bar shows a drawing's sections. A line marks where each one starts,
  and its title sits in its stretch of the bar, cut short when it does not
  fit. A press or a drag on the bar goes to that time. A Sections toggle
  beside Loop opens a left-hand panel that lists each section with its start,
  marks the one playing, and goes to a section's start on a click. The panel
  starts open. A short tick on the bar marks where each subsection starts, and
  hovering the bar names the section and subsection under the pointer. In the
  panel, a section with subsections has a fold arrow and starts folded.
  Opened, it lists its subsections, marks the one playing, and goes to one's
  start on a click.
- The viewer prints a warning on stderr for each character in a drawing's
  text that no font has, whenever the set of those characters changes. The
  status bar shows the first of them.
- A drawing with a `timeline` plays in the viewer. A bar under the render
  holds a Play and Pause button, a slider over the drawing's length, the time
  as `1.25 s / 5.00 s`, and a Loop toggle that starts on. Space plays and
  pauses, and Left and Right step one frame, unless a text field has focus. A
  file opens paused at 0. A reload keeps the time and whether it plays, and
  cuts the time to the new length. A still drawing shows no bar.
- The viewer reads the font files a drawing lists in `fonts`, relative to the
  drawing's file, and builds the drawing again when one of them changes.
- The `acre-gui` viewer. It renders a drawing and draws it again when the
  source changes. The last good render stays up while the source has an
  error, and the error's line, column and message show in the status line.
- `acre-gui FILE` shows a file and reads it again whenever it changes on disk.
  Without a file it shows the last one opened. File > Open and Open Recent
  pick another.
- `acre-gui --stdio` renders the source an editor sends as JSON lines on stdin,
  and answers on stdout with the source's error and with clicks on the render.
  It closes when stdin closes. `src/link.rs` lists the messages.
- The render is drawn at the pane's own pixels, so it stays sharp at any size.
  The scroll wheel zooms about the pointer, a drag pans, and a double-click
  fits the canvas to the pane. Buttons zoom out, zoom in, fit, and show the
  canvas at 100%. The status line shows the zoom and the canvas point under
  the pointer.
- A pan or a zoom moves and scales the last drawn image, which reaches a
  quarter of the pane past each edge. acre-gui draws the scene again 150 ms
  after the person stops, or sooner when the image no longer covers the pane.
- A click on the render sends the editor to the node that drew the object
  under the pointer. Without an editor, the status line names the node's
  line. The render outlines what the node under the editor's cursor drew.

### Fixed

- A missing-glyph warning no longer hides the status line's notices. A
  notice set after the last build, such as the reason a drag was refused,
  now shows ahead of the warning. A new build clears the notice. A source
  error still shows first.

- The viewer no longer crashes in a very wide window. It draws a margin past
  each edge of the pane, so its texture was 1.5 times the pane's size. egui
  creates the GPU device with a limit of 8192 pixels on a side for a texture.
  In a wide window on a Retina screen, the viewer's texture was larger than
  that. The viewer now asks egui for the device's texture limit at run time,
  whatever that limit is, and draws a smaller margin to stay within it. If the
  pane alone is wider or taller than the device's limit, the viewer draws it
  at a lower resolution.
