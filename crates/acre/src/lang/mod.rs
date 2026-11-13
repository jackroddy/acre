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

mod format;
mod lex;

pub use format::format;

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
    /// The values between `(` and `)`, an arithmetic expression that
    /// `scene` evaluates once names are replaced.
    Group(Vec<Value>),
    /// An operator inside a group.
    Op(char),
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
    /// A word a `let` names.
    Name,
    Number,
    Str,
    /// A markup sign inside a string: `` ` ``, `**`, `{name`, `}` or `$`.
    Markup,
    /// A color, or a `let` name that stands for one, with its value.
    Color([u8; 4]),
    Punct,
    Comment,
    /// Text the lexer cannot read.
    Invalid,
}

/// Every token in `src` with its class, in source order, with strings
/// split at their markup. Unlike [`parse`], this reads to the end past
/// any error.
pub fn highlight(src: &str) -> Vec<(Span, Highlight)> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(token) = lex::next_token(src, &mut i) {
        match token {
            Ok(token) => tokens.push(Ok(token)),
            Err(e) => {
                // every lexer error's span ends past where its
                // token started, so this always moves forward
                i = i.max(e.span.end);
                tokens.push(Err(e.span));
            }
        }
    }
    let lets = let_names(&tokens);

    let mut out = Vec::new();
    let mut item_start = true;
    for token in &tokens {
        let token = match token {
            Ok(token) => token,
            Err(span) => {
                out.push((*span, Highlight::Invalid));
                continue;
            }
        };
        let class = match &token.tok {
            Tok::Word(_) if item_start => Highlight::Key,
            Tok::Word(w) => match lets.get(w.as_str()) {
                Some(Some(rgba)) => Highlight::Color(*rgba),
                Some(None) => Highlight::Name,
                None => Highlight::Word,
            },
            Tok::Number(_) => Highlight::Number,
            Tok::Str(_) => {
                markup(src, token.span, &mut out);
                item_start = false;
                continue;
            }
            Tok::Color(rgba) => Highlight::Color(*rgba),
            Tok::Open | Tok::Close | Tok::Semi | Tok::LParen | Tok::RParen | Tok::Op(_) => {
                Highlight::Punct
            }
            Tok::Comment => Highlight::Comment,
        };
        if class != Highlight::Comment {
            item_start = matches!(token.tok, Tok::Open | Tok::Close | Tok::Semi);
        }
        out.push((token.span, class));
    }
    out
}

/// The names `let` lines define, each with its color when its value is
/// one color.
fn let_names(tokens: &[Result<Token, Span>]) -> std::collections::HashMap<&str, Option<[u8; 4]>> {
    let mut out = std::collections::HashMap::new();
    let mut item_start = true;
    for (k, token) in tokens.iter().enumerate() {
        let Ok(token) = token else { continue };
        if item_start
            && token.tok == Tok::Word("let".into())
            && let Some(Ok(Token {
                tok: Tok::Word(name),
                ..
            })) = tokens.get(k + 1)
        {
            let value = match (tokens.get(k + 2), tokens.get(k + 3)) {
                (
                    Some(Ok(Token {
                        tok: Tok::Color(rgba),
                        ..
                    })),
                    Some(Ok(Token { tok: Tok::Semi, .. })),
                ) => Some(*rgba),
                _ => None,
            };
            out.insert(name.as_str(), value);
        }
        if token.tok != Tok::Comment {
            item_start = matches!(token.tok, Tok::Open | Tok::Close | Tok::Semi);
        }
    }
    out
}

/// Append the string token at `span` to `out`, as `Str` pieces split by
/// `Markup` signs.
fn markup(src: &str, span: Span, out: &mut Vec<(Span, Highlight)>) {
    let text = &src[span.start..span.end];
    let mut push = |start: usize, end: usize, class: Highlight| {
        if start < end {
            let span = Span {
                start: span.start + start,
                end: span.start + end,
            };
            match out.last_mut() {
                Some((last, c)) if *c == class && last.end == span.start => last.end = span.end,
                _ => out.push((span, class)),
            }
        }
    };
    let bytes = text.as_bytes();
    let mut k = 0;
    while k < bytes.len() {
        let (len, class) = match bytes[k] {
            b'\\' => (2.min(bytes.len() - k), Highlight::Str),
            b'`' | b'}' => (1, Highlight::Markup),
            b'$' => {
                // a formula's braces and backslashes are LaTeX, not
                // markup, so it stays one `Str` between its `$` signs
                let mut end = k + 1;
                while end < bytes.len() && bytes[end] != b'$' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                push(k, k + 1, Highlight::Markup);
                push(k + 1, end, Highlight::Str);
                k = end;
                (1.min(bytes.len() - end), Highlight::Markup)
            }
            b'*' if bytes.get(k + 1) == Some(&b'*') => (2, Highlight::Markup),
            b'{' => {
                let name = bytes[k..]
                    .iter()
                    .position(|&b| b == b' ' || b == b'"')
                    .unwrap_or(bytes.len() - k);
                (name, Highlight::Markup)
            }
            _ => {
                let c = text[k..].chars().next().map_or(1, char::len_utf8);
                (c, Highlight::Str)
            }
        };
        push(k, k + len, class);
        k += len;
    }
}

/// Where a byte offset sits in the source, for an editor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    /// The kinds of the nodes around the offset, outermost first.
    pub nodes: Vec<String>,
    /// The first word of the property the offset is in.
    pub key: Option<String>,
    /// Whether the offset is in the first word of an item, where a
    /// property or a node's kind goes.
    pub at_item_start: bool,
    /// The word the offset is in or just after, and where it is.
    pub word: Option<(Span, String)>,
}

/// The context at byte `at` of `src`. Unlike [`parse`], this reads
/// source with errors in it.
pub fn context_at(src: &str, at: usize) -> Context {
    let mut out = Context::default();
    let mut nodes: Vec<String> = Vec::new();
    let mut item: Option<String> = None;
    let mut item_start = true;
    let mut i = 0;
    while let Some(token) = lex::next_token(src, &mut i) {
        let Ok(token) = token else { continue };
        if token.span.start > at {
            break;
        }
        // a word the offset is inside or just after
        if let Tok::Word(w) = &token.tok
            && token.span.start <= at
            && at <= token.span.end
        {
            out.word = Some((token.span, w[..at - token.span.start].to_string()));
            out.at_item_start = item_start;
            out.nodes = nodes.clone();
            out.key = if item_start { None } else { item.clone() };
            return out;
        }
        if token.span.end > at {
            break;
        }
        match &token.tok {
            Tok::Open => {
                nodes.push(item.take().unwrap_or_default());
                item_start = true;
            }
            Tok::Close => {
                nodes.pop();
                item = None;
                item_start = true;
            }
            Tok::Semi => {
                item = None;
                item_start = true;
            }
            Tok::Comment => {}
            Tok::Word(w) if item_start => {
                item = Some(w.clone());
                item_start = false;
            }
            _ => item_start = false,
        }
    }
    out.nodes = nodes;
    out.at_item_start = item_start;
    out.key = if item_start { None } else { item };
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
                Tok::LParen => {
                    values.push(self.group(token.span)?);
                    continue;
                }
                Tok::RParen | Tok::Op(_) => {
                    return Err(Error::new(
                        token.span,
                        format!("{} belongs inside `( )`", describe(&token.tok)),
                    ));
                }
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

impl Parser<'_> {
    /// The values up to the `)` matching `open`, as one group.
    fn group(&mut self, open: Span) -> Result<Value, Error> {
        let mut values = Vec::new();
        loop {
            let Some(token) = self.tokens.get(self.pos) else {
                return Err(Error::new(open, "this `(` is never closed"));
            };
            self.pos += 1;
            let kind = match &token.tok {
                Tok::Number(n) => ValueKind::Number(*n),
                Tok::Word(w) => ValueKind::Word(w.clone()),
                Tok::Op(c) => ValueKind::Op(*c),
                Tok::LParen => {
                    values.push(self.group(token.span)?);
                    continue;
                }
                Tok::RParen => {
                    return Ok(Value {
                        kind: ValueKind::Group(values),
                        span: Span {
                            start: open.start,
                            end: token.span.end,
                        },
                    });
                }
                other => {
                    return Err(Error::new(
                        token.span,
                        format!(
                            "expected a number, a name or an operator, found {}",
                            describe(other)
                        ),
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
        Tok::LParen => "`(`",
        Tok::RParen => "`)`",
        Tok::Op(_) => "an operator",
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
    fn highlight_splits_strings_at_markup_and_knows_let_names() {
        let src = "canvas { let ink #102030; let u 4; text { at (u) 0; text \"a `b` {ink c}\"; color ink; } }";
        let classes: Vec<(&str, Highlight)> = highlight(src)
            .into_iter()
            .map(|(span, class)| (&src[span.start..span.end], class))
            .filter(|(_, c)| *c != Highlight::Punct)
            .collect();
        let ink = Highlight::Color([0x10, 0x20, 0x30, 0xff]);
        assert_eq!(
            classes,
            [
                ("canvas", Highlight::Key),
                ("let", Highlight::Key),
                ("ink", ink),
                ("#102030", ink),
                ("let", Highlight::Key),
                ("u", Highlight::Name),
                ("4", Highlight::Number),
                ("text", Highlight::Key),
                ("at", Highlight::Key),
                ("u", Highlight::Name),
                ("0", Highlight::Number),
                ("text", Highlight::Key),
                ("\"a ", Highlight::Str),
                ("`", Highlight::Markup),
                ("b", Highlight::Str),
                ("`", Highlight::Markup),
                (" ", Highlight::Str),
                ("{ink", Highlight::Markup),
                (" c", Highlight::Str),
                ("}", Highlight::Markup),
                ("\"", Highlight::Str),
                ("color", Highlight::Key),
                ("ink", ink),
            ]
        );
    }

    #[test]
    fn highlight_keeps_a_formula_whole_between_its_dollar_signs() {
        let src = r#"text "a $\frac{x}{y}$ {ink b}";"#;
        let classes: Vec<(&str, Highlight)> = highlight(src)
            .into_iter()
            .map(|(span, class)| (&src[span.start..span.end], class))
            .filter(|(_, c)| *c != Highlight::Punct && *c != Highlight::Key)
            .collect();
        assert_eq!(
            classes,
            [
                ("\"a ", Highlight::Str),
                ("$", Highlight::Markup),
                (r"\frac{x}{y}", Highlight::Str),
                ("$", Highlight::Markup),
                (" ", Highlight::Str),
                ("{ink", Highlight::Markup),
                (" b", Highlight::Str),
                ("}", Highlight::Markup),
                ("\"", Highlight::Str),
            ]
        );
    }

    #[test]
    fn context_names_the_nodes_and_the_property_around_an_offset() {
        let src = "canvas { row r { at 1 2; cell { fi";
        let at = |needle: &str| src.find(needle).unwrap() + needle.len();
        let ctx = context_at(src, at("at 1"));
        assert_eq!(ctx.nodes, ["canvas", "row"]);
        assert_eq!(ctx.key.as_deref(), Some("at"));
        assert!(!ctx.at_item_start);
        let ctx = context_at(src, src.len());
        assert_eq!(ctx.nodes, ["canvas", "row", "cell"]);
        assert!(ctx.at_item_start);
        assert_eq!(ctx.word.unwrap().1, "fi");
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
