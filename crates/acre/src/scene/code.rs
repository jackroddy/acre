use super::diagram::Label;
use super::primitive::{Color, Font, HAlign, Point, Primitive, Run, Stroke, VAlign};
use crate::fonts::Fonts;
use crate::text;

/// The distance from one line of code to the next, in multiples of the
/// text size.
const LINE_HEIGHT: f64 = 1.45;

/// A block of source code, highlighted, on a background.
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub origin: Point,
    /// Each line as runs of one color.
    pub lines: Vec<Vec<Run>>,
    pub size: f64,
    pub padding: Point,
    /// The color of line numbers, or `None` to leave them out.
    pub numbers: Option<Color>,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub radius: f64,
    /// The lines, numbered from 1, drawn on `emphasis`.
    pub emphasize: Vec<usize>,
    pub emphasis: Color,
    /// The fonts the lines are measured and set in.
    pub fonts: Fonts,
}

/// What a stretch of source is, for its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Plain,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
}

/// A language the highlighter knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    Acre,
    /// No highlighting.
    Text,
}

impl Lang {
    pub fn named(name: &str) -> Option<Lang> {
        Some(match name {
            "rust" => Lang::Rust,
            "python" => Lang::Python,
            "acre" => Lang::Acre,
            "text" => Lang::Text,
            _ => return None,
        })
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as", "break", "const", "continue", "crate", "else", "enum", "false", "fn", "for",
                "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
                "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
                "unsafe", "use", "where", "while",
            ],
            Lang::Python => &[
                "False", "None", "True", "and", "as", "assert", "break", "class", "continue",
                "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "not", "or", "pass", "raise", "return", "try",
                "while", "with", "yield",
            ],
            Lang::Acre => &[
                "canvas", "box", "ellipse", "row", "cell", "line", "arrow", "text", "marker",
                "table", "brace", "ruler", "group", "panel", "caption", "stack", "grid", "palette",
                "style", "code", "let",
            ],
            Lang::Text => &[],
        }
    }

    fn line_comment(self) -> Option<&'static str> {
        match self {
            Lang::Rust | Lang::Acre => Some("//"),
            Lang::Python => Some("#"),
            Lang::Text => None,
        }
    }
}

/// `source` split into lines of classified stretches.
pub fn highlight(source: &str, lang: Lang) -> Vec<Vec<(String, Class)>> {
    let mut lines = Vec::new();
    let mut in_block_comment = false;
    for line in source.split('\n') {
        let mut out: Vec<(String, Class)> = Vec::new();
        let mut push = |text: &str, class: Class| {
            if text.is_empty() {
                return;
            }
            match out.last_mut() {
                Some((last, c)) if *c == class => last.push_str(text),
                _ => out.push((text.to_string(), class)),
            }
        };
        if lang == Lang::Text {
            push(line, Class::Plain);
            lines.push(out);
            continue;
        }
        let mut i = 0;
        while i < line.len() {
            let rest = &line[i..];
            if in_block_comment {
                let end = rest.find("*/").map_or(rest.len(), |k| k + 2);
                in_block_comment = !rest[..end].ends_with("*/");
                push(&rest[..end], Class::Comment);
                i += end;
                continue;
            }
            if lang == Lang::Rust && rest.starts_with("/*") {
                in_block_comment = true;
                push("/*", Class::Comment);
                i += 2;
                continue;
            }
            if lang.line_comment().is_some_and(|c| rest.starts_with(c)) {
                push(rest, Class::Comment);
                break;
            }
            let c = rest.chars().next().expect("rest is not empty");
            let quote = c == '"' || (c == '\'' && lang == Lang::Python);
            if quote {
                let mut end = 1;
                let mut escaped = false;
                for (k, d) in rest.char_indices().skip(1) {
                    end = k + d.len_utf8();
                    if escaped {
                        escaped = false;
                    } else if d == '\\' {
                        escaped = true;
                    } else if d == c {
                        break;
                    }
                }
                push(&rest[..end], Class::Str);
                i += end;
            } else if c.is_ascii_digit() || (lang == Lang::Acre && c == '#') {
                let end = rest
                    .find(|d: char| {
                        !(d.is_ascii_alphanumeric() || d == '_' || d == '.' || d == '#')
                    })
                    .unwrap_or(rest.len());
                push(&rest[..end], Class::Number);
                i += end;
            } else if c.is_alphabetic() || c == '_' {
                let end = rest
                    .find(|d: char| {
                        !(d.is_alphanumeric() || d == '_' || (lang == Lang::Acre && d == '-'))
                    })
                    .unwrap_or(rest.len());
                let word = &rest[..end];
                let class = if lang.keywords().contains(&word) {
                    Class::Keyword
                } else if word.starts_with(|d: char| d.is_uppercase()) && lang != Lang::Acre {
                    Class::Type
                } else {
                    Class::Plain
                };
                push(word, class);
                i += end;
            } else {
                push(&rest[..c.len_utf8()], Class::Plain);
                i += c.len_utf8();
            }
        }
        lines.push(out);
    }
    lines
}

impl Code {
    fn line_height(&self) -> f64 {
        LINE_HEIGHT * self.size
    }

    /// The width of the gutter that holds the line numbers.
    fn gutter(&self) -> f64 {
        match self.numbers {
            None => 0.0,
            Some(_) => {
                let digits = self.lines.len().to_string().len();
                text::layout(
                    &self.fonts,
                    &[mono("0".repeat(digits), Color::BLACK)],
                    self.size,
                    None,
                )
                .width()
                    + self.size
            }
        }
    }

    /// The width and height of the block, padding included.
    pub fn extent(&self) -> Point {
        let widest = self
            .lines
            .iter()
            .map(|runs| text::layout(&self.fonts, runs, self.size, None).width())
            .fold(0.0, f64::max);
        Point::new(
            widest + self.gutter() + 2.0 * self.padding.x,
            self.lines.len() as f64 * self.line_height() + 2.0 * self.padding.y,
        )
    }

    /// The box of line `n`, numbered from 1, across the whole block, as
    /// its origin and size.
    pub fn line_rect(&self, n: usize) -> (Point, Point) {
        let width = self.extent().x;
        let y = self.origin.y + self.padding.y + (n - 1) as f64 * self.line_height();
        (
            Point::new(self.origin.x, y),
            Point::new(width, self.line_height()),
        )
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let extent = self.extent();
        if self.fill.is_some() || self.stroke.is_some() {
            out.push(Primitive::Rect {
                origin: self.origin,
                size: extent,
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
        }
        for &n in &self.emphasize {
            if n >= 1 && n <= self.lines.len() {
                let (origin, size) = self.line_rect(n);
                out.push(Primitive::Rect {
                    origin,
                    size,
                    radius: 0.0,
                    fill: Some(self.emphasis),
                    stroke: None,
                });
            }
        }
        let face = self.fonts.face(Font::Mono);
        let ascent = face.ascender() as f64 * self.size / face.units_per_em() as f64;
        let gutter = self.gutter();
        let left = self.origin.x + self.padding.x;
        for (k, runs) in self.lines.iter().enumerate() {
            // each line's baseline sits so its text is centered in the
            // line's height
            let top = self.origin.y + self.padding.y + k as f64 * self.line_height();
            let baseline = top + (self.line_height() - self.size) / 2.0 + ascent * 0.92;
            if let Some(color) = self.numbers {
                let number = Label::plain(
                    &self.fonts,
                    (k + 1).to_string(),
                    self.size,
                    Font::Mono,
                    color,
                );
                out.push(number.at(
                    Point::new(left + gutter - self.size, baseline),
                    HAlign::Right,
                    VAlign::Baseline,
                ));
            }
            if !runs.is_empty() {
                out.push(Primitive::Text {
                    origin: Point::new(left + gutter, baseline),
                    runs: runs.clone(),
                    size: self.size,
                    width: None,
                    h_align: HAlign::Left,
                    v_align: VAlign::Baseline,
                });
            }
        }
    }
}

fn mono(text: String, color: Color) -> Run {
    Run {
        text,
        font: Font::Mono,
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(source: &str, lang: Lang) -> Vec<(String, Class)> {
        highlight(source, lang).into_iter().flatten().collect()
    }

    #[test]
    fn rust_marks_keywords_types_strings_numbers_and_comments() {
        let got = classes(
            "fn key(&self) -> u64 { Foo::new(\"a\\\"b\", 42) } // done",
            Lang::Rust,
        );
        let find = |text: &str| got.iter().find(|(t, _)| t == text).map(|(_, c)| *c);
        assert_eq!(find("fn"), Some(Class::Keyword));
        assert_eq!(find("Foo"), Some(Class::Type));
        assert_eq!(find("\"a\\\"b\""), Some(Class::Str));
        assert_eq!(find("42"), Some(Class::Number));
        assert_eq!(find("// done"), Some(Class::Comment));
    }

    #[test]
    fn a_block_comment_runs_across_lines() {
        let lines = highlight("a /* one\ntwo */ b", Lang::Rust);
        assert_eq!(
            lines[0].last().unwrap(),
            &("/* one".to_string(), Class::Comment)
        );
        assert_eq!(lines[1][0], ("two */".to_string(), Class::Comment));
        assert_eq!(lines[1].last().unwrap(), &(" b".to_string(), Class::Plain));
    }

    #[test]
    fn python_quotes_either_way_and_comments_with_a_hash() {
        let got = classes("def f(x): return 'a' # c", Lang::Python);
        assert!(got.contains(&("def".into(), Class::Keyword)));
        assert!(got.contains(&("'a'".into(), Class::Str)));
        assert!(got.contains(&("# c".into(), Class::Comment)));
    }
}
