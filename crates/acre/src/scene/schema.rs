//! Every node kind a canvas holds and the properties each takes, with
//! what each value is. The builder checks nodes against this table, and
//! an editor can list it.

/// One kind of node.
#[derive(Debug)]
pub struct NodeKind {
    pub name: &'static str,
    pub about: &'static str,
    pub properties: &'static [Property],
    /// The kinds of node it holds.
    pub children: &'static [&'static str],
}

/// One property of a node kind.
#[derive(Debug)]
pub struct Property {
    pub name: &'static str,
    /// The values it takes, as a reader would write them.
    pub takes: &'static str,
    pub about: &'static str,
}

impl NodeKind {
    /// The names of its properties.
    pub fn keys(&self) -> Vec<&'static str> {
        self.properties.iter().map(|p| p.name).collect()
    }

    pub fn property(&self, name: &str) -> Option<&'static Property> {
        self.properties.iter().find(|p| p.name == name)
    }
}

/// The node kind named `name`.
pub fn node(name: &str) -> Option<&'static NodeKind> {
    NODES.iter().find(|n| n.name == name)
}

const fn prop(name: &'static str, takes: &'static str, about: &'static str) -> Property {
    Property { name, takes, about }
}

const AT: Property = prop(
    "at",
    "X Y | NAME [SIDE F] [DX DY] | ROW N [SIDE F] [DX DY] | MATRIX R C SIDE F [DX DY]",
    "where `place` puts the node: a point in the group's coordinates, or from a named thing's \
     top-left corner or the point F of the way along its SIDE, left to right or top to bottom; \
     without it, the group's origin, or the node's slot in a stack or grid",
);
const AT_CORNER: Property = prop(
    "at",
    "X Y | NAME [SIDE F] [DX DY] | ROW N [SIDE F] [DX DY] | MATRIX R C SIDE F [DX DY]",
    "the top-left corner, in the group's coordinates, or from a named thing's top-left corner \
     or the point F of the way along its SIDE, left to right or top to bottom; \
     without it, the group's origin, or the node's slot in a stack or grid",
);
const PLACES: &str =
    "center | top | bottom | left | right | top-left | top-right | bottom-left | bottom-right";
const PLACE: Property = prop(
    "place",
    PLACES,
    "the point of its box that `at` puts down; the default is top-left",
);
const TEXT_PLACE: Property = prop(
    "place",
    PLACES,
    "the point of its box that `at` puts down; the default is the top edge where `align` says",
);
const FILL: Property = prop("fill", "COLOR | none", "the inside color");
const STROKE: Property = prop("stroke", "COLOR | none", "the outline color");
const STROKE_WIDTH: Property = prop("stroke-width", "N", "the outline width in pixels");
const DASH: Property = prop(
    "dash",
    "ON OFF",
    "dash and gap lengths for a dashed outline",
);
const RADIUS: Property = prop("radius", "N", "the radius of rounded corners");
const TEXT_SIZE: Property = prop("text-size", "N", "the size of the text in pixels");
const TEXT_COLOR: Property = prop("text-color", "COLOR", "the color of the text");
const FONT: Property = prop(
    "font",
    "mono | mono-bold | sans | sans-bold | sans-italic | NAME",
    "the text's font; a NAME is one `fonts` registers",
);
const LABEL: Property = prop("label", "\"TEXT\"", "text beside it, with markup");
const SIDE: Property = prop("side", "top | bottom | left | right", "which way it faces");
const FROM: Property = prop(
    "from",
    "X Y | NAME [SIDE [F] | CORNER] | ROW N [SIDE [F] | CORNER] | MATRIX R C [SIDE [F] | CORNER]",
    "where it starts: a point, or the edge of a named thing; a SIDE is top, bottom, left or right and puts it at that side's middle, or with F at the point F of the way along it, left to right or top to bottom; a CORNER is top-left, top-right, bottom-left or bottom-right",
);
const TO: Property = prop(
    "to",
    "X Y | NAME [SIDE [F] | CORNER] | ROW N [SIDE [F] | CORNER] | MATRIX R C [SIDE [F] | CORNER]",
    "where it ends",
);
const STYLE: Property = prop("style", "NAME...", "the styles whose properties it takes");
const ANCHOR: Property = prop(
    "anchor",
    "center | top | bottom | left | right | top-left | top-right | bottom-left | bottom-right",
    "the point of its box that stays put when a timeline scales it; the default is center",
);

const POINTS: Property = prop(
    "points",
    "X Y X Y ... | DATA XCOL YCOL",
    "the points in turn, in the group's coordinates; or one per row of the `data` named DATA, X from column XCOL and Y from column YCOL",
);

const SHAPE: &[Property] = &[
    AT,
    PLACE,
    prop(
        "size",
        "W H",
        "the width and height; without it, the size of the text and its padding",
    ),
    FILL,
    STROKE,
    STROKE_WIDTH,
    DASH,
    RADIUS,
    prop("text", "\"TEXT\"", "text inside, with markup"),
    TEXT_SIZE,
    TEXT_COLOR,
    FONT,
    prop("width", "N", "the width the text wraps within, at spaces"),
    prop(
        "padding",
        "N | X Y",
        "the space between the outline and the text",
    ),
    prop(
        "text-align",
        "left | center | right | justify",
        "the text from the top inside the padding; without it, the text is centered; justify stretches or shrinks the spaces so each wrapped line fills `width`, all but the last line of a paragraph",
    ),
    ANCHOR,
    STYLE,
];

const HEAD_SIZE: Property = prop(
    "head-size",
    "N",
    "the length of a triangle or open head and the diameter of a dot, in pixels; without it, a triangle is 8 + 3 × stroke-width long and a dot 5 + 3 × stroke-width across",
);

const LABEL_SIDE: Property = prop(
    "label-side",
    "above | left | right | on",
    "which side of the line the label sits on; left and right are as seen going from `from` to `to`, and above is the side that faces up; on centers it on the line, which breaks around it, leaving the same gap a label beside the line keeps",
);

const LABEL_AT: Property = prop(
    "label-at",
    "F",
    "how far along the line the label sits, from 0 at `from` to 1 at `to`; the default is 0.5",
);

const SWEEP: &[Property] = &[
    FROM,
    TO,
    prop(
        "leave",
        "up | down | left | right",
        "the way it sets off from `from`, when `from` is a point or a thing with no side; a side sets off out of that side",
    ),
    prop(
        "enter",
        "up | down | left | right",
        "the way it arrives at `to`, when `to` is a point or a thing with no side; a side is entered from outside",
    ),
    STROKE,
    STROKE_WIDTH,
    DASH,
    prop(
        "head",
        "none | triangle | open | dot",
        "what it draws at `to`; a triangle unless set",
    ),
    prop(
        "tail",
        "none | triangle | open | dot",
        "what it draws at `from`",
    ),
    HEAD_SIZE,
    LABEL,
    LABEL_SIDE,
    LABEL_AT,
    TEXT_SIZE,
    TEXT_COLOR,
    FONT,
    ANCHOR,
    STYLE,
];

const CONNECTOR: &[Property] = &[
    FROM,
    TO,
    prop(
        "repeat",
        "N",
        "draw it N times, each named cell one further along",
    ),
    STROKE,
    STROKE_WIDTH,
    DASH,
    prop(
        "route",
        "straight | elbow | curve",
        "the path from `from` to `to`; an elbow turns at right angles and a curve bends smoothly; an end given a side leaves or enters along it, so an elbow between perpendicular sides is an L, and an end with no side follows the longer axis; a curve leaves a corner along its diagonal, and an elbow along whichever of the corner's two sides faces the other end more",
    ),
    prop(
        "head",
        "none | triangle | open | dot",
        "what it draws at `to`; an `arrow` has a triangle and a `line` has none",
    ),
    prop(
        "tail",
        "none | triangle | open | dot",
        "what it draws at `from`",
    ),
    HEAD_SIZE,
    LABEL,
    LABEL_SIDE,
    LABEL_AT,
    prop(
        "side",
        "top | bottom | left | right",
        "the side a loop leaves and comes back into, when `from` and `to` name one thing with no side; the default is top, and a straight loop draws as a curve",
    ),
    TEXT_SIZE,
    TEXT_COLOR,
    FONT,
    ANCHOR,
    STYLE,
];

const DRAWN: &[&str] = &[
    "box", "ellipse", "polygon", "polyline", "row", "matrix", "line", "arrow", "sweep", "text",
    "math", "marker", "table", "brace", "ruler", "group", "panel", "caption", "stack", "grid",
    "code", "repeat",
];

pub const NODES: &[NodeKind] = &[
    NodeKind {
        name: "canvas",
        about: "the drawing: its size, and everything on it",
        properties: &[
            prop(
                "size",
                "W H",
                "the width and height in pixels; `auto` for either takes the box around everything drawn",
            ),
            prop(
                "margin",
                "N",
                "the space around the drawing when `size` has an `auto`, default 20",
            ),
            prop("background", "COLOR", "the color behind everything"),
            prop(
                "duration",
                "Ts",
                "how long the animation runs, in seconds such as `4s`; at least until the timeline's last change, which is the default",
            ),
            FONT,
            prop(
                "let",
                "NAME VALUES...",
                "a name that stands for its values in any later property; `(NAME N)` stands for value N of several, counting from 0",
            ),
        ],
        children: &[
            "box", "ellipse", "polygon", "polyline", "row", "matrix", "line", "arrow", "sweep",
            "text", "math", "marker", "table", "brace", "ruler", "group", "panel", "caption",
            "stack", "grid", "code", "repeat", "palette", "style", "fonts", "data", "timeline",
        ],
    },
    NodeKind {
        name: "box",
        about: "a rectangle, with optional text inside",
        properties: SHAPE,
        children: &[],
    },
    NodeKind {
        name: "ellipse",
        about: "an ellipse filling the box at `at` of `size`",
        properties: SHAPE,
        children: &[],
    },
    NodeKind {
        name: "row",
        about: "cells side by side on a grid of equal units, or sharing a width by weight",
        properties: &[
            AT,
            PLACE,
            prop(
                "unit",
                "W H",
                "the width of one unit and the row's height; with `weights`, only the height counts",
            ),
            prop(
                "span",
                "N",
                "how many units each cell covers; not with `weights`",
            ),
            prop(
                "width",
                "N",
                "the width the cells share by `weights`, gaps included",
            ),
            prop(
                "weights",
                "N... | DATA COL",
                "one weight per cell, in order, or one per row of a data column; each cell's share of `width` is its weight over the sum",
            ),
            prop("gap", "N", "the space between cells"),
            RADIUS,
            prop("cells", "\"TEXT\"...", "one cell per string"),
            prop("chars", "\"TEXT\"", "one cell per character"),
            prop(
                "palette",
                "NAME",
                "the palette that fills cells by their text",
            ),
            prop(
                "notes",
                "show | hide",
                "whether the palette's notes show under cells",
            ),
            FILL,
            STROKE,
            STROKE_WIDTH,
            DASH,
            TEXT_SIZE,
            TEXT_COLOR,
            prop("note-size", "N", "the size of notes"),
            prop("note-color", "COLOR", "the color of notes"),
            FONT,
            prop(
                "note-font",
                "mono | mono-bold | sans | sans-bold | sans-italic",
                "the notes' font",
            ),
            ANCHOR,
            STYLE,
        ],
        children: &["cell"],
    },
    NodeKind {
        name: "matrix",
        about: "a grid of equal cells, addressed by row and column from 0 as `NAME R C`",
        properties: &[
            AT,
            PLACE,
            prop("rows", "N", "how many rows"),
            prop("columns", "N", "how many columns"),
            prop("unit", "W H", "the width and height of one cell"),
            prop(
                "row-numbers",
                "FIRST STEP",
                "a number left of each row, FIRST beside row 0 and growing by STEP",
            ),
            prop(
                "column-numbers",
                "FIRST STEP",
                "a number above each column, FIRST over column 0 and growing by STEP",
            ),
            prop(
                "cells",
                "DATA RCOL CCOL TEXTCOL",
                "text for cells from the `data` named DATA, one cell per row: row from column RCOL, column from column CCOL, and the text from column TEXTCOL",
            ),
            FILL,
            STROKE,
            STROKE_WIDTH,
            DASH,
            TEXT_SIZE,
            TEXT_COLOR,
            FONT,
            ANCHOR,
            STYLE,
        ],
        children: &["cell"],
    },
    NodeKind {
        name: "cell",
        about: "one more cell at the end of a row, or the look of one cell of a matrix",
        properties: &[
            prop(
                "at",
                "R C",
                "the row and column, from 0, of a matrix's cell; a later cell at one place overrides what an earlier one sets",
            ),
            prop("text", "\"TEXT\"", "the cell's text"),
            prop(
                "note",
                "\"TEXT\"",
                "small text under the cell's text, in a row",
            ),
            prop(
                "span",
                "N",
                "how many units it covers, in a row without `weights`",
            ),
            FILL,
            STROKE,
            STROKE_WIDTH,
            DASH,
            TEXT_COLOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "polygon",
        about: "a closed shape through its points, filled inside",
        properties: &[POINTS, FILL, STROKE, STROKE_WIDTH, DASH, ANCHOR, STYLE],
        children: &[],
    },
    NodeKind {
        name: "polyline",
        about: "an open path through its points",
        properties: &[
            POINTS,
            STROKE,
            STROKE_WIDTH,
            DASH,
            prop(
                "head",
                "none | triangle | open | dot",
                "what it draws at the last point",
            ),
            prop(
                "tail",
                "none | triangle | open | dot",
                "what it draws at the first point",
            ),
            HEAD_SIZE,
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "line",
        about: "a line between two points or things, straight unless `route` says otherwise",
        properties: CONNECTOR,
        children: &[],
    },
    NodeKind {
        name: "arrow",
        about: "a line with a head at `to`",
        properties: CONNECTOR,
        children: &[],
    },
    NodeKind {
        name: "sweep",
        about: "an arrow along one curve that sets off and arrives in set ways, and moves each way for half the distance between its ends along that way",
        properties: SWEEP,
        children: &[],
    },
    NodeKind {
        name: "text",
        about: "text placed by its top edge",
        properties: &[
            AT,
            TEXT_PLACE,
            prop("text", "\"TEXT\"", "the text, with markup"),
            prop("size", "N", "the size in pixels"),
            prop("color", "COLOR", "the color"),
            prop(
                "align",
                "left | center | right",
                "where `at` sits along the text",
            ),
            prop("width", "N", "the width the text wraps within, at spaces"),
            prop(
                "text-align",
                "justify",
                "stretch or shrink the spaces so each wrapped line fills `width`, all but the last line of a paragraph",
            ),
            prop(
                "rotate",
                "DEGREES",
                "the angle the text turns about `at`, counterclockwise",
            ),
            prop(
                "number",
                "N",
                "a badge with this number to the left of the text, drawn as `style marker` sets; the text is aligned left",
            ),
            FONT,
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "marker",
        about: "a number in a filled circle; `style marker` sets the look of every marker and of numbered `text`",
        properties: &[
            prop(
                "at",
                "X Y | NAME [SIDE [F]] | ROW N [SIDE [F]]",
                "the center: a point or the center of a named thing; with a side, the circle sits outside the side's middle, or the point F of the way along it, touching it",
            ),
            prop("number", "N", "the number in the circle"),
            prop("diameter", "N", "the width of the circle, default 20"),
            FILL,
            TEXT_SIZE,
            TEXT_COLOR,
            FONT,
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "math",
        about: "a formula in LaTeX math, set in display style and placed by its top edge",
        properties: &[
            AT,
            TEXT_PLACE,
            prop(
                "text",
                "\"\"\"FORMULA\"\"\"",
                "the formula, such as `\\frac{n}{2}`, without `$` signs",
            ),
            prop("size", "N", "the size in pixels"),
            prop("color", "COLOR", "the color"),
            prop(
                "align",
                "left | center | right",
                "where `at` sits along the formula",
            ),
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "table",
        about: "rows of strings in aligned columns",
        properties: &[
            AT_CORNER,
            prop("columns", "X...", "the x of each column"),
            prop(
                "align",
                "(left | center | right)...",
                "each column's alignment",
            ),
            prop("row-height", "N", "the distance between rows"),
            prop("size", "N", "the text size"),
            prop("color", "COLOR", "the text color"),
            FONT,
            prop(
                "row",
                "\"TEXT\"...",
                "one row, a string per column; repeat for each row",
            ),
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "brace",
        about: "a curly or square bracket with a label",
        properties: &[
            prop(
                "over",
                "NAME... | ROW [N [M]]... | MATRIX R C...",
                "the things, rows, runs of cells or matrix cells it brackets; with more than one, it spans the box around them all and follows the first",
            ),
            prop("from", "X Y", "where it starts, without `over`"),
            prop("to", "X Y", "where it ends, without `over`"),
            SIDE,
            prop("depth", "N", "how far the tip stands out"),
            prop("gap", "N", "the space between it and what it brackets"),
            prop("shape", "curly | square", "its shape"),
            STROKE,
            STROKE_WIDTH,
            LABEL,
            TEXT_SIZE,
            TEXT_COLOR,
            FONT,
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "ruler",
        about: "a scale of equal units with numbers",
        properties: &[
            AT_CORNER,
            prop("unit", "N", "the length of one unit"),
            prop("count", "N", "how many units"),
            prop("first", "N", "the number over the first unit"),
            prop("step", "N", "how much each number grows"),
            prop("every", "N", "a number over every Nth unit"),
            prop(
                "side",
                "top | bottom | left | right",
                "where the numbers sit; a top or bottom ruler runs right from `at` with its ticks pointing away from the numbers, and a left or right ruler runs down with its ticks and numbers on that side",
            ),
            STROKE,
            STROKE_WIDTH,
            TEXT_SIZE,
            TEXT_COLOR,
            FONT,
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "group",
        about: "nodes moved together by `at`",
        properties: &[
            prop(
                "at",
                "X Y | NAME [SIDE F] [DX DY]",
                "where the group's origin sits",
            ),
            ANCHOR,
        ],
        children: DRAWN,
    },
    NodeKind {
        name: "panel",
        about: "a numbered section with a heading; `stroke` or `fill` draws it as a card",
        properties: &[
            prop(
                "at",
                "X Y | NAME [SIDE F] [DX DY]",
                "the top-left corner of the frame, or of the heading when there is no frame",
            ),
            prop("title", "\"TEXT\"", "the heading, with markup"),
            prop("number", "none", "leave the panel out of the numbering"),
            prop("title-size", "N", "the heading's size"),
            prop("title-color", "COLOR", "the heading's color"),
            prop(
                "title-font",
                "mono | mono-bold | sans | sans-bold | sans-italic",
                "the heading's font",
            ),
            prop("number-color", "COLOR", "the color of the panel's number"),
            prop(
                "gap",
                "N",
                "the space between the heading and what the panel holds",
            ),
            STROKE,
            STROKE_WIDTH,
            DASH,
            FILL,
            prop(
                "padding",
                "N",
                "the space between the frame and what it holds",
            ),
            prop("width", "N", "the least width of the frame"),
            prop("height", "N", "the least height of the frame"),
            RADIUS,
            ANCHOR,
            STYLE,
        ],
        children: DRAWN,
    },
    NodeKind {
        name: "stack",
        about: "children placed one after another, each from where the one before ends",
        properties: &[
            prop(
                "at",
                "X Y | NAME [SIDE F] [DX DY]",
                "where the first child's slot starts",
            ),
            prop("direction", "down | right", "which way the children go"),
            prop("gap", "N", "the space between children"),
            prop(
                "align",
                "start | center | end",
                "where a narrower child sits across the stack",
            ),
            ANCHOR,
            STYLE,
        ],
        children: DRAWN,
    },
    NodeKind {
        name: "grid",
        about: "children placed row by row in columns; lines, arrows and braces take no room",
        properties: &[
            prop(
                "at",
                "X Y | NAME [SIDE F] [DX DY]",
                "the top-left corner of the first cell",
            ),
            prop("columns", "N", "how many cells across"),
            prop("gap", "N | X Y", "the space between cells"),
            prop(
                "align",
                "start | center | end",
                "where a smaller child sits in its cell",
            ),
            ANCHOR,
            STYLE,
        ],
        children: DRAWN,
    },
    NodeKind {
        name: "code",
        about: "source code, highlighted, on a background; lines are named from 1",
        properties: &[
            AT,
            PLACE,
            prop(
                "lang",
                "rust | python | acre | text",
                "the language to highlight",
            ),
            prop("text", "\"\"\"CODE\"\"\"", "the source, without markup"),
            prop("size", "N", "the text size"),
            prop("line-numbers", "show | hide", "whether line numbers show"),
            prop(
                "emphasize",
                "N...",
                "the lines, from 1, drawn on the emphasis color",
            ),
            prop("padding", "N | X Y", "the space around the code"),
            FILL,
            STROKE,
            STROKE_WIDTH,
            DASH,
            RADIUS,
            prop("text-color", "COLOR", "the color of plain text"),
            prop("keyword-color", "COLOR", "the color of keywords"),
            prop(
                "type-color",
                "COLOR",
                "the color of names that start with a capital",
            ),
            prop("string-color", "COLOR", "the color of strings"),
            prop("comment-color", "COLOR", "the color of comments"),
            prop("number-color", "COLOR", "the color of numbers"),
            prop("line-number-color", "COLOR", "the color of line numbers"),
            prop("emphasis", "COLOR", "the background of emphasized lines"),
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "caption",
        about: "text under the panel it sits in, or under everything before it",
        properties: &[
            prop("text", "\"TEXT\"", "the caption, with markup"),
            prop("width", "N", "the width it wraps within"),
            prop("size", "N", "the size in pixels"),
            prop("color", "COLOR", "the color"),
            FONT,
            prop("gap", "N", "the space above it"),
            ANCHOR,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "palette",
        about: "fills, notes and text colors for cells, by their text",
        properties: &[prop(
            "symbol",
            "\"TEXT\" FILL [\"NOTE\"] [TEXT-COLOR]",
            "how a cell with this text is drawn; repeat for each",
        )],
        children: &[],
    },
    NodeKind {
        name: "fonts",
        about: "font files a drawing uses, by name, with paths relative to the drawing's file",
        properties: &[
            prop(
                "NAME",
                "\"PATH\"",
                "registers the font file at PATH as NAME, which `font` then takes; repeat for each",
            ),
            prop(
                "math",
                "\"PATH\"",
                "sets formulas in the font file at PATH instead of Noto Sans Math; the file needs an OpenType MATH table",
            ),
        ],
        children: &[],
    },
    NodeKind {
        name: "data",
        about: "a table of numbers from a file, which `points` takes columns of; it draws nothing",
        properties: &[prop(
            "file",
            "\"PATH\"",
            "the file, relative to the drawing's file: `.tsv` for tabs between cells, `.csv` for commas; its first line names the columns",
        )],
        children: &[],
    },
    NodeKind {
        name: "timeline",
        about: "what happens to named nodes, and when; it holds entries, each an instant `Ts { ACTION...; }` \
                or a span `Ts to Ts [ease linear | in | out | in-out] { ACTION...; }`; a span eases in-out \
                unless it names another, and an action at an instant takes effect at once; an action changes a \
                thing from where it stands to its value, and two actions may not change one thing at once; \
                a NAME may be a group, panel, stack or grid, which moves, scales and fades as one with \
                everything inside it, and `ROW N` is one cell of a row, which moves, scales and fades on its \
                own and draws above the cells that do not move; a matrix changes as one, never by cell; `section` and `subsection` lines sit in the timeline itself, \
                between its entries",
        properties: &[
            prop(
                "section",
                "Ts \"TITLE\"",
                "start a part of the animation named TITLE at time T, which runs until the next section starts; the viewer labels each part on its time bar",
            ),
            prop(
                "subsection",
                "Ts \"TITLE\"",
                "start a part of the latest section before T, named TITLE, which runs until the next subsection or section starts",
            ),
            prop(
                "scale",
                "NAME F | ROW N F",
                "draw the node F times the size layout gave it, strokes and text included, about its `anchor`; neighbours stay put",
            ),
            prop(
                "move",
                "NAME DX DY | ROW N DX DY",
                "draw the node DX across and DY down from where layout put it; `move NAME 0 0` takes it back",
            ),
            prop(
                "fade",
                "NAME in | out | F | ROW N in | out | F",
                "fade the node to fully drawn, to gone, or to opacity F from 0 to 1; a node whose first change of opacity is `fade NAME in` starts hidden",
            ),
            prop(
                "show",
                "NAME | ROW N",
                "draw the node from this instant, and only at an instant; a node whose first change of opacity is `show` starts hidden",
            ),
            prop(
                "hide",
                "NAME | ROW N",
                "stop drawing the node from this instant, and only at an instant",
            ),
            prop(
                "fill",
                "NAME COLOR | ROW N COLOR",
                "change the fill of a box, an ellipse, a marker, a code block or a row's cell N; a row's name changes every cell, each from its own fill; not of a group, panel, stack or grid",
            ),
            prop(
                "stroke",
                "NAME COLOR | ROW N COLOR",
                "change the outline of a node that has one, a line's or an arrow's color, or a row's cell N's outline; a row's name changes every cell that has an outline",
            ),
            prop(
                "text-color",
                "NAME COLOR | ROW N COLOR",
                "change the color of every run of a node's text, or of a row's cell N; a row's name changes every cell that has text",
            ),
            prop(
                "text",
                "NAME \"TEXT\" | ROW N \"TEXT\"",
                "change the text of a box, an ellipse, a text or a row's cell N, only at an instant; layout sizes and places the node at the widest text the timeline gives it, and a row's cell keeps its fill",
            ),
        ],
        children: &[],
    },
    NodeKind {
        name: "repeat",
        about: "`repeat INDEX FIRST LAST { NODE... }` draws its nodes once for each whole number from FIRST \
                to LAST, both included; INDEX stands for that number wherever a `let` name can, and may not \
                be a `let` name or the index of a repeat around it; a node named inside gets the number \
                added to its name in each copy, as do the words inside that name it, so `b` is `b0`, `b1` \
                and so on; a repeat inside a repeat adds the outer number first",
        properties: &[],
        children: DRAWN,
    },
    NodeKind {
        name: "style",
        about: "named properties a node takes with `style NAME;`",
        properties: &[],
        children: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_child_kind_is_a_kind_and_no_kind_repeats_a_property() {
        for kind in NODES {
            for child in kind.children {
                assert!(node(child).is_some(), "{} holds unknown {child}", kind.name);
            }
            let keys = kind.keys();
            for (i, key) in keys.iter().enumerate() {
                assert!(!keys[..i].contains(key), "{} repeats {key}", kind.name);
            }
        }
    }
}
