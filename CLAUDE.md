# acre

A Rust command-line tool that reads an animation written in its own small
language and renders it to frames. The CLI and the scene model exist; the
language and the renderer are still to be written.

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

acre is a library first. The `acre` binary is one program built on it. A later
program will be a viewer that draws scenes. `lang`, `scene` and `render` take
values and return values. Code that reads a file, writes to disk or opens a
window goes in the binary.

The GUI sits on top of the library, never inside it. A program that depends on
acre to draw scenes does not get the editor or a window with it.

## Branches

All working changes go on `dev`. Do not open a feature branch unless I ask for
one.

`main` carries releases and nothing else. Push to it when cutting a release,
and leave it alone the rest of the time.

## Releases

acre follows [semantic versioning](https://semver.org/spec/v2.0.0.html). While
the crate is below 1.0, a breaking change takes the minor number.

Tag every release, annotated, as `vMAJOR.MINOR.PATCH`, on the commit that was
published.

## Formatting

`rustfmt` is the format. Run `cargo fmt` before committing.

## Changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Anything that changes what someone running the tool sees goes under
`[Unreleased]` in the commit that changes it.

## Temporary files

Scratch goes in `tmp-claude/` at the root, one directory per task, and never
into git.
