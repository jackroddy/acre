//! Data files a drawing takes numbers from.

use std::collections::HashMap;
use std::path::Path;

use super::Point;
use super::build::{Props, keys, twice};
use crate::lang::{Error, Node, Program, Property, Span, Value, ValueKind};

/// A data file the canvas declares with `data`, for a program to read.
#[derive(Debug, Clone, PartialEq)]
pub struct DataFile {
    /// The name `points` takes for it.
    pub name: String,
    /// The path as written, which a program resolves against the file
    /// the program came from.
    pub path: String,
    /// Where the path is in the source.
    pub span: Span,
}

/// The data files `program` declares, in source order. A program reads
/// them all and passes their bytes, in this order, to
/// [`build`](super::build).
pub fn data_files(program: &Program) -> Result<Vec<DataFile>, Error> {
    match program.nodes.as_slice() {
        [canvas, ..] if canvas.kind.text == "canvas" => listed(canvas),
        _ => Ok(Vec::new()),
    }
}

/// An error for a `data` node in `nodes` or below them.
fn nested(nodes: &[Node]) -> Result<(), Error> {
    for node in nodes {
        if node.kind.text == "data" {
            return Err(Error::new(node.kind.span, "a `data` goes in the canvas"));
        }
        nested(&node.children)?;
    }
    Ok(())
}

/// The files of the `data` nodes of `canvas`.
pub(super) fn listed(canvas: &Node) -> Result<Vec<DataFile>, Error> {
    for node in &canvas.children {
        nested(&node.children)?;
    }
    let mut out: Vec<DataFile> = Vec::new();
    for node in canvas.children.iter().filter(|n| n.kind.text == "data") {
        let name = node
            .name
            .as_ref()
            .ok_or_else(|| Error::new(node.kind.span, "a `data` needs a name"))?;
        if out.iter().any(|d| d.name == name.text) {
            return Err(twice("`data` nodes", name));
        }
        let props = Props::new(node, &keys("data"), false)?;
        let p = props.require("file")?;
        let file = match p.values.as_slice() {
            [
                Value {
                    kind: ValueKind::Str(path),
                    span,
                },
            ] => DataFile {
                name: name.text.clone(),
                path: path.clone(),
                span: *span,
            },
            _ => return Err(p.expected("a quoted path to a .tsv or .csv file")),
        };
        separator(&file)?;
        out.push(file);
    }
    Ok(out)
}

/// The character between cells in `file`, which its extension picks.
fn separator(file: &DataFile) -> Result<char, Error> {
    let extension = Path::new(&file.path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("tsv") {
        Ok('\t')
    } else if extension.eq_ignore_ascii_case("csv") {
        Ok(',')
    } else {
        Err(Error::new(
            file.span,
            format!(
                "`{}` is not a .tsv or .csv file, which `data` reads",
                file.path
            ),
        ))
    }
}

/// The columns and rows of a data file.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Table {
    path: String,
    columns: Vec<String>,
    /// Each row's line in the file, counted from 1, and its cells.
    rows: Vec<(usize, Vec<String>)>,
}

/// The tables of the files `listed` names, by data name, from their
/// `bytes` in the same order.
pub(super) fn read(
    listed: &[DataFile],
    bytes: &[Vec<u8>],
    canvas: &Node,
) -> Result<HashMap<String, Table>, Error> {
    if listed.len() != bytes.len() {
        return Err(Error::new(
            canvas.span,
            format!(
                "`data` names {} files and {} were read",
                listed.len(),
                bytes.len()
            ),
        ));
    }
    let mut out = HashMap::new();
    for (file, data) in listed.iter().zip(bytes) {
        let table =
            parse(&file.path, data, separator(file)?).map_err(|e| Error::new(file.span, e))?;
        out.insert(file.name.clone(), table);
    }
    Ok(out)
}

/// The table in `bytes`, with `separator` between cells: the first line
/// names the columns, and every other line that is not blank is a row.
fn parse(path: &str, bytes: &[u8], separator: char) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| format!("`{path}` is not UTF-8 text"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let cells = |n: usize, line: &str| -> Result<Vec<String>, String> {
        line.split(separator)
            .map(|cell| {
                let cell = cell.trim();
                if separator == ',' && cell.starts_with('"') {
                    return Err(format!(
                        "`{path}` line {n} has a quoted cell, and quoted CSV is not supported"
                    ));
                }
                Ok(cell.to_string())
            })
            .collect()
    };
    let mut lines = text.lines().enumerate().map(|(i, line)| (i + 1, line));
    let columns = match lines.next() {
        Some((n, line)) if !line.trim().is_empty() => cells(n, line)?,
        _ => return Err(format!("`{path}` needs the column names on its first line")),
    };
    for (i, c) in columns.iter().enumerate() {
        if columns[..i].contains(c) {
            return Err(format!("`{path}` names the column `{c}` twice"));
        }
    }
    let mut rows = Vec::new();
    for (n, line) in lines {
        if line.trim().is_empty() {
            continue;
        }
        let row = cells(n, line)?;
        if row.len() < columns.len() {
            return Err(format!(
                "`{path}` line {n} has fewer cells than the {} columns the first line names",
                columns.len()
            ));
        }
        rows.push((n, row));
    }
    Ok(Table {
        path: path.into(),
        columns,
        rows,
    })
}

impl Table {
    /// The points `p` takes as `points DATA XCOL YCOL`, one per row with
    /// X from column XCOL and Y from column YCOL, from `offset`.
    pub(super) fn points(&self, p: &Property, offset: Point) -> Result<Vec<Point>, Error> {
        let [_, x, y] = p.values.as_slice() else {
            return Err(Error::new(
                p.span,
                "`points DATA XCOL YCOL` takes two column names after the data name",
            ));
        };
        let (x, y) = (self.column(x)?, self.column(y)?);
        let mut out = Vec::with_capacity(self.rows.len());
        for (n, cells) in &self.rows {
            out.push(offset + Point::new(self.number(*n, cells, x)?, self.number(*n, cells, y)?));
        }
        Ok(out)
    }

    /// The cells `p` gives text as `cells DATA RCOL CCOL TEXTCOL`, one
    /// per row, as each cell's row and column in a matrix of `rows` by
    /// `columns`, and its text.
    pub(super) fn cells(
        &self,
        p: &Property,
        rows: usize,
        columns: usize,
    ) -> Result<Vec<(usize, usize, String)>, Error> {
        let [_, r, c, text] = p.values.as_slice() else {
            return Err(Error::new(
                p.span,
                "`cells DATA RCOL CCOL TEXTCOL` takes three column names after the data name",
            ));
        };
        let (r, c, text) = (self.column(r)?, self.column(c)?, self.column(text)?);
        let mut out = Vec::with_capacity(self.rows.len());
        for (n, cells) in &self.rows {
            let row = self.index(*n, cells, r, rows)?;
            let column = self.index(*n, cells, c, columns)?;
            out.push((row, column, cells[text.0].clone()));
        }
        Ok(out)
    }

    /// The whole number below `count` in the cell of `column` on line
    /// `n`.
    fn index(
        &self,
        n: usize,
        cells: &[String],
        column: (usize, &str, Span),
        count: usize,
    ) -> Result<usize, Error> {
        let x = self.number(n, cells, column)?;
        if x.fract() != 0.0 || x < 0.0 || x >= count as f64 {
            let (i, name, span) = column;
            return Err(Error::new(
                span,
                format!(
                    "`{}` line {n} has `{}` in column `{name}`, which is not a whole number from 0 to {}",
                    self.path,
                    cells[i],
                    count - 1
                ),
            ));
        }
        Ok(x as usize)
    }

    /// The index of the column `v` names, with `v`.
    fn column<'v>(&self, v: &'v Value) -> Result<(usize, &'v str, Span), Error> {
        let ValueKind::Word(name) = &v.kind else {
            return Err(Error::new(v.span, "expected a column name"));
        };
        match self.columns.iter().position(|c| c == name) {
            Some(i) => Ok((i, name, v.span)),
            None => Err(Error::new(
                v.span,
                format!(
                    "`{}` has no column `{name}`; its columns are {}",
                    self.path,
                    self.columns.join(", ")
                ),
            )),
        }
    }

    /// The number in the cell of `column` on line `n`.
    fn number(
        &self,
        n: usize,
        cells: &[String],
        (i, name, span): (usize, &str, Span),
    ) -> Result<f64, Error> {
        let cell = &cells[i];
        match cell.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(x),
            _ => Err(Error::new(
                span,
                format!(
                    "`{}` line {n} has `{cell}` in column `{name}`, which is not a number",
                    self.path
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(t: &Table) -> Vec<(usize, Vec<&str>)> {
        t.rows
            .iter()
            .map(|(n, cells)| (*n, cells.iter().map(String::as_str).collect()))
            .collect()
    }

    #[test]
    fn a_tsv_has_its_columns_first_and_skips_blank_lines() {
        let t = parse("a.tsv", b"x\ty\tname\n1\t2\tone\n\n 3 \t4.5\ttwo\t\n", '\t').unwrap();
        assert_eq!(t.columns, ["x", "y", "name"]);
        assert_eq!(
            rows(&t),
            [(2, vec!["1", "2", "one"]), (4, vec!["3", "4.5", "two", ""])]
        );
    }

    #[test]
    fn a_csv_splits_on_commas_and_trims_cells() {
        let t = parse("a.csv", b"\xEF\xBB\xBFx, y\r\n1 ,2\r\n", ',').unwrap();
        assert_eq!(t.columns, ["x", "y"]);
        assert_eq!(rows(&t), [(2, vec!["1", "2"])]);
    }

    #[test]
    fn a_short_row_a_quoted_cell_or_no_header_is_an_error() {
        assert_eq!(
            parse("a.csv", b"x,y\n1,2\n3\n", ','),
            Err("`a.csv` line 3 has fewer cells than the 2 columns the first line names".into())
        );
        assert_eq!(
            parse("a.csv", b"x,y\n\"1\",2\n", ','),
            Err("`a.csv` line 2 has a quoted cell, and quoted CSV is not supported".into())
        );
        assert_eq!(
            parse("a.tsv", b"\n1\t2\n", '\t'),
            Err("`a.tsv` needs the column names on its first line".into())
        );
        assert_eq!(
            parse("a.tsv", b"x\tx\n", '\t'),
            Err("`a.tsv` names the column `x` twice".into())
        );
        assert_eq!(
            parse("a.tsv", b"\xff", '\t'),
            Err("`a.tsv` is not UTF-8 text".into())
        );
    }
}
