# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `render::draw_svg` writes a scene as SVG, and `render::draw_pdf` writes it
  as a one-page PDF. Both draw the same primitives as the PNG renderer and put
  text where it does. `draw_svg` writes text as glyph outlines, so a viewer
  needs no font. `draw_pdf` embeds a subset of each font the scene uses, so a
  reader can select and copy the text. A canvas pixel is a CSS pixel in both,
  so a PDF page is 0.75 points to the pixel.
- Each glyph in a `text::Layout` has the bytes of `Layout::text` it sets.
- A `fonts` node in the canvas registers font files under names that `font`
  accepts, in the canvas, on any node and in a style. `math "PATH"` in it sets
  formulas in another math font, which needs an OpenType MATH table. The
  library reads no file. `scene::font_files` lists the files a program names,
  and `scene::build` takes their bytes. A scene holds its `Fonts`, which the
  text measure, the math layout and the renderer all use.
- A `marker` node draws a number in a filled circle. `at` takes a point, a
  named thing (the circle sits at its center) or a named thing's side (the
  circle sits outside that side, touching it). `number` on a `text` puts the
  same badge to the left of the text. `style marker` sets the fill, text color,
  text size, font and `diameter` of both.
- `route` on a `line` or an `arrow` is `straight`, `elbow` or `curve`. An end
  given a side (`from build left`) leaves or enters along it. An elbow between
  perpendicular sides turns once, and between parallel sides it turns halfway,
  or steps out past both when they face the same way. A curve's handles run out
  of each side. An end with no side follows the longer distance between the
  ends: an elbow turns halfway along it, and a curve leaves along it.
- `head` and `tail` on a `line` or an `arrow` are `none`, `triangle`, `open` or
  `dot`, at the end and the start. An `arrow` still has a triangle head and a
  `line` none.
- `label-side` (`above`, `left` or `right`) and `label-at` (a fraction from 0
  to 1) place a connector's label beside the line or part way along it.
- `text-align justify` on `text` and on a box's text sets each wrapped line
  flush with both edges. It needs `width`. acre chooses a justified paragraph's
  breaks to keep the total looseness of its lines low, so the lines come out
  about the same length. Only the spaces outside code spans change width. They
  widen, or narrow to two thirds of their width. A last line shorter than a
  third of `width` counts against a set of breaks, so acre moves a word or two
  down to it unless that leaves the lines above too loose. The last line of a
  paragraph, a line that ends at `\n` and a line of one word keep their natural
  spacing. Ragged text breaks as before.
- A line of a label's text that starts with `- ` is a list item. acre draws a
  bullet in the text's color and hangs the item's wrapped lines under its
  first word. `width` includes the indent. A list item sits 0.3 of the text
  size lower than a plain line would. The line after a list does too. With
  `text-align justify` each item justifies on its own. A line that starts
  with `\- ` starts with a dash, written `"\\- "` in a quoted string. Text with
  no such line draws as before.
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
- `$...$` in any label's markup is a formula in LaTeX math, such as
  `"takes $\lceil \log_2 5 \rceil$ bits"`. Backslashes inside it need no
  escape. `\$` is a dollar sign, and a `$` with no partner is an error.
- A `math` node sets one formula in display style, placed by its top edge like
  `text`: `math { text """\sum_{i=1}^n i = \frac{n(n+1)}{2}"""; size 24; }`.

### Changed

- `text::Glyph` is `Clone` and no longer `Copy`, since it holds a range.
- Text is shaped with rustybuzz, so kerning and other font rules apply. The new
  `text` module lays out and measures text for both `scene` and `render`.
  `Label::size` gives a label's measured size, and `Scene::bounds` measures
  text instead of estimating it.
- `lang::highlight` keeps a formula between `$` signs as one string, with the
  `$` signs as markup.
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
- A connector's label sits beside the line, clear of the stroke, on any
  slope. A label on a vertical or diagonal line no longer overlaps it. A
  horizontal line's label is where it was.
- `scene::Connector` holds a `route`, a `head` and a `tail` of type `Head`, and
  a `label_side` and `label_at`. `head` was a `bool`.
- `text::layout` takes a `justify` argument. `Primitive::Text` and
  `scene::Label` have a `justify` field. `text::Layout` has a `block` field.
  It is true for justified text and for text with a list item.
  `Layout::place` gives the lines of a block one left edge.
- `scene::Run` has a `bullet` field. It is true for the bullet of a list item.
  `text::Line` has a `gap` field. It holds the space above the line beyond one
  line height.

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
