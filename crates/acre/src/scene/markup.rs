use std::collections::HashMap;

use super::primitive::{Color, Font, Run};

/// The runs a label's text marks up, starting in `font` and `color`.
///
/// `` `code` `` sets text in the mono font, `**bold**` in the bold sans
/// font, and `{NAME text}` colors `text` with the color `colors` gives
/// `NAME`, or with `{#rrggbb text}`. A backslash takes the character
/// after it as it is.
pub fn runs(
    src: &str,
    font: Font,
    color: Color,
    colors: &HashMap<String, Color>,
) -> Result<Vec<Run>, String> {
    let mut out: Vec<Run> = Vec::new();
    let mut mono = false;
    let mut bold = false;
    let mut stack: Vec<Color> = Vec::new();
    let mut chars = src.chars().peekable();
    let mut text = String::new();

    // the run being built is pushed whenever the style changes, with
    // the style it was written in
    let flush = |out: &mut Vec<Run>, text: &mut String, mono: bool, bold: bool, stack: &[Color]| {
        if text.is_empty() {
            return;
        }
        let font = match (mono, bold) {
            (true, _) => Font::Mono,
            (false, true) => Font::SansBold,
            (false, false) => font,
        };
        let color = stack.last().copied().unwrap_or(color);
        match out.last_mut() {
            Some(last) if last.font == font && last.color == color => last.text.push_str(text),
            _ => out.push(Run {
                text: text.clone(),
                font,
                color,
            }),
        }
        text.clear();
    };

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(c) => text.push(c),
                None => return Err("a `\\` at the end of the text escapes nothing".into()),
            },
            '`' => {
                flush(&mut out, &mut text, mono, bold, &stack);
                mono = !mono;
            }
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                flush(&mut out, &mut text, mono, bold, &stack);
                bold = !bold;
            }
            '{' => {
                flush(&mut out, &mut text, mono, bold, &stack);
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == ' ' {
                        break;
                    }
                    name.push(c);
                }
                let color = match name.strip_prefix('#') {
                    Some(hex) => {
                        hex_color(hex).ok_or_else(|| format!("`#{hex}` is not a color"))?
                    }
                    None => *colors
                        .get(&name)
                        .ok_or_else(|| format!("no color is named `{name}`"))?,
                };
                stack.push(color);
            }
            '}' => {
                flush(&mut out, &mut text, mono, bold, &stack);
                if stack.pop().is_none() {
                    return Err("a `}` closes no `{`".into());
                }
            }
            c => text.push(c),
        }
    }
    if !stack.is_empty() {
        return Err("a `{` is never closed".into());
    }
    flush(&mut out, &mut text, mono, bold, &stack);
    Ok(out)
}

fn hex_color(hex: &str) -> Option<Color> {
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let mut rgba = [255u8; 4];
    for (k, channel) in rgba.iter_mut().enumerate().take(hex.len() / 2) {
        *channel = u8::from_str_radix(hex.get(2 * k..2 * k + 2)?, 16).ok()?;
    }
    let [r, g, b, a] = rgba.map(|v| v as f32 / 255.0);
    Some(Color::rgba(r, g, b, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);

    fn styles(src: &str) -> Vec<(String, Font, Color)> {
        let colors = HashMap::from([("red".to_string(), RED)]);
        runs(src, Font::Sans, Color::BLACK, &colors)
            .unwrap()
            .into_iter()
            .map(|r| (r.text, r.font, r.color))
            .collect()
    }

    #[test]
    fn marks_switch_font_and_color_and_nest() {
        assert_eq!(
            styles("a `b` {red c **d** `e`} \\{f\\}"),
            [
                ("a ".into(), Font::Sans, Color::BLACK),
                ("b".into(), Font::Mono, Color::BLACK),
                (" ".into(), Font::Sans, Color::BLACK),
                ("c ".into(), Font::Sans, RED),
                ("d".into(), Font::SansBold, RED),
                (" ".into(), Font::Sans, RED),
                ("e".into(), Font::Mono, RED),
                (" {f}".into(), Font::Sans, Color::BLACK),
            ]
        );
    }

    #[test]
    fn an_unknown_color_or_an_open_brace_is_an_error() {
        let colors = HashMap::new();
        assert_eq!(
            runs("{blue x}", Font::Sans, Color::BLACK, &colors).unwrap_err(),
            "no color is named `blue`"
        );
        assert_eq!(
            runs("{#ff0000 x", Font::Sans, Color::BLACK, &colors).unwrap_err(),
            "a `{` is never closed"
        );
    }
}
