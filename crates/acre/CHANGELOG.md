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
- A `row` node draws a run of cells on a grid of equal units. Each cell covers
  a whole number of units. `cells` lists texts, and a `cell` child adds a cell
  with its own span, fill, stroke or note.
- A `palette` node maps cell texts to fills and notes. A row names one with
  `palette`, and `notes show` writes each note under its cell, one character to
  a unit.
- A `brace` node draws a curly or square bracket with a label. `over` places it
  along a shape, a row, or a run of a row's cells.
- A `ruler` node draws a numbered scale of equal units.
- `from` and `to` on a line or an arrow can name a row's cell by index, and a
  side of a shape or cell.
- `font` picks `mono`, `sans`, `sans-bold` or `sans-italic` for a node's text.
  On the canvas it sets the default. The sans faces are Noto Sans.
- `dash` on a stroke and `radius` on a box. A `\n` in a string starts a new line.
- `width N` on `text`, and on a box's text, wraps lines at spaces to fit N
  pixels. A box without `size` takes the size of its text plus `padding`.
  `text-align` sets the text in a box from the top, inside the padding, at the
  left, center or right.
- A `panel` node is a section with a heading. Panels number themselves in
  source order, and `number none` leaves one out. What a panel holds goes
  under its heading. With a `stroke` or a `fill`, a panel is a card: its `at`
  is the card's corner, the heading and contents sit inside `padding`, and
  `width` and `height` set the card's least size.
- A `caption` node sets text under the panel it sits in, or under everything
  before it.
- A style named after a node kind, such as `style panel { ... }`, applies to
  every node of that kind.
- A `stack` node places its children one after another, down or to the right,
  with a `gap`, and `align` sets where a narrower child sits. A `grid` node
  places its children row by row in `columns`, each column as wide as its
  widest child and each row as tall as its tallest. Both are optional: `at`
  works as before everywhere else. A child's lines, arrows and braces take room
  when everything they name is placed already, and none otherwise.
- `at` is optional. Without it, a node sits at its group's origin, or at its
  slot in a stack or grid.
- A string can run over several lines between `"""` and `"""`. It keeps its line
  breaks, drops the indent its lines share and a break straight after the
  opening, and takes no escapes. Markup still applies where it is a label.
- A `code` node draws source code on a background, highlighted for `rust`,
  `python` or `acre`, or plain as `text`. It takes `line-numbers show`,
  `emphasize` for lines to tint, and a color for each kind of token. Its lines
  are named from 1, so `arrow { from snippet 7 right; ... }` and
  `brace { over snippet 2 4; ... }` work as they do for a row's cells.
- `let NAME VALUES;` on the canvas names a list of values, and the bare word
  `NAME` stands for them in any later property.
- A `style` node names a set of properties. `style NAME;` gives a node each one
  it does not set itself.
- A `group` node moves the nodes inside it by its `at`.
- `at NAME dx dy` places a node relative to the top-left corner of a shape, a
  row or a row's cell.
- `chars "..."` on a row adds one cell per character.
- `repeat N` on a line or an arrow draws it N times, with each named cell one
  further along each time.
- A palette `symbol` may give a text color after its note.
- A number may be an arithmetic expression in parentheses, such as
  `(64 * u + 76)`, over numbers and `let` names. A minus between two terms
  needs spaces around it.
- Text marks up its runs. `` `code` `` sets mono, `**bold**` sets bold sans, and
  `{NAME text}` colors text with a `let` color or `{#rrggbb text}`. A backslash
  before one of these marks takes it as it is.
- A `table` node sets rows of strings in columns, each aligned to its own x.
- A `let` may not use a word acre reads itself, such as `left` or `none`.
- `lang::format` rewrites source in one layout: four spaces of indent per
  brace, one space between tokens, none inside `( )`, at most one blank line.
  It keeps comments and the line breaks between items.
- `render::draw_view` draws the part of a scene that falls in a pixmap of any
  size, at any scale and offset, clipped to the canvas.
- Each `scene::Object` carries `source`, the span of the node that made it.
  `Scene::bounds` gives the box around an object, and `Scene::object_at` the
  smallest object under a point.
- `scene::schema` lists every node kind, the properties each takes, the values
  each property takes, and what each is for. The builder checks nodes against
  it.
- `lang::context_at` gives the nodes around a byte offset, the property it sits
  in, and the word under it, reading past errors.
- `size W auto`, `size auto H` and `size auto auto` on the canvas take each
  `auto` side from the box around everything drawn, as `Scene::bounds`
  measures it. A new canvas property `margin` sets the space added on both
  ends of an `auto` side, 20 by default. The drawing moves to start at the
  margin on each `auto` axis.
- acre embeds Noto Sans Math. `math::MathFont` holds a font with an OpenType
  MATH table, and `MathFont::noto` returns the embedded one. `Font::Math`
  names it.
- `math::parse` reads a formula in a subset of LaTeX math into a list of atoms,
  each with a class that sets the space around it.
- `math::layout` sets a formula into glyphs and rules by the rules of *The
  TeXbook*, appendix G, with the constants and glyph variants of the font's
  MATH table. A run in `Font::Math` holds a formula, and `text::layout` sets
  it in its line as one piece that never breaks.

### Changed

- Text is shaped with rustybuzz, so kerning and other font rules apply. The new
  `text` module lays out and measures text for both `scene` and `render`.
  `Label::size` gives a label's measured size, and `Scene::bounds` measures
  text instead of estimating it.
- `lang::highlight` splits strings at their markup, marks the words a `let`
  defines as `Highlight::Name`, and gives each color, and each `let` name that
  stands for one, as `Highlight::Color` with its value.
- `render::draw_frame` takes a scale, the number of image pixels to each canvas
  pixel.
- A line of wrapped text breaks at a space outside a mono code span when the
  line has one. It breaks at a space inside a span only when the span alone is
  wider than the width.
- `text::Glyph` holds a `text::Mark` in place of its font and glyph id. A mark
  is a glyph's outline at its own size, or a filled rule such as a fraction
  bar.

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
