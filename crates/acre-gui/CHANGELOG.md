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
