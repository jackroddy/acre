use super::primitive::{Color, Font, HAlign, Point, Primitive, Run, Stroke, VAlign};
use super::track::Track;

/// A grid of cells holding text, which covers arrays, vectors and
/// matrices. Cells are row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// The top-left corner of the cell at row 0, column 0.
    pub origin: Track<Point>,
    pub rows: usize,
    pub cols: usize,
    pub cell_size: Point,
    /// The space between neighbouring cells.
    pub gap: f64,
    pub text_size: f64,
    pub stroke: Option<Stroke>,
    pub cells: Vec<Cell>,
}

/// One cell of a grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: Track<String>,
    pub fill: Track<Color>,
    pub text_color: Track<Color>,
    /// Displacement from the cell's slot, for sliding a cell around.
    pub offset: Track<Point>,
}

impl Cell {
    /// A white cell with black text that stays in its slot.
    pub fn new(text: impl Into<String>) -> Self {
        Cell {
            text: Track::Constant(text.into()),
            fill: Track::Constant(Color::WHITE),
            text_color: Track::Constant(Color::BLACK),
            offset: Track::Constant(Point::ZERO),
        }
    }
}

impl Grid {
    /// A grid of `rows` by `cols` cells, filled from `texts` in row-major
    /// order.
    pub fn new(
        rows: usize,
        cols: usize,
        texts: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let cells: Vec<Cell> = texts.into_iter().map(Cell::new).collect();
        assert_eq!(cells.len(), rows * cols, "a grid needs one text per cell");
        Grid {
            origin: Track::Constant(Point::ZERO),
            rows,
            cols,
            cell_size: Point::new(40.0, 40.0),
            gap: 4.0,
            text_size: 20.0,
            stroke: Some(Stroke::solid(Color::BLACK, 1.0)),
            cells,
        }
    }

    /// The cell at `row`, `col`.
    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        &mut self.cells[row * self.cols + col]
    }

    /// Where the slot at `row`, `col` sits relative to the grid's origin.
    pub fn slot(&self, row: usize, col: usize) -> Point {
        Point::new(
            col as f64 * (self.cell_size.x + self.gap),
            row as f64 * (self.cell_size.y + self.gap),
        )
    }

    /// Append this grid at time `t` to `out`, each cell as a rect
    /// followed by its text.
    pub fn draw(&self, t: f64, out: &mut Vec<Primitive>) {
        let origin = self.origin.at(t);
        for (i, cell) in self.cells.iter().enumerate() {
            let (row, col) = (i / self.cols, i % self.cols);
            let corner = origin + self.slot(row, col) + cell.offset.at(t);
            out.push(Primitive::Rect {
                origin: corner,
                size: self.cell_size,
                radius: 0.0,
                fill: Some(cell.fill.at(t)),
                stroke: self.stroke,
            });
            out.push(Primitive::Text {
                origin: corner + self.cell_size * 0.5,
                runs: vec![Run {
                    text: cell.text.at(t),
                    font: Font::Mono,
                    color: cell.text_color.at(t),
                    bullet: false,
                }],
                size: self.text_size,
                width: None,
                justify: false,
                h_align: HAlign::Center,
                v_align: VAlign::Middle,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_origins(prims: &[Primitive]) -> Vec<Point> {
        prims
            .iter()
            .filter_map(|p| match p {
                Primitive::Rect { origin, .. } => Some(*origin),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lays_cells_out_row_major_with_gaps() {
        let mut grid = Grid::new(2, 2, ["a", "b", "c", "d"]);
        grid.cell_size = Point::new(10.0, 10.0);
        grid.gap = 2.0;
        grid.origin = Track::Constant(Point::new(100.0, 200.0));
        let mut out = Vec::new();
        grid.draw(0.0, &mut out);
        assert_eq!(out.len(), 8);
        assert_eq!(
            rect_origins(&out),
            vec![
                Point::new(100.0, 200.0),
                Point::new(112.0, 200.0),
                Point::new(100.0, 212.0),
                Point::new(112.0, 212.0),
            ]
        );
    }

    #[test]
    fn centres_text_in_its_cell() {
        let mut grid = Grid::new(1, 1, ["7"]);
        grid.cell_size = Point::new(10.0, 20.0);
        let mut out = Vec::new();
        grid.draw(0.0, &mut out);
        match &out[1] {
            Primitive::Text {
                origin,
                runs,
                h_align,
                v_align,
                ..
            } => {
                assert_eq!(*origin, Point::new(5.0, 10.0));
                assert_eq!(runs[0].text, "7");
                assert_eq!(*h_align, HAlign::Center);
                assert_eq!(*v_align, VAlign::Middle);
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_swap_is_halfway_at_its_midpoint() {
        let mut grid = Grid::new(1, 3, ["1", "2", "3"]);
        grid.cell_size = Point::new(10.0, 10.0);
        grid.gap = 0.0;
        let to_col2 = grid.slot(0, 2) - grid.slot(0, 0);
        grid.cell_mut(0, 0).offset = Track::from_to(1.0, Point::ZERO, 2.0, to_col2);
        grid.cell_mut(0, 2).offset = Track::from_to(1.0, Point::ZERO, 2.0, to_col2 * -1.0);
        let mut out = Vec::new();
        grid.draw(1.5, &mut out);
        let origins = rect_origins(&out);
        assert_eq!(origins[0], Point::new(10.0, 0.0));
        assert_eq!(origins[1], Point::new(10.0, 0.0));
        assert_eq!(origins[2], Point::new(10.0, 0.0));
        out.clear();
        grid.draw(2.0, &mut out);
        let origins = rect_origins(&out);
        assert_eq!(origins[0], Point::new(20.0, 0.0));
        assert_eq!(origins[2], Point::new(0.0, 0.0));
    }

    #[test]
    #[should_panic(expected = "one text per cell")]
    fn rejects_the_wrong_number_of_texts() {
        Grid::new(2, 2, ["a", "b"]);
    }
}
