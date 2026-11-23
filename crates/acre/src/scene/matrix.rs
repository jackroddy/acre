use super::Target;
use super::diagram::Label;
use super::primitive::{Color, HAlign, Point, Primitive, Stroke, VAlign};

/// The space between a matrix's numbers and its cells.
const NUMBER_GAP: f64 = 6.0;

/// A grid of equal cells, addressed by row and column from 0, with
/// optional numbers beside its rows and above its columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    /// The top-left corner of the first cell.
    pub origin: Point,
    pub rows: usize,
    pub columns: usize,
    /// The width and height of one cell.
    pub unit: Point,
    /// The cells row by row: row `r`, column `c` is `cells[r * columns + c]`.
    pub cells: Vec<MatrixCell>,
    /// The fill of every cell that sets none of its own.
    pub fill: Option<Color>,
    /// The outline of every cell that sets none of its own.
    pub stroke: Option<Stroke>,
    /// The size, font and color of the numbers and of cell text, with
    /// no text.
    pub text: Label,
    /// The first number beside the rows and the step between them.
    pub row_numbers: Option<(f64, f64)>,
    /// The first number above the columns and the step between them.
    pub column_numbers: Option<(f64, f64)>,
    /// The row cell its `at` names, which it follows as that cell moves
    /// and scales.
    pub follow: Option<Target>,
}

/// One cell of a matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixCell {
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub label: Option<Label>,
    /// Whether the cell sets its own outline, which draws after the
    /// cells that do not so that no neighbour covers it.
    pub outlined: bool,
}

impl Matrix {
    /// The rectangle of the cell at `row` and `column`, as its origin and
    /// size.
    pub fn cell_rect(&self, row: usize, column: usize) -> (Point, Point) {
        let origin =
            self.origin + Point::new(column as f64 * self.unit.x, row as f64 * self.unit.y);
        (origin, self.unit)
    }

    /// The width and height of the cells together.
    pub fn size(&self) -> Point {
        Point::new(
            self.columns as f64 * self.unit.x,
            self.rows as f64 * self.unit.y,
        )
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        for outlined in [false, true] {
            for (i, cell) in self.cells.iter().enumerate() {
                if cell.outlined != outlined {
                    continue;
                }
                let (origin, size) = self.cell_rect(i / self.columns, i % self.columns);
                if cell.fill.is_some() || cell.stroke.is_some() {
                    out.push(Primitive::Rect {
                        origin,
                        size,
                        radius: 0.0,
                        fill: cell.fill,
                        stroke: cell.stroke,
                    });
                }
                if let Some(label) = &cell.label {
                    out.push(label.at(origin + size * 0.5, HAlign::Center, VAlign::Middle));
                }
            }
        }
        if let Some((first, step)) = self.row_numbers {
            for r in 0..self.rows {
                let at = Point::new(
                    self.origin.x - NUMBER_GAP,
                    self.origin.y + (r as f64 + 0.5) * self.unit.y,
                );
                let label = self.text.with_text(format!("{}", first + step * r as f64));
                out.push(label.at(at, HAlign::Right, VAlign::Middle));
            }
        }
        if let Some((first, step)) = self.column_numbers {
            for c in 0..self.columns {
                let at = Point::new(
                    self.origin.x + (c as f64 + 0.5) * self.unit.x,
                    self.origin.y - NUMBER_GAP,
                );
                let label = self.text.with_text(format!("{}", first + step * c as f64));
                out.push(label.at(at, HAlign::Center, VAlign::Baseline));
            }
        }
    }

    pub fn repaint_fill(&mut self, color: Color) {
        self.fill = Some(color);
        for cell in &mut self.cells {
            cell.fill = Some(color);
        }
    }

    pub fn repaint_stroke(&mut self, color: Color) {
        let strokes = self.cells.iter_mut().map(|c| &mut c.stroke);
        for stroke in std::iter::once(&mut self.stroke).chain(strokes).flatten() {
            stroke.color = color;
        }
    }

    pub fn repaint_text(&mut self, color: Color) {
        self.text.set_color(color);
        for label in self.cells.iter_mut().filter_map(|c| c.label.as_mut()) {
            label.set_color(color);
        }
    }
}
