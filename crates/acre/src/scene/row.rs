use super::Target;
use super::diagram::Label;
use super::primitive::{Color, HAlign, Point, Primitive, Stroke, VAlign, bounds};

/// A horizontal run of cells, for words of bits, arrays and strings.
/// A cell covers a whole number of equal units, or a share of the row's
/// width in proportion to its weight.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The top-left corner of the first cell.
    pub origin: Point,
    pub height: f64,
    /// Whether any cell has a note, which raises every cell's label so
    /// the labels of the row line up.
    pub noted: bool,
    /// The space left between neighbouring cells.
    pub gap: f64,
    /// The radius of each cell's rounded corners.
    pub radius: f64,
    pub cells: Vec<RowCell>,
    /// The row cell its `at` names, which it follows as that cell moves
    /// and scales.
    pub follow: Option<Target>,
}

/// One cell of a row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowCell {
    /// The distance from the row's left edge to the cell's, before the
    /// gap.
    pub left: f64,
    /// The width the cell covers, gap included.
    pub width: f64,
    /// The number of units the cell covers, 1 in a row with weights.
    pub span: usize,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub label: Option<Label>,
    /// Small text under the label, one character to a unit when it has
    /// as many characters as the cell has units.
    pub note: Option<Label>,
}

/// Where a note's middle sits, as a share of the cell's height.
const NOTE_Y: f64 = 0.78;

/// Where a label's middle sits in a row with notes, as a share of the
/// cell's height.
const NOTED_LABEL_Y: f64 = 0.4;

/// The height of a row of `cells` on units `unit` tall: the unit, or
/// taller when the tallest note would reach past the bottom of a cell.
pub(crate) fn height(unit: f64, cells: &[RowCell]) -> f64 {
    let below = cells
        .iter()
        .filter_map(|c| c.note.as_ref())
        .filter_map(|n| {
            bounds(
                &n.fonts,
                &[n.at(Point::ZERO, HAlign::Center, VAlign::Middle)],
            )
        })
        .map(|(_, max)| max.y)
        .fold(0.0, f64::max);
    // `below` is how far a note reaches under its middle, and
    // the note's bottom meets the cell's at
    //   NOTE_Y * h + below = h
    unit.max(below / (1.0 - NOTE_Y))
}

impl Row {
    /// The rectangle cell `index` fills, as its origin and size.
    pub fn cell_rect(&self, index: usize) -> (Point, Point) {
        let cell = &self.cells[index];
        let x = self.origin.x + cell.left + self.gap / 2.0;
        let width = (cell.width - self.gap).max(0.0);
        (Point::new(x, self.origin.y), Point::new(width, self.height))
    }

    /// The width of the whole row, from the left edge of its first cell
    /// to the right edge of its last, gaps included.
    pub fn width(&self) -> f64 {
        self.cells.last().map_or(0.0, |c| c.left + c.width)
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        for i in 0..self.cells.len() {
            self.draw_cell(i, out);
        }
    }

    /// Append cell `index`, with its text and its note, to `out`.
    pub fn draw_cell(&self, index: usize, out: &mut Vec<Primitive>) {
        let cell = &self.cells[index];
        let (origin, size) = self.cell_rect(index);
        if cell.fill.is_some() || cell.stroke.is_some() {
            out.push(Primitive::Rect {
                origin,
                size,
                radius: self.radius,
                fill: cell.fill,
                stroke: cell.stroke,
            });
        }
        let center = origin + size * 0.5;
        let label_y = if self.noted {
            origin.y + size.y * NOTED_LABEL_Y
        } else {
            center.y
        };
        if let Some(label) = &cell.label {
            out.push(label.at(
                Point::new(center.x, label_y),
                HAlign::Center,
                VAlign::Middle,
            ));
        }
        if let Some(note) = &cell.note {
            let y = origin.y + size.y * NOTE_Y;
            let text = note.text();
            let chars = text.chars().count();
            if chars == cell.span && chars > 1 {
                let left = self.origin.x + cell.left;
                let unit = cell.width / cell.span as f64;
                for (k, c) in text.chars().enumerate() {
                    let x = left + (k as f64 + 0.5) * unit;
                    let one = note.with_text(c);
                    out.push(one.at(Point::new(x, y), HAlign::Center, VAlign::Middle));
                }
            } else {
                out.push(note.at(Point::new(center.x, y), HAlign::Center, VAlign::Middle));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(left: f64, width: f64) -> RowCell {
        RowCell {
            left,
            width,
            span: 1,
            fill: Some(Color::WHITE),
            stroke: None,
            label: None,
            note: None,
        }
    }

    #[test]
    fn a_cell_covers_its_units_less_the_gap() {
        let row = Row {
            origin: Point::new(10.0, 20.0),
            height: 40.0,
            noted: false,
            gap: 2.0,
            radius: 0.0,
            cells: vec![cell(0.0, 48.0), cell(48.0, 16.0)],
            follow: None,
        };
        assert_eq!(
            row.cell_rect(1),
            (Point::new(59.0, 20.0), Point::new(14.0, 40.0))
        );
    }
}
