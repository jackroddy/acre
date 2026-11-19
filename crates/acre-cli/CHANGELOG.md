# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `render` writes an animation as an MP4 file when `--out` ends in `.mp4` or
  `--format mp4` is given. It draws a frame every `1/fps` seconds from 0
  through the duration and pipes them to `ffmpeg`, which must be on the
  `PATH`. `--fps N` replaces the canvas's frame rate and `--jobs N` draws
  frames on N threads. A drawing with no duration is an error. A failed
  export removes the partial file and prints ffmpeg's message.
- `render --time T` draws the drawing T seconds into its timeline. Without it,
  `render` draws the start, as before.
- `render` writes SVG or PDF as well as PNG. It takes the format from the
  extension of `--out`, and `--format png|svg|pdf` names it outright. Without
  `--out`, the file goes next to the drawing with the format's extension. An
  `--out` with any other extension is an error unless `--format` is given.
- `check` and `render` read the font files a drawing lists in `fonts`, with
  paths relative to the drawing's file. A file that cannot be read, or a math
  font without a MATH table, is an error naming the file and its line.
- The `acre` command, moved here from the `acre` crate. Its 0.1.0 release
  shipped inside `acre` 0.1.0.

### Changed

- `acre check FILE` reports the first error in a file with its line and column.
- `acre render FILE` draws the file and writes a PNG image. `--out` names the
  image file, where it used to name a directory. Without `--out` the image goes
  beside the source with a `.png` extension.
- `acre render --scale N` writes the image at N image pixels to each canvas
  pixel.
- `acre fmt FILE...` rewrites drawings in acre's one layout. `--check` lists
  the files that would change and writes nothing.
- `acre fmt -` formats stdin and writes the result to stdout, for an editor's
  format command.
- `acre reference [KIND]` lists every node kind, or one, with each property
  it takes, the values the property takes, and what it does.
