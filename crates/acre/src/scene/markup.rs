use std::collections::HashMap;

use super::primitive::{Color, Font, Run};

/// The runs a label's text marks up, starting in `font` and `color`.
///
/// `` `code` `` sets text in the mono font, `**bold**` in the bold sans
/// font, and `{NAME text}` colors `text` with the color `colors` gives
/// `NAME`, or with `{#rrggbb text}`. `$formula$` is LaTeX math, as one
/// run in the math font. A line that starts with `- ` is a list item,
/// and its `- ` becomes a bullet run, `•` in `color`. A backslash takes the
/// character after it as it is, so `\$` is a dollar sign and a line that
/// starts with `\- ` starts with a dash.
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
    let mut line_start = true;

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
            Some(last) if !last.bullet && last.font == font && last.color == color => {
                last.text.push_str(text)
            }
            _ => out.push(Run {
                text: text.clone(),
                font,
                color,
                bullet: false,
            }),
        }
        text.clear();
    };

    while let Some(c) = chars.next() {
        let at_line_start = std::mem::replace(&mut line_start, c == '\n');
        match c {
            '-' if at_line_start && chars.peek() == Some(&' ') => {
                chars.next();
                flush(&mut out, &mut text, mono, bold, &stack);
                out.push(Run {
                    text: "\u{2022}".into(),
                    font: Font::Sans,
                    color,
                    bullet: true,
                });
            }
            '\\' => match chars.next() {
                Some(c) => text.push(c),
                None => return Err("a `\\` at the end of the text escapes nothing".into()),
            },
            '`' => {
                flush(&mut out, &mut text, mono, bold, &stack);
                mono = !mono;
            }
            '$' => {
                flush(&mut out, &mut text, mono, bold, &stack);
                let formula = formula(&mut chars)?;
                out.push(Run {
                    text: formula,
                    font: Font::Math,
                    color: stack.last().copied().unwrap_or(color),
                    bullet: false,
                });
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

/// The formula after an opening `$`, up to the `$` that closes it, with
/// its backslashes kept for LaTeX.
fn formula(chars: &mut impl Iterator<Item = char>) -> Result<String, String> {
    let mut formula = String::new();
    loop {
        match chars.next() {
            Some('$') => break,
            Some('\\') => {
                formula.push('\\');
                formula.extend(chars.next());
            }
            Some(c) => formula.push(c),
            None => {
                return Err("a `$` opens a formula that is never closed; \
                            write `\\$` for a dollar sign"
                    .into());
            }
        }
    }
    if formula.trim().is_empty() {
        return Err("a formula between `$` signs is empty; write `\\$` for a dollar sign".into());
    }
    crate::math::parse(&formula).map_err(|e| format!("in `${formula}$`: {e}"))?;
    Ok(formula)
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
    fn a_formula_is_one_math_run_in_the_color_around_it() {
        assert_eq!(
            styles(r"a $\frac{x}{2}$ {red $y$} \$5"),
            [
                ("a ".into(), Font::Sans, Color::BLACK),
                (r"\frac{x}{2}".into(), Font::Math, Color::BLACK),
                (" ".into(), Font::Sans, Color::BLACK),
                ("y".into(), Font::Math, RED),
                (" $5".into(), Font::Sans, Color::BLACK),
            ]
        );
    }

    #[test]
    fn a_lone_dollar_or_a_bad_formula_is_an_error() {
        let colors = HashMap::new();
        let err = |src| runs(src, Font::Sans, Color::BLACK, &colors).unwrap_err();
        assert_eq!(
            err("costs $5"),
            "a `$` opens a formula that is never closed; write `\\$` for a dollar sign"
        );
        assert_eq!(
            err("$x^$"),
            "in `$x^$`: an argument is missing at the end of the formula"
        );
    }

    #[test]
    fn a_line_that_starts_with_a_dash_and_a_space_is_a_list_item() {
        let colors = HashMap::from([("red".to_string(), RED)]);
        let runs: Vec<(String, Font, Color, bool)> = runs(
            "- a\n\\- b\nc - d\n-e\n- **f**\n{red g\n- h}",
            Font::Mono,
            Color::BLACK,
            &colors,
        )
        .unwrap()
        .into_iter()
        .map(|r| (r.text, r.font, r.color, r.bullet))
        .collect();
        let bullet = ("\u{2022}".to_string(), Font::Sans, Color::BLACK, true);
        assert_eq!(
            runs,
            [
                bullet.clone(),
                (
                    "a\n- b\nc - d\n-e\n".into(),
                    Font::Mono,
                    Color::BLACK,
                    false
                ),
                bullet.clone(),
                ("f".into(), Font::SansBold, Color::BLACK, false),
                ("\n".into(), Font::Mono, Color::BLACK, false),
                ("g\n".into(), Font::Mono, RED, false),
                bullet,
                ("h".into(), Font::Mono, RED, false),
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
