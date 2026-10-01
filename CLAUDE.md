# acre

A Rust command-line tool that reads an animation written in its own small
language and renders it to frames. Only the CLI exists so far: the language,
the scene model and the renderer are all still to be written.

Work and facts live in foam, not here: `foam ready` for what is open, `foam
memories` for what earlier sessions found. This file holds the rules.

## The language

An animation is one text file, `.acre`, that acre reads whole. The language is
declarative: it says what is on the canvas and how each thing changes over
time, and acre works out every frame from that. The syntax is not designed yet.
Design it in a foam issue before putting it in code.

## Layers

The planned shape. Only the renderer deals in pixels.

- `lang` turns source text into a `Program`: tokens, then a syntax tree, with
  errors that name a line and column.
- `scene` turns a `Program` into a function of time: what every object is and
  where it is at any instant.
- `render` draws the scene at an instant into an image, and writes a run of
  them to disk.

`lang` and `scene` never touch a file or a screen, so they carry the unit tests,
and a second output format is one renderer.

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
