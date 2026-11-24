# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `scene::edit::arrange_edits` works out the edits that move each of
  several nodes by an offset of its own, as one list of edits. An `at`
  that names another arranged node, or a thing inside one, gets its `DX
  DY` changed by the difference between the two offsets. A node inside
  another arranged node moves with it. It refuses what `move_edits`
  refuses. It also refuses an end, a marker or a brace that names a thing
  arranged by a different offset from its own node, since the node would
  change shape.
- `scene::edit::resize_edits` works out the edits that resize a box or an
  ellipse from one box to another, given by top-left and bottom-right
  corners. It writes the new `size`, or adds one to a node its text sizes.
  It moves `at` by as much as the point `place` names moves, so with
  `place center` a drag on one side keeps the side across put. It refuses
  what `move_edits` refuses, and a `size` from a `let`, a `( ... )` or a
  style.
- `scene::edit::vertex_edits` works out the edits that move one vertex of
  a polygon or a polyline. It refuses `points` from data, a `let` or a
  `( ... )`. A polyline's point written twice in a row is one vertex, as
  the polyline is drawn.
- `scene::edit::property_edits` works out the edits that set a property
  such as `fill` or `dash` on one or more nodes. It replaces a node's own
  value, or adds one before the node's first property, which then goes over
  a style's value. An empty value takes the node's own value away. It
  refuses a key the node's kind does not take, a node inside a `repeat`, a
  value from a `let` or `( ... )`, and an edit after which the file would
  no longer build.
- `scene::edit::property_values` reads a node's value of each of a list of
  keys its kind takes, and says whether the node, a style or the default
  sets it. For a `let` name it gives the values the `let` names.
- `scene::edit` works out the source edits that add, name, delete and copy
  whole nodes. `NewNode::text` writes a box, an ellipse, a text, a line,
  an arrow, a polygon or a polyline on one line, and `insert_edits` adds
  nodes at the end of the canvas, before its timeline. `fresh_name` picks
  a name such as `box1` that no word in the file and no `repeat` copy
  uses, and `name_edits` gives an unnamed node a name. `delete_edits`
  removes one or more nodes with their lines, and refuses while anything
  left in the file names one of them. `copy_text` gives nodes as text, and
  `paste_edits` adds that text moved by an offset, with names that clash
  with the file renamed along with the words that name them. Every result
  keeps the layout `acre fmt` writes.
- `Scene::origin` is where the source's 0 0 falls on the canvas. A canvas
  whose `size` has an `auto` moves it away from the top-left corner, so a
  viewer subtracts it from a canvas point to get source coordinates.
- `scene::edit::text_edits` works out the edit that sets a drawn node's
  text, and `scene::edit::text_of` reads that text as the file writes it,
  markup included. A node's text is its `text` on a box, an ellipse, a
  text, a math, a code or a caption, and its `label` on a line, an arrow,
  a sweep, a merge or a brace. The new text goes in as a string literal
  that reads back the same, with `"`, `\` and line breaks escaped. A
  `"""` string stays one when it can hold the new text. A node without the
  property gets one, as `move_edits` adds an `at`. `text_edits` refuses a
  text from a `let` or a style, a node inside a `repeat`, a node whose
  text the timeline sets, and the cells of a row, a matrix or a table.
- `lang::string_literal` writes a text as a `"..."` or `"""` literal that
  reads back as that text.
- `scene::edit::node_path` gives a node's place in a `Program`: its index
  among its siblings, and the index of each node around it.
  `scene::edit::node_span` gives the span of the node at such a path. With
  the two, a viewer finds a node again after an edit shifts its span.
- A row takes `cells DATA COL`, one cell per row of a data column. A table
  takes `row DATA COL...`, one table row per row of the data file, with the
  cell of each column COL in turn. A table may mix these with `row
  "TEXT"...` lines, and the rows keep the order of the lines. A table's
  cells from data take markup as a quoted string does.
- A `polygon` or `polyline` with `points DATA XCOL YCOL` takes `units SX
  SY` and `origin X Y`. A data point (X, Y) lands at ((X - OX) × SX, (Y -
  OY) × SY) in the group's coordinates. A negative unit turns that axis
  around. The defaults are `units 1 1` and `origin 0 0`, so a drawing
  without them draws as before. `units` or `origin` with numbers in
  `points` is an error.

- `scene::edit::move_edits` works out the edits to a file's text that move
  one or more drawn nodes by an offset in pixels. `scene::edit::apply` makes them. The
  offset goes onto the node's `at X Y`, onto the DX DY of an `at` from a
  named thing, and onto every number of `from`, `to` and `points`. A node
  with no `at` gets one before its first property, on that property's line
  or on a line of its own as the body is written. A number keeps the
  decimals it was written with. It takes more when the offset needs them,
  up to two, so an offset of 2.5 writes 2.5. The nodes move as one. A node
  inside another of them takes no edits of its own. An `at`, an end, a
  marker's `at` or a brace's `over` that names a moved node, or a node
  inside one, follows that node and takes no edit. For a node in a
  `repeat`, a child of a stack or grid, a node the timeline moves or scales
  or that sits in one, a value from a `let`, a `( ... )` or a style, and an
  end, a marker or a brace on a named thing that does not move,
  `move_edits` returns an error that says why.
- Text between single asterisks, `*like this*`, is set in italic sans. An
  opening `*` starts a word and a closing `*` ends one. Any other `*` is the
  character, so `2 * 3` and `a*b` stay as written, and so does a `*` in a
  code span. Code inside italic stays upright and italic inside bold stays
  bold, since acre has no italic mono or bold italic face. `\*` writes an
  asterisk.
- A child of a stack with `space shown` takes its space only once the
  timeline shows it. Layout still puts every child in its slot. Until the
  child's first `show` or `fade NAME in`, acre draws the children after it
  back by its size and the stack's gap, so the hidden child leaves no gap.
  As that action starts, they jump to their slots. A later `hide` does not
  give the space back. `space always` is the default, and a style can carry
  `space`. A node with `space shown` sits directly in a stack, and its first
  change of opacity in the timeline is a `show` or `fade NAME in`. A node placed with `at` from one of
  the children after it stays at that child's slot.
- Every drawn node but `caption` takes `tag "TEXT"` or `tag auto`. acre sets
  the tag 8 pixels left of the box around what the node draws, centered on
  it vertically. `tag auto` gives the next letter, (a), (b) and on, counting
  through the whole canvas in source order. A tag set by hand does not
  count. `tag-size`, `tag-color` and `tag-font` set its look, by default 14
  pixels, black and sans-bold, and a style can carry them. An `auto` canvas
  makes room for tags. When a timeline moves, scales or fades a node, its
  tag moves, scales and fades with it. A color change leaves the tag alone.
  `Object::tag` is new.
- `merge` joins several lines into one arrow. Each `from` starts a line,
  and `from` on a `merge` is the one property a node may set more than
  once. Each line sweeps into a junction, and an arrow runs on from the
  junction to `to`. `at X Y` sets the junction. Without `at`, the junction
  sits out of `to`'s side, or above `to` when `to` names no side, halfway to
  the nearest `from`. A line from an end with no side sets off the way the
  arrow runs. The arrow takes `head`, `head-size`, `label`, `label-side` and
  `label-at`, and `tail` draws at each `from`. `stroke`, `stroke-width` and
  `dash` set every line. When a timeline moves or scales the things the
  ends name, the lines follow, and `fade`, `show`, `hide` and `stroke` act
  on the whole merge. `Merge` and `Kind::Merge` are new.
- `label-side on` sets a connector's label centered on its line at
  `label-at`. acre breaks the line around the label's box and leaves on each
  side the gap that a label beside a line keeps from it. On an elbow the
  break falls on the segment that holds the label. The break moves with the
  line when a timeline moves or scales its ends. `LabelSide::On` is the new
  variant.
- `sweep` takes `label`, `label-side`, `label-at`, `text-size`, `text-color`
  and `font`, as `arrow` does.
- `at NAME SIDE F` puts a node at the point F of the way along a side of the
  thing NAME names. F runs from 0 to 1, left to right on the top and bottom
  and top to bottom on the left and right. Two numbers after F move the point.
  A row's cell takes `ROW N SIDE F`. A matrix's cell takes `MATRIX R C SIDE F`,
  which `at` did not take before. `from` and `to` take `SIDE F` as well, and
  so does a `marker`'s `at`. On an ellipse the point lies on the outline.
- `place` on a box, ellipse, row, matrix, code, text or math names the point
  of its own box that `at` puts down. It takes `center`, a side or a corner.
  Without `place`, a node sits by its top-left corner as before. Text and
  math sit by the point of their top edge that `align` names, as before.
  `anchor` still names the point a timeline scales about.
- `repeat INDEX FIRST LAST { NODE... }` draws its nodes once for each whole
  number from FIRST to LAST, both included. In each copy, INDEX stands for
  that number wherever a `let` name can, inside `( )` as well. A node named
  inside a repeat gets the number added to its name in each copy, so `b`
  becomes `b0`, `b1` and so on. Each word inside the repeat that names such a
  node gets the same number. A repeat inside another adds the outer number
  first, so for `i` 0 and `j` 1 the copy of `b` is `b01`. A last number below
  the first draws nothing. An index that has the name of a `let` or of an
  outer repeat's index is an error.
- `(NAME N)` stands for value N, counting from 0, of a `let` with several
  values. `fill (tints i);` takes the i-th color of `let tints a b c d;`.

- A `row` with `width W` and `weights N...` gives each cell a share of W in
  proportion to its weight, in order. The `gap` between cells comes out of
  each cell's share. `weights DATA COL` takes one weight per line of a column
  of a `data` file. A weighted row takes its height from `unit` and ignores
  the unit's width. `span` on the row or on one of its cells is an error. A
  count of weights that differs from the count of cells is an error. Weights
  are 0 or more, and at least one is above 0. `at`, `from`, `to`, `over` and
  the timeline's cell actions find each cell at its weighted place.
- A row-wide `fill`, `stroke` or `text-color` action in a timeline, such as
  `fill r #ffcc00`, changes every cell of the row. Each cell changes from
  its own color, so a cell with its own fill or a palette's fill blends
  from that fill. `stroke` skips cells with no outline, and `text-color`
  skips cells with no text. Before this, the action was an error.
- A timeline changes a node's text with `text NAME "TEXT"`, and the text of
  one cell of a row with `text ROW N "TEXT"`. The action works on a box, an
  ellipse, a text and a row's cell. It takes effect at an instant. A span is
  an error. A box or a text sets the new text with markup in its own font and
  color. A row's cell keeps its fill. Layout measures the node once, at the
  widest text the timeline gives it. A box without `size` keeps one size.
  The nodes after it in a stack keep their places. Arrows and braces meet the
  box where layout put it. `text-color` colors the new text too. This holds
  for a cell or a box that has no text until the timeline gives it some.
  `Object::texts` holds the changes, each a `TextChange`. `Kind::relabel`
  sets one. `Span` implements `Hash`.
- `matrix NAME { rows N; columns N; unit W H; }` draws a grid of equal cells.
  It takes `at` as a `row` does. `fill`, `stroke`, `stroke-width`, `dash`,
  `text-size`, `text-color` and `font` set every cell.
  `row-numbers FIRST STEP` writes a number left of each row, and
  `column-numbers FIRST STEP` writes one above each column. A child
  `cell { at R C; }` sets `text`, `fill`, `stroke`, `stroke-width`, `dash` or
  `text-color` for the cell at row R and column C, both counted from 0. A
  later `cell` at the same place overrides what an earlier one sets. A cell
  that sets its own outline draws after the others, so no neighbour covers
  its edge. `cells DATA RCOL CCOL TEXTCOL` sets the text of one cell per row
  of a data file. A `cell` child's `text` replaces it. A row or column
  outside the matrix is an error, and for `cells` the message gives the
  file's line. `from`, `to` and `over` name one cell as `NAME R C` and the
  whole matrix as `NAME`. A timeline moves, scales, fades, shows and hides
  the matrix as one and changes its colors, and an action on one cell is an
  error. `Kind::Matrix` holds it.
- A `data NAME { file "PATH"; }` node in the canvas names a data file. PATH
  is relative to the drawing's file. A `.tsv` file has tabs between cells,
  and a `.csv` file has commas. Quoted CSV cells are not supported. The first
  line names the columns. Every other line that is not blank is a row.
  `points DATA XCOL YCOL` on a `polygon` or a `polyline` takes one point per
  row. X comes from column XCOL and Y from column YCOL. A data name or a
  column that does not exist is an error. A row with fewer cells than the
  first line is an error. A cell that is not a number is an error, and its
  message gives the file's line and the column. A `data` draws nothing.
  `scene::data_files` lists the files a program reads. Each is a `DataFile`
  with a `name`, a `path` and a `span`.
- `polygon NAME { points X Y X Y ...; }` draws a closed shape through its
  points, and takes `fill`, `stroke`, `stroke-width` and `dash` as a `box`
  does. `polyline NAME { points X Y X Y ...; }` draws an open path through its
  points, and takes `head` and `tail` as a `line` does. A point is two numbers
  from the origin of the group the node sits in, and each number may be a
  `let` name or a `( ... )` expression. A polygon with fewer than three
  points, a polyline with fewer than two, or an odd count of numbers is an
  error. A timeline moves, scales, fades, shows and hides both kinds and
  changes their colors. `from`, `to`, `at` and `over` can name either kind,
  and take the box around its points. `Kind::Polygon` and `Kind::Polyline`
  hold them.
- A `section` line in a `timeline`, such as `section 4s "The types";`, names
  the part of the animation that starts at its time. A part runs until the
  next one starts. Two sections at one time, or a section at or past the end,
  is an error. `Scene::sections` lists them in order of start, each a
  `Section` with a `start` and a `title`. A `subsection` line, such as
  `subsection 7.5s "Masked";`, names a part of the latest section before it,
  and runs until the next subsection or section starts. A subsection before
  the first section, two at one time, or one at or past the end is an error.
  `Section::subsections` lists them, each a `Subsection` with a `start` and a
  `title`.
- A `timeline` node in the canvas animates named nodes. An entry is an
  instant, `2s { ... }`, or a span, `0s to 1s { ... }`. A span eases in and
  out unless it names `ease linear`, `ease in`, `ease out` or `ease in-out`.
  The actions are `scale NAME F`, `move NAME DX DY`, `fade NAME in|out|F`,
  `show NAME` and `hide NAME`, and `fill`, `stroke` and `text-color` with a
  node or a row's cell (`ROW N`) and a color. Each action takes one thing of a
  node from where it stands to its value. Two actions on one thing at once are
  an error. A node that a timeline first shows or fades in starts hidden.
  `acre reference timeline` lists the actions.
- `anchor` on a drawn node names the point of its box that stays put as it
  scales: `center` by default, a side such as `top`, or a corner such as
  `top-left`.
- A timeline action can name a `group`, `panel`, `stack` or `grid`. `fade`,
  `show`, `hide`, `move` and `scale` change it as one, with everything inside
  it. Opacity multiplies down the tree, and a move or a scale inside a moving
  group adds to the group's. A group scales about the point of its box that
  `anchor` names, and the four kinds now take `anchor`. A color action on a
  group is an error.
- A timeline action can name one cell of a row as `ROW N`, as in
  `move r 3 0 40`, `scale r 3 1.5`, `fade r 3 out`, `show r 3` and
  `hide r 3`. The cell's text and note move with it, and it scales about its
  center. A cell that is moved or scaled at an instant is drawn after the last
  row, above every cell that is not.
- Connectors and braces attached to a node inside a moved group, or to a moved
  cell, follow it. A row placed with `at ROW N` follows that cell.
- A scene's `duration` runs to the timeline's last change. `duration 5s` on the
  canvas lengthens it, and one shorter than the timeline is an error.
- `lang` reads a number with `s` straight after it, such as `1.5s`, as a time,
  `ValueKind::Time`. A node may open with a time in place of a kind, as in
  `0s to 1s { ... }`. Its `kind` is then empty, and the new `Node::head` holds
  the values before `{`.
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
- A connector's `from` and `to` take a corner after a name, `top-left`,
  `top-right`, `bottom-left` or `bottom-right`, as in
  `from a bottom-right; to b top-left;`. An ellipse's corner is where the
  diagonal of its box crosses it. A curve leaves a corner along the diagonal.
  An elbow leaves along whichever of the corner's two sides faces the other
  end more. An end on a corner follows the corner as its thing moves and
  scales.
- A `line` or an `arrow` whose `from` and `to` name the same thing, with no
  side, draws a loop out of one side of it and back in, clockwise. `side`
  picks the side, `top` by default. A straight or curved loop draws as a
  curve, and an elbow loop as three sides of a rectangle. `side` on a line
  that is not a loop is an error. `scene::Connector` has a `looped` field.
- A `line` or an `arrow` whose two ends meet at one point is an error. It
  used to draw nothing.
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
- `font mono-bold` sets text in JetBrains Mono NL Bold, which acre embeds
  beside the regular face. `Font::MonoBold` names it.
- When a run's font lacks a glyph for a character, `text::layout` sets that
  character in the first of sans, mono and the math font that has it, and
  lays it out with that font's advance. A font registered with `fonts` falls
  back the same way. `fonts::FALLBACK` holds the order, and `Fonts::cover`
  gives the font that sets a character.
- `Scene::missing_glyphs` lists each character in the scene's text that no
  font has.
- `head-size N` on `arrow`, `line` and `polyline` sets the length of a
  triangle or open head and the diameter of a dot, in pixels, at both ends.
  Without it a triangle stays `8 + 3 × stroke-width` long and a dot
  `5 + 3 × stroke-width` across. The shaft's length still caps both.
- `sweep { from ...; to ...; }` draws an arrow along one cubic. `leave DIR`
  and `enter DIR`, each `up`, `down`, `left` or `right`, set the way it
  travels at an end that is a point or a thing with no side. An end on a
  side travels out of that side or into it. An end with neither, or with
  both, is an error. Each handle reaches half the distance between the ends
  along its own way, so between `leave down` and `enter down` the curve only
  moves down. A handle whose other end lies behind or level with its way
  reaches half the whole length. `stroke`, `stroke-width`, `dash`, `head`,
  `tail` and `head-size` work as on `arrow`, and the head is a triangle
  unless set. `Route::Sweep` holds the ways.
- `Primitive::Cubic` is a cubic Bézier curve with two handles. The PNG, SVG
  and PDF renderers draw it as a curve, not as straight segments. A `sweep`
  draws its shaft as one `Cubic` that runs between the backs of its heads,
  so it meets each head at the middle of its back. Each head points along
  the sweep's way at its end. `arrow { route curve; }` still draws 24
  straight segments.

### Changed

- `Scene` has a public field `origin`, so code that builds a `Scene` by
  hand sets it, to `Point::ZERO` for a canvas of fixed size.

- Any node may keep values before `{`. When they are not one bare word,
  `lang::parse` keeps them in `Node::head`, as it did for a node that opens
  with a time. The error for two names before `{`, or for a name that is not
  a bare word, now comes from `scene` and no longer from `lang::parse`.

- `Row` holds the row's height in `height`, where it held `unit`. `RowCell`
  holds a cell's place in pixels from the row's left edge in `left` and
  `width`, where it held `start`. `Row::width` gives the width of the whole
  row.
- A `row` is as tall as its tallest note needs. When a note would reach past
  the bottom of `unit`, every cell of the row grows until the note fits. A
  stack places the next node below the taller row, and braces and arrows meet
  its new edge.
- In a `row` where any cell has a note, every cell's text sits where the text
  of a cell with a note sits, so the texts of the row line up. `Row` has a new
  field, `noted`, that records this.
- `scene::build` takes a third argument, the bytes of each file
  `scene::data_files` lists, in that order.
- At the time of its first keyframe, a `Track` takes the last keyframe at that
  time, as it already did at any later time. An instant at the start of a
  track takes effect at that instant.
- A `let` may not take the name `in`, `out`, `to`, `ease`, `linear`, `in-out`,
  `top-left`, `top-right`, `bottom-left` or `bottom-right`.
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

- An `Object` holds a `scale` track, the `pivot` it scales about, an `offset`
  track that moves it from where layout put it, and `paints`, the colors that
  change over time. `Scene::draw` moves and scales an object's primitives with
  `Primitive::transform`, its stroke widths, dashes, radii and text sizes
  included, so every renderer draws the change.
- A `Connector` keeps the thing each end is attached to in `follow`, and a
  `Brace` keeps the thing it is over. `Scene::draw` puts the end on the
  attached thing's moved and scaled edge at every instant. A drawing with no
  motion draws as before.
- `Kind::draw` draws a kind where layout put it. It replaces `Object::draw`,
  since only the scene knows where an object's attached things are.
- An `Object` holds `groups`, the `Animation` of each animated group around
  it, innermost first, and `cells`, the `Animation` of each row cell that
  moves on its own. `Object::motion` includes the groups. `Motion::then`
  composes two motions, and `Row::draw_cell` draws one cell.
- `Follow::target` and `Brace::follow` are a `scene::Target`, the span of a
  node and an optional row cell. `Row` has a `follow` field.

### Removed

- The `acre` command. It now comes from the `acre-cli` crate, and this crate is
  a library only.

### Fixed

- A property added to a node whose body holds only spaces, such as
  `group { }`, now replaces the spaces, so the result keeps the layout
  `acre fmt` writes.
- `brace { over a b; }` spans the box around every thing named after `over`.
  It used to cover `a` alone. A brace over several things follows the first
  as it moves. A word after `over` that names nothing is an error.
- A `ruler` with `side left` or `side right` runs down from `at`, with its
  ticks and numbers on that side. It used to draw the horizontal scale of
  `side top`.
- acre sets a code span inside `**bold**` markup in bold mono. It used to set
  it in regular mono, like a code span outside bold.

## [0.1.0] - 2026-09-30

### Added

- The `acre` command, with `check <file>` and `render [--out <dir>] <file>`.
  Both name their arguments and exit with "not written yet": this release
  holds the crate name and nothing renders.

[Unreleased]: https://github.com/jackroddy/acre/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jackroddy/acre/releases/tag/v0.1.0
