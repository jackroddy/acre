# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-11-24

### Fixed

- `acre reference` and `acre fmt -` end quietly when the program reading
  their output stops early, as in `acre reference | head`. Before, the first
  panicked and the second printed an error, and both exited non-zero.

## [0.1.0] - 2026-11-24

### Added

- The `acre` command, moved here from the `acre` crate. Its 0.1.0 release
  shipped inside `acre` 0.1.0, with `check` and `render` that drew nothing.
- `acre check FILE` reports the first error in a file with its line and
  column.
- `acre render FILE` draws the file and writes a PNG, SVG, PDF or MP4 file.
  It takes the format from the extension of `--out`, and
  `--format png|svg|pdf|mp4` names it outright. Without `--out`, the file
  goes beside the drawing with the format's extension. An `--out` with any
  other extension is an error unless `--format` is given. `--out` names the
  file, where in 0.1.0 it named a directory.
- `acre render --scale N` writes N image pixels to each canvas pixel, or N
  CSS pixels for SVG and PDF.
- `acre render --time T` draws the drawing T seconds into its timeline.
  Without it, `render` draws the start.
- `render` writes an animation as an MP4 file when `--out` ends in `.mp4` or
  `--format mp4` is given. It draws a frame every `1/fps` seconds from 0
  through the duration and pipes them to `ffmpeg`, which must be on the
  `PATH`. `--fps N` replaces the canvas's frame rate and `--jobs N` draws
  frames on N threads. A drawing with no duration is an error, and so is
  `--fps` for another format. A failed export removes the partial file and
  prints ffmpeg's message.
- `check` and `render` read the font files a drawing lists in `fonts` and the
  data files it names with `data`, each relative to the drawing's file. A
  file that cannot be read, or a math font without a MATH table, is an error
  that names the file and its line.
- `check` and `render` print a warning on stderr for each character in a
  drawing's text that no font has, such as
  `drawing.acre: warning: no font has a glyph for '☃' (U+2603)`.
- `acre fmt FILE...` rewrites drawings in acre's one layout. `--check` lists
  the files that would change and writes nothing. `acre fmt -` formats stdin
  and writes the result to stdout, for an editor's format command.
- `acre reference [KIND]` lists every node kind, or one, with each property
  it takes, the values the property takes, and what it does.
  `acre reference timeline` lists the timeline's actions.

[Unreleased]: https://github.com/jackroddy/acre/compare/acre-cli-v0.1.1...HEAD
[0.1.1]: https://github.com/jackroddy/acre/compare/acre-cli-v0.1.0...acre-cli-v0.1.1
[0.1.0]: https://github.com/jackroddy/acre/releases/tag/acre-cli-v0.1.0
