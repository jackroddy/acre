# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
