# acre

A Rust tool that reads a drawing written in its own small language and
renders it. It is a cargo workspace of three crates under `crates/`. `acre` is
the library. `acre-cli` builds the `acre` command, which checks and renders
files. `acre-gui` builds `acre-gui`, the editor and the main way a person works
with acre.

Work and facts live in foam, not here: `foam ready` for what is open, `foam
memories` for what earlier sessions found. This file holds the rules.

## The language

An animation is one text file, `.acre`, that acre reads whole. The language is
declarative: it says what is on the canvas and how each thing changes over
time, and acre works out every frame from that.

The syntax is not settled. Jack settles it by writing files and reacting to
what they draw. Each version of the language stays small enough to build and
run before the next is designed. Still images come first and animation after.
The syntax uses braces and looks nothing like Python.

A file is a tree of braced nodes. `scene` gives each node kind and property its
meaning. The parser gives them none. A change of vocabulary then leaves the
parser alone. Record each change of syntax and its reason on the language
design issue in foam.

## Layers

The planned shape. Only the renderer deals in pixels.

- `lang` turns source text into a `Program`: tokens, then a syntax tree, with
  errors that name a line and column.
- `scene` turns a `Program` into a function of time: what every object is and
  where it is at any instant.
- `render` draws the scene at an instant into an image in memory.

`lang` and `scene` never touch a file or a screen, so they carry the unit tests,
and a second output format is one renderer.

The `acre` crate is a library. `acre-cli` and `acre-gui` are programs built on
it. A later viewer will be a third. `lang`, `scene` and `render` take values and
return values. Code that reads a file, writes to disk or opens a window goes in
a program crate, never in `acre`.

The library has no GUI dependencies. A program that depends on `acre` to draw
scenes gets no editor and no window.

## Branches

All working changes go on `dev`. Do not open a feature branch unless I ask for
one.

`main` carries releases and nothing else. Push to it when cutting a release,
and leave it alone the rest of the time.

## Releases

Each crate follows [semantic versioning](https://semver.org/spec/v2.0.0.html)
on its own. While a crate is below 1.0, a breaking change takes the minor
number. A change moves only the version of the crate it changes.

Tag every release, annotated, as `<crate>-vMAJOR.MINOR.PATCH`, on the commit
that was published. The one release before the split is tagged `v0.1.0`.

## Formatting

`rustfmt` is the format. Run `cargo fmt --all` before committing.

## Changelog

Each crate keeps a `CHANGELOG.md` in its own directory, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Anything that changes
what a user of that crate sees goes under `[Unreleased]` in the commit that
changes it.

## Temporary files

Scratch goes in `tmp-claude/` at the root, one directory per task, and never
into git.
