use super::diagram::Label;
use super::primitive::{Color, HAlign, Point, Primitive, Stroke, VAlign};

/// A horizontal run of cells on a grid of equal units, for words of
/// bits, arrays and strings. A cell covers a whole number of units.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The top-left corner of the first unit.
    pub origin: Point,
    /// The width of one unit and the height of the row.
    pub unit: Point,
    /// The space left between neighbouring cells.
    pub gap: f64,
    /// The radius of each cell's rounded corners.
    pub radius: f64,
    pub cells: Vec<RowCell>,
}

/// One cell of a row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowCell {
    /// The first unit the cell covers.
    pub start: usize,
    /// The number of units the cell covers.
    pub span: usize,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub label: Option<Label>,
    /// Small text under the label, one character to a unit when it has
    /// as many characters as the cell has units.
    pub note: Option<Label>,
}

impl Row {
    /// The rectangle cell `index` fills, as its origin and size.
    pub fn cell_rect(&self, index: usize) -> (Point, Point) {
        let cell = &self.cells[index];
        let x = self.origin.x + cell.start as f64 * self.unit.x + self.gap / 2.0;
        let width = cell.span as f64 * self.unit.x - self.gap;
        (Point::new(x, self.origin.y), Point::new(width, self.unit.y))
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        for (i, cell) in self.cells.iter().enumerate() {
            let (origin, size) = self.cell_rect(i);
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
            let label_y = match &cell.note {
                Some(_) => origin.y + size.y * 0.4,
                None => center.y,
            };
            if let Some(label) = &cell.label {
                out.push(label.at(
                    Point::new(center.x, label_y),
                    HAlign::Center,
                    VAlign::Middle,
                ));
            }
            if let Some(note) = &cell.note {
                let y = origin.y + size.y * 0.78;
                let text = note.text();
                let chars = text.chars().count();
                if chars == cell.span && chars > 1 {
                    let left = self.origin.x + cell.start as f64 * self.unit.x;
                    for (k, c) in text.chars().enumerate() {
                        let x = left + (k as f64 + 0.5) * self.unit.x;
                        let one = note.with_text(c);
                        out.push(one.at(Point::new(x, y), HAlign::Center, VAlign::Middle));
                    }
                } else {
                    out.push(note.at(Point::new(center.x, y), HAlign::Center, VAlign::Middle));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(start: usize, span: usize) -> RowCell {
        RowCell {
            start,
            span,
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
            unit: Point::new(16.0, 40.0),
            gap: 2.0,
            radius: 0.0,
            cells: vec![cell(0, 3), cell(3, 1)],
        };
        assert_eq!(
            row.cell_rect(1),
            (Point::new(59.0, 20.0), Point::new(14.0, 40.0))
        );
    }
}
