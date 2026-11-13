use super::{Error, Span};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
    Word(String),
    Number(f64),
    Str(String),
    Color([u8; 4]),
    Open,
    Close,
    Semi,
    LParen,
    RParen,
    /// `+`, `-`, `*` or `/` inside parentheses.
    Op(char),
    Comment,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub(super) fn tokens(src: &str) -> Result<Vec<Token>, Error> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(token) = next_token(src, &mut i) {
        let token = token?;
        if token.tok != Tok::Comment {
            out.push(token);
        }
    }
    Ok(out)
}

/// The token starting at or after `*i`, with `*i` moved past it, or
/// `None` at the end of the source.
pub(super) fn next_token(src: &str, i: &mut usize) -> Option<Result<Token, Error>> {
    let bytes = src.as_bytes();
    while bytes.get(*i).is_some_and(|b| b.is_ascii_whitespace()) {
        *i += 1;
    }
    let start = *i;
    let &b = bytes.get(start)?;
    let tok = match b {
        b'/' if bytes.get(start + 1) == Some(&b'/') => {
            while *i < bytes.len() && bytes[*i] != b'\n' {
                *i += 1;
            }
            Tok::Comment
        }
        b'{' => {
            *i += 1;
            Tok::Open
        }
        b'}' => {
            *i += 1;
            Tok::Close
        }
        b';' => {
            *i += 1;
            Tok::Semi
        }
        b'"' if src[start..].starts_with("\"\"\"") => match block(src, start) {
            Ok((text, end)) => {
                *i = end;
                Tok::Str(text)
            }
            Err(e) => return Some(Err(e)),
        },
        b'"' => match string(src, start) {
            Ok((text, end)) => {
                *i = end;
                Tok::Str(text)
            }
            Err(e) => return Some(Err(e)),
        },
        b'#' => {
            *i += 1;
            while *i < bytes.len() && bytes[*i].is_ascii_alphanumeric() {
                *i += 1;
            }
            match color(&src[start + 1..*i], Span { start, end: *i }) {
                Ok(c) => Tok::Color(c),
                Err(e) => return Some(Err(e)),
            }
        }
        b'(' => {
            *i += 1;
            Tok::LParen
        }
        b')' => {
            *i += 1;
            Tok::RParen
        }
        b'+' | b'*' | b'/' => {
            *i += 1;
            Tok::Op(b as char)
        }
        b'-' if !bytes
            .get(start + 1)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.') =>
        {
            *i += 1;
            Tok::Op('-')
        }
        b'-' | b'0'..=b'9' => {
            if b == b'-' {
                *i += 1;
            }
            while *i < bytes.len() && (bytes[*i].is_ascii_digit() || bytes[*i] == b'.') {
                *i += 1;
            }
            let span = Span { start, end: *i };
            let text = &src[start..*i];
            match text.parse() {
                Ok(value) => Tok::Number(value),
                Err(_) => return Some(Err(Error::new(span, format!("`{text}` is not a number")))),
            }
        }
        b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
            while *i < bytes.len()
                && (bytes[*i].is_ascii_alphanumeric() || bytes[*i] == b'_' || bytes[*i] == b'-')
            {
                *i += 1;
            }
            Tok::Word(src[start..*i].to_string())
        }
        _ => {
            let c = src[start..].chars().next().unwrap();
            let span = Span {
                start,
                end: start + c.len_utf8(),
            };
            return Some(Err(Error::new(span, format!("unexpected character `{c}`"))));
        }
    };
    Some(Ok(Token {
        tok,
        span: Span { start, end: *i },
    }))
}

/// The text of the string literal opening at `start`, and the offset
/// just past its closing quote.
fn string(src: &str, start: usize) -> Result<(String, usize), Error> {
    let mut text = String::new();
    let mut chars = src[start + 1..].char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Ok((text, start + 1 + at + 1)),
            '\\' => match chars.next() {
                Some((_, '"')) => text.push('"'),
                Some((_, '\\')) => text.push('\\'),
                Some((_, 'n')) => text.push('\n'),

                // kept with their backslash for the markup in labels
                // to read
                Some((_, c @ ('{' | '}' | '`' | '*'))) => {
                    text.push('\\');
                    text.push(c);
                }
                Some((esc, c)) => {
                    let at = start + 1 + esc;
                    let span = Span {
                        start: at - 1,
                        end: at + c.len_utf8(),
                    };
                    return Err(Error::new(span, format!("unknown escape `\\{c}`")));
                }
                None => break,
            },
            c => text.push(c),
        }
    }
    let span = Span {
        start,
        end: start + 1,
    };
    Err(Error::new(span, "string is never closed"))
}

/// The text of the block string opening with `"""` at `start`, and the
/// offset just past its closing `"""`. It takes no escapes. A line break
/// straight after the opening and the last line, when it holds only
/// spaces, are dropped, and so is the indent its lines share.
fn block(src: &str, start: usize) -> Result<(String, usize), Error> {
    let body = start + 3;
    let Some(len) = src[body..].find("\"\"\"") else {
        let span = Span { start, end: body };
        return Err(Error::new(span, "block string is never closed"));
    };
    let raw = &src[body..body + len];
    let raw = raw.strip_prefix('\n').unwrap_or(raw);
    let mut lines: Vec<&str> = raw.split('\n').collect();
    if lines.len() > 1 && lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let text = lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or("").trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    Ok((text, body + len + 3))
}

fn color(hex: &str, span: Span) -> Result<[u8; 4], Error> {
    let bad = || Error::new(span, "a color is `#rrggbb` or `#rrggbbaa`");
    if hex.len() != 6 && hex.len() != 8 {
        return Err(bad());
    }
    let mut rgba = [255; 4];
    for (k, channel) in rgba.iter_mut().enumerate().take(hex.len() / 2) {
        *channel = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).map_err(|_| bad())?;
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_string_keeps_its_lines_without_their_shared_indent() {
        let src = "text \"\"\"\n        fn f() {\n            \\n\n        }\n    \"\"\";";
        assert_eq!(
            toks(src),
            [
                Tok::Word("text".into()),
                Tok::Str("fn f() {\n    \\n\n}".into()),
                Tok::Semi,
            ]
        );
        assert_eq!(toks("\"\"\"one line\"\"\""), [Tok::Str("one line".into())]);
        let err = tokens("a \"\"\"never closed").unwrap_err();
        assert_eq!(err.message, "block string is never closed");
    }

    fn toks(src: &str) -> Vec<Tok> {
        tokens(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn every_token_kind_is_read() {
        assert_eq!(
            toks(r#"box a-1 { at -4 2.5; text "say \"hi\""; fill #ff8000; } // done"#),
            vec![
                Tok::Word("box".into()),
                Tok::Word("a-1".into()),
                Tok::Open,
                Tok::Word("at".into()),
                Tok::Number(-4.0),
                Tok::Number(2.5),
                Tok::Semi,
                Tok::Word("text".into()),
                Tok::Str("say \"hi\"".into()),
                Tok::Semi,
                Tok::Word("fill".into()),
                Tok::Color([255, 128, 0, 255]),
                Tok::Semi,
                Tok::Close,
            ]
        );
    }

    #[test]
    fn a_token_span_covers_its_text() {
        let src = "  box \"é\"";
        let spans: Vec<&str> = tokens(src)
            .unwrap()
            .iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert_eq!(spans, ["box", "\"é\""]);
    }

    #[test]
    fn an_unclosed_string_points_at_its_opening_quote() {
        let err = tokens("text \"abc").unwrap_err();
        assert_eq!(err.span, Span { start: 5, end: 6 });
    }

    #[test]
    fn a_color_must_have_six_or_eight_digits() {
        assert!(tokens("#fff").is_err());
        assert_eq!(toks("#00000080"), vec![Tok::Color([0, 0, 0, 128])]);
    }
}
