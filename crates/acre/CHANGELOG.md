# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A first version of the acre language, for still drawings. A file holds one
  `canvas`, and the canvas holds `box`, `ellipse`, `line`, `arrow` and `text`
  nodes. `examples/layers.acre` uses each of them.
- `lang::parse` reads source text into a tree of nodes. `scene::build` turns
  that tree into a `Scene`. `render::draw_frame` draws a scene into a pixmap.
- `lang::highlight` returns every token in a source with a class, for coloring
  it in an editor. Unlike `lang::parse`, it reads past errors.

### Removed

- The `acre` command. It now comes from the `acre-cli` crate, and this crate is
  a library only.

## [0.1.0] - 2026-09-30

### Added

- The `acre` command, with `check <file>` and `render [--out <dir>] <file>`.
  Both name their arguments and exit with "not written yet": this release
  holds the crate name and nothing renders.

[Unreleased]: https://github.com/jackroddy/acre/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jackroddy/acre/releases/tag/v0.1.0
