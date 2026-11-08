//! Source text to a [`Program`].
//!
//! A program is a tree of nodes. A node is a kind, an optional name and a
//! braced body of properties and child nodes:
//!
//! ```text
//! box lang { at 40 76; size 120 48; text "lang"; }
//! ```
//!
//! The parser gives no node kind or property a meaning. `scene` does.

mod lex;

use std::fmt;

use lex::{Tok, Token};

/// A byte range in the source, `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A parsed file: its top-level nodes in source order.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub nodes: Vec<Node>,
}

/// A kind, an optional name, and a braced body.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Word,
    pub name: Option<Word>,
    pub properties: Vec<Property>,
    pub children: Vec<Node>,
    /// From the kind to the closing brace.
    pub span: Span,
}

/// A bare word in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub span: Span,
}

/// A key and the values after it, up to a semicolon.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: Word,
    pub values: Vec<Value>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub kind: ValueKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    Number(f64),
    Str(String),
    /// `#rrggbb` or `#rrggbbaa`, with alpha 255 when absent.
    Color([u8; 4]),
    Word(String),
}

/// A problem with the source, and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub message: String,
    pub span: Span,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            span,
        }
    }

    /// The 1-based line and column of the error's start in `src`.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        let before = &src[..self.span.start.min(src.len())];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        (line, before[line_start..].chars().count() + 1)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Parse a whole file.
pub fn parse(src: &str) -> Result<Program, Error> {
    let tokens = lex::tokens(src)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
    };
    let (properties, nodes) = parser.body(None)?;
    if let Some(p) = properties.first() {
        return Err(Error::new(
            p.span,
            format!("property `{}` is outside any node", p.key.text),
        ));
    }
    Ok(Program { nodes })
}

/// What a span of source is, for coloring it in an editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    /// The first word of a property or a node.
    Key,
    Word,
    Number,
    Str,
    Color,
    Punct,
    Comment,
    /// Text the lexer cannot read.
    Invalid,
}

/// Every token in `src` with its class, in source order. Unlike
/// [`parse`], this reads to the end past any error.
pub fn highlight(src: &str) -> Vec<(Span, Highlight)> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut item_start = true;
    while let Some(token) = lex::next_token(src, &mut i) {
        let token = match token {
            Ok(token) => token,
            Err(e) => {
                // every lexer error's span ends past where its
                // token started, so this always moves forward
                i = i.max(e.span.end);
                out.push((e.span, Highlight::Invalid));
                continue;
            }
        };
        let class = match token.tok {
            Tok::Word(_) if item_start => Highlight::Key,
            Tok::Word(_) => Highlight::Word,
            Tok::Number(_) => Highlight::Number,
            Tok::Str(_) => Highlight::Str,
            Tok::Color(_) => Highlight::Color,
            Tok::Open | Tok::Close | Tok::Semi => Highlight::Punct,
            Tok::Comment => Highlight::Comment,
        };
        if class != Highlight::Comment {
            item_start = class == Highlight::Punct;
        }
        out.push((token.span, class));
    }
    out
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl Parser<'_> {
    /// The properties and nodes up to the `}` matching `open`, or to the
    /// end of the file when `open` is `None`.
    fn body(&mut self, open: Option<Span>) -> Result<(Vec<Property>, Vec<Node>), Error> {
        let mut properties = Vec::new();
        let mut nodes = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.pos) else {
                return match open {
                    Some(open) => Err(Error::new(open, "this `{` is never closed")),
                    None => Ok((properties, nodes)),
                };
            };
            match &token.tok {
                Tok::Close if open.is_some() => {
                    self.pos += 1;
                    return Ok((properties, nodes));
                }
                Tok::Word(text) => {
                    let key = Word {
                        text: text.clone(),
                        span: token.span,
                    };
                    self.pos += 1;
                    self.item(key, &mut properties, &mut nodes)?;
                }
                _ => {
                    return Err(Error::new(
                        token.span,
                        format!(
                            "expected a property or a node, found {}",
                            describe(&token.tok)
                        ),
                    ));
                }
            }
        }
    }

    /// The rest of a property or a node, whose first word is `key`.
    fn item(
        &mut self,
        key: Word,
        properties: &mut Vec<Property>,
        nodes: &mut Vec<Node>,
    ) -> Result<(), Error> {
        let mut values = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.pos) else {
                let end = values.last().map_or(key.span, |v: &Value| v.span);
                return Err(Error::new(
                    end,
                    format!("missing `;` after property `{}`", key.text),
                ));
            };
            self.pos += 1;
            let kind = match &token.tok {
                Tok::Comment => continue,
                Tok::Number(n) => ValueKind::Number(*n),
                Tok::Str(s) => ValueKind::Str(s.clone()),
                Tok::Color(c) => ValueKind::Color(*c),
                Tok::Word(w) => ValueKind::Word(w.clone()),
                Tok::Semi => {
                    properties.push(Property {
                        span: Span {
                            start: key.span.start,
                            end: token.span.end,
                        },
                        key,
                        values,
                    });
                    return Ok(());
                }
                Tok::Open => {
                    let name = match values.as_slice() {
                        [] => None,
                        [
                            Value {
                                kind: ValueKind::Word(text),
                                span,
                            },
                        ] => Some(Word {
                            text: text.clone(),
                            span: *span,
                        }),
                        [one] => return Err(Error::new(one.span, "a node's name is a bare word")),
                        [_, extra, ..] => {
                            return Err(Error::new(
                                extra.span,
                                "a node takes at most one name before `{`",
                            ));
                        }
                    };
                    let (props, children) = self.body(Some(token.span))?;
                    let end = self.tokens[self.pos - 1].span.end;
                    nodes.push(Node {
                        span: Span {
                            start: key.span.start,
                            end,
                        },
                        kind: key,
                        name,
                        properties: props,
                        children,
                    });
                    return Ok(());
                }
                Tok::Close => {
                    let end = values.last().map_or(key.span, |v| v.span);
                    return Err(Error::new(
                        end,
                        format!("missing `;` after property `{}`", key.text),
                    ));
                }
            };
            values.push(Value {
                kind,
                span: token.span,
            });
        }
    }
}

fn describe(tok: &Tok) -> &'static str {
    match tok {
        Tok::Word(_) => "a word",
        Tok::Number(_) => "a number",
        Tok::Str(_) => "a string",
        Tok::Color(_) => "a color",
        Tok::Open => "`{`",
        Tok::Close => "`}`",
        Tok::Semi => "`;`",
        Tok::Comment => "a comment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_at(src: &str) -> (usize, usize, String) {
        let err = parse(src).unwrap_err();
        let (line, col) = err.line_col(src);
        (line, col, err.message)
    }

    #[test]
    fn nodes_nest_and_keep_their_properties() {
        let program = parse(
            "canvas {\n  size 640 200;\n  box a { at 1 2; text \"hi\"; }\n  arrow { from a; }\n}",
        )
        .unwrap();
        let canvas = &program.nodes[0];
        assert_eq!(canvas.kind.text, "canvas");
        assert_eq!(canvas.properties[0].key.text, "size");
        assert_eq!(canvas.properties[0].values.len(), 2);
        assert_eq!(canvas.children.len(), 2);
        let b = &canvas.children[0];
        assert_eq!(b.name.as_ref().unwrap().text, "a");
        assert_eq!(b.properties[1].values[0].kind, ValueKind::Str("hi".into()));
        assert_eq!(canvas.children[1].name, None);
    }

    #[test]
    fn a_node_span_runs_from_its_kind_to_its_closing_brace() {
        let src = "x { box b { at 1 2; } }";
        let node = &parse(src).unwrap().nodes[0].children[0];
        assert_eq!(&src[node.span.start..node.span.end], "box b { at 1 2; }");
    }

    #[test]
    fn errors_name_a_line_and_column() {
        assert_eq!(
            err_at("canvas {\n  size 640 200\n}"),
            (2, 12, "missing `;` after property `size`".into())
        );
        assert_eq!(
            err_at("canvas {\n  box {"),
            (2, 7, "this `{` is never closed".into())
        );
        assert_eq!(
            err_at("canvas {\n}\n}"),
            (3, 1, "expected a property or a node, found `}`".into())
        );
        assert_eq!(
            err_at("size 1 2;"),
            (1, 1, "property `size` is outside any node".into())
        );
        assert_eq!(
            err_at("box a b {}"),
            (1, 7, "a node takes at most one name before `{`".into())
        );
    }

    #[test]
    fn highlight_marks_the_first_word_of_each_item_as_a_key() {
        let src = "box a { // note\n fill none; }";
        let classes: Vec<(&str, Highlight)> = highlight(src)
            .into_iter()
            .map(|(span, class)| (&src[span.start..span.end], class))
            .collect();
        assert_eq!(
            classes,
            [
                ("box", Highlight::Key),
                ("a", Highlight::Word),
                ("{", Highlight::Punct),
                ("// note", Highlight::Comment),
                ("fill", Highlight::Key),
                ("none", Highlight::Word),
                (";", Highlight::Punct),
                ("}", Highlight::Punct),
            ]
        );
    }

    #[test]
    fn highlight_reads_past_an_error() {
        let src = "a @ b";
        let classes: Vec<Highlight> = highlight(src).into_iter().map(|(_, c)| c).collect();
        assert_eq!(
            classes,
            [Highlight::Key, Highlight::Invalid, Highlight::Word]
        );
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        let src = "x { text \"é\"; @ }";
        assert_eq!(err_at(src).1, 15);
    }
}
