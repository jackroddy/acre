# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

