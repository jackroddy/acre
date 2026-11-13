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
    "X Y | NAME [DX DY] | ROW N [DX DY]",
    "the top-left corner, in the group's coordinates or from a named thing's corner; \
     without it, the group's origin, or the node's slot in a stack or grid",
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
    "mono | sans | sans-bold | sans-italic",
    "the text's font",
);
const LABEL: Property = prop("label", "\"TEXT\"", "text beside it, with markup");
const SIDE: Property = prop("side", "top | bottom | left | right", "which way it faces");
const FROM: Property = prop(
    "from",
    "X Y | NAME [SIDE] | ROW N [SIDE]",
    "where it starts: a point, or the edge of a named thing",
);
const TO: Property = prop("to", "X Y | NAME [SIDE] | ROW N [SIDE]", "where it ends");
const STYLE: Property = prop("style", "NAME...", "the styles whose properties it takes");

const SHAPE: &[Property] = &[
    AT,
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
        "left | center | right",
        "the text from the top inside the padding; without it, the text is centered",
    ),
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
        "the path from `from` to `to`; an elbow turns at right angles and a curve leaves each end along the longer axis",
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
    LABEL,
    prop(
        "label-side",
        "above | left | right",
        "which side of the line the label sits on; left and right are as seen going from `from` to `to`, and above is the side that faces up",
    ),
    prop(
        "label-at",
        "F",
        "how far along the line the label sits, from 0 at `from` to 1 at `to`; the default is 0.5",
    ),
    TEXT_SIZE,
    TEXT_COLOR,
    FONT,
    STYLE,
];

const DRAWN: &[&str] = &[
    "box", "ellipse", "row", "line", "arrow", "text", "math", "table", "brace", "ruler", "group",
    "panel", "caption", "stack", "grid", "code",
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
            FONT,
            prop(
                "let",
                "NAME VALUES...",
                "a name that stands for its values in any later property",
            ),
        ],
        children: &[
            "box", "ellipse", "row", "line", "arrow", "text", "math", "table", "brace", "ruler",
            "group", "panel", "caption", "stack", "grid", "code", "palette", "style",
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
        about: "cells side by side on a grid of equal units",
        properties: &[
            AT,
            prop("unit", "W H", "the width of one unit and the row's height"),
            prop("span", "N", "how many units each cell covers"),
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
                "mono | sans | sans-bold | sans-italic",
                "the notes' font",
            ),
            STYLE,
        ],
        children: &["cell"],
    },
    NodeKind {
        name: "cell",
        about: "one more cell at the end of a row",
        properties: &[
            prop("text", "\"TEXT\"", "the cell's text"),
            prop("note", "\"TEXT\"", "small text under the cell's text"),
            prop("span", "N", "how many units it covers"),
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
        name: "text",
        about: "text placed by its top edge",
        properties: &[
            AT,
            prop("text", "\"TEXT\"", "the text, with markup"),
            prop("size", "N", "the size in pixels"),
            prop("color", "COLOR", "the color"),
            prop(
                "align",
                "left | center | right",
                "where `at` sits along the text",
            ),
            prop("width", "N", "the width the text wraps within, at spaces"),
            FONT,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "math",
        about: "a formula in LaTeX math, set in display style and placed by its top edge",
        properties: &[
            AT,
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
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "table",
        about: "rows of strings in aligned columns",
        properties: &[
            AT,
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
                "NAME | ROW [N [M]]",
                "the thing, row or run of cells it brackets",
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
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "ruler",
        about: "a scale of equal units with numbers",
        properties: &[
            AT,
            prop("unit", "N", "the width of one unit"),
            prop("count", "N", "how many units"),
            prop("first", "N", "the number over the first unit"),
            prop("step", "N", "how much each number grows"),
            prop("every", "N", "a number over every Nth unit"),
            SIDE,
            STROKE,
            STROKE_WIDTH,
            TEXT_SIZE,
            TEXT_COLOR,
            FONT,
            STYLE,
        ],
        children: &[],
    },
    NodeKind {
        name: "group",
        about: "nodes moved together by `at`",
        properties: &[prop(
            "at",
            "X Y | NAME [DX DY]",
            "where the group's origin sits",
        )],
        children: DRAWN,
    },
    NodeKind {
        name: "panel",
        about: "a numbered section with a heading; `stroke` or `fill` draws it as a card",
        properties: &[
            prop(
                "at",
                "X Y | NAME [DX DY]",
                "the top-left corner of the frame, or of the heading when there is no frame",
            ),
            prop("title", "\"TEXT\"", "the heading, with markup"),
            prop("number", "none", "leave the panel out of the numbering"),
            prop("title-size", "N", "the heading's size"),
            prop("title-color", "COLOR", "the heading's color"),
            prop(
                "title-font",
                "mono | sans | sans-bold | sans-italic",
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
                "X Y | NAME [DX DY]",
                "where the first child's slot starts",
            ),
            prop("direction", "down | right", "which way the children go"),
            prop("gap", "N", "the space between children"),
            prop(
                "align",
                "start | center | end",
                "where a narrower child sits across the stack",
            ),
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
                "X Y | NAME [DX DY]",
                "the top-left corner of the first cell",
            ),
            prop("columns", "N", "how many cells across"),
            prop("gap", "N | X Y", "the space between cells"),
            prop(
                "align",
                "start | center | end",
                "where a smaller child sits in its cell",
            ),
            STYLE,
        ],
        children: DRAWN,
    },
    NodeKind {
        name: "code",
        about: "source code, highlighted, on a background; lines are named from 1",
        properties: &[
            AT,
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
