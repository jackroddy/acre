use super::lex::{self, Tok, Token};
use super::{Error, parse};

const INDENT: &str = "    ";

/// `src` in acre's one layout: four spaces of indent per open brace,
/// one space between tokens on a line, none inside `( )` or before `;`,
/// and at most one blank line in a row. Comments stay, and so does each
/// line break the author put between two tokens.
///
/// # Errors
///
/// If `src` does not parse.
pub fn format(src: &str) -> Result<String, Error> {
    parse(src)?;
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(token) = lex::next_token(src, &mut i) {
        tokens.push(token?);
    }

    let mut out = String::new();
    let mut depth = 0usize;
    for (k, token) in tokens.iter().enumerate() {
        if token.tok == Tok::Close {
            depth = depth.saturating_sub(1);
        }
        if k > 0 {
            let prev = &tokens[k - 1];
            let lines = src[prev.span.end..token.span.start].matches('\n').count();
            if lines > 0 {
                out.push('\n');
                if lines > 1 {
                    out.push('\n');
                }
                for _ in 0..depth {
                    out.push_str(INDENT);
                }
            } else if spaced(&tokens[..k], token) {
                out.push(' ');
            }
        }
        out.push_str(src[token.span.start..token.span.end].trim_end());
        if token.tok == Tok::Open {
            depth += 1;
        }
    }
    out.push('\n');
    Ok(out)
}

/// Whether a space goes between the last of `before` and `next` when
/// they share a line.
fn spaced(before: &[Token], next: &Token) -> bool {
    let prev = &before[before.len() - 1].tok;
    let unary = |k: usize| {
        before.len() > k && matches!(before[before.len() - 1 - k].tok, Tok::LParen | Tok::Op(_))
    };
    match (prev, &next.tok) {
        (_, Tok::Semi | Tok::RParen) => false,
        (Tok::LParen, _) => false,

        // a minus straight after `(` or another operator negates the
        // term after it
        (Tok::Op('-'), _) if before.len() == 1 || unary(1) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_and_indent_are_redone_and_line_breaks_kept() {
        let src = "canvas{size  10 10 ;\n\n\n  let u (  4*2 - -1 );  let v (-u);\n\
                   box a{at 0 0;size 2 2;}   // a box\n      group {\n at 1 1;\n}\n}";
        assert_eq!(
            format(src).unwrap(),
            "canvas { size 10 10;\n\
             \n    let u (4 * 2 - -1); let v (-u);\n\
             \x20   box a { at 0 0; size 2 2; } // a box\n\
             \x20   group {\n        at 1 1;\n    }\n}\n"
        );
    }

    #[test]
    fn formatting_twice_changes_nothing() {
        // the fixture uses lets, styles, groups, arithmetic and markup
        let fixture = "canvas {
    size 100 100;
    let u (4 * 2);
    style s { fill #ffffff; }

    group {
        at 0 (-u);
        row r { at 0 0; unit u 10; chars \"ab\"; } // a row
        text { at r 0 (u + 2); text \"`x` {#ff0000 y}\"; style s; }
    }
}
";
        assert_eq!(format(fixture).unwrap(), fixture);
        for src in [include_str!("../../examples/layers.acre"), fixture] {
            let once = format(src).unwrap();
            assert_eq!(format(&once).unwrap(), once);
        }
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_alone() {
        assert!(format("canvas { size 1 1").is_err());
    }
}
