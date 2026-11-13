//! Edits the source editor makes for the person: indenting new lines,
//! toggling comments, finding the matching brace, and completing words.
//! Offsets are bytes into the text.

use std::ops::Range;

use acre::lang::{self, Highlight};
use acre::scene::schema;

const INDENT: &str = "    ";

/// The text and cursor after the editor's own touch on an edit that
/// turned `old` into `new` with the cursor at `cursor`, or `None` when it
/// adds nothing. A new line takes the indent of the line before, one
/// more after a `{`. A `}` typed first on a line moves one indent left.
pub fn after_edit(old: &str, new: &str, cursor: usize) -> Option<(String, usize)> {
    if new.len() != old.len() + 1 || cursor == 0 || !new.is_char_boundary(cursor - 1) {
        return None;
    }
    let typed = new[..cursor].chars().next_back()?;
    if old[..cursor - 1] != new[..cursor - 1] || old[cursor - 1..] != new[cursor..] {
        return None;
    }
    let line_start = |at: usize| new[..at].rfind('\n').map_or(0, |i| i + 1);
    match typed {
        '\n' => {
            let before = &new[line_start(cursor - 1)..cursor - 1];
            let mut indent: String = before.chars().take_while(|c| *c == ' ').collect();
            if before.trim_end().ends_with('{') {
                indent.push_str(INDENT);
            }
            if indent.is_empty() {
                return None;
            }
            let mut text = new.to_string();
            text.insert_str(cursor, &indent);
            Some((text, cursor + indent.len()))
        }
        '}' => {
            let start = line_start(cursor - 1);
            let before = &new[start..cursor - 1];
            if before.len() < INDENT.len() || !before.chars().all(|c| c == ' ') {
                return None;
            }
            let mut text = new.to_string();
            text.replace_range(cursor - 1 - INDENT.len()..cursor - 1, "");
            Some((text, cursor - INDENT.len()))
        }
        _ => None,
    }
}

/// The text with `// ` put before every line `selection` touches, or
/// taken off when each of those lines already has it, and the new
/// selection.
pub fn toggle_comment(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let start = text[..selection.start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[selection.end..]
        .find('\n')
        .map_or(text.len(), |i| selection.end + i);
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let commented = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim_start().starts_with("//"));
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let changed: Vec<String> = lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                l.to_string()
            } else if commented {
                let at = l.len() - l.trim_start().len();
                let rest = &l[at + 2..];
                format!("{}{}", &l[..at], rest.strip_prefix(' ').unwrap_or(rest))
            } else {
                format!("{}// {}", &l[..indent], &l[indent..])
            }
        })
        .collect();
    let block = changed.join("\n");
    let mut out = String::with_capacity(text.len() + lines.len() * 3);
    out.push_str(&text[..start]);
    out.push_str(&block);
    out.push_str(&text[end..]);
    (out, start..start + block.len())
}

/// The braces of the pair the cursor at `at` touches, the brace just
/// before or just after it, as the offsets of the opening and closing
/// brace. Braces inside strings and comments do not count.
pub fn matching_brace(text: &str, at: usize) -> Option<(usize, usize)> {
    let braces: Vec<(usize, bool)> = lang::highlight(text)
        .into_iter()
        .filter(|(_, class)| *class == Highlight::Punct)
        .filter_map(|(span, _)| match &text[span.start..span.end] {
            "{" => Some((span.start, true)),
            "}" => Some((span.start, false)),
            _ => None,
        })
        .collect();
    let mut stack = Vec::new();
    let mut pairs = Vec::new();
    for (offset, open) in braces {
        if open {
            stack.push(offset);
        } else if let Some(o) = stack.pop() {
            pairs.push((o, offset));
        }
    }
    let touches = |b: usize| b == at || b + 1 == at;
    pairs.into_iter().find(|&(o, c)| touches(o) || touches(c))
}

/// What completing the word at `at` would offer: the range the word
/// covers and the words that could replace it.
pub struct Completion {
    pub replace: Range<usize>,
    pub candidates: Vec<&'static str>,
}

/// The completions for the word ending at `at`: the properties and
/// child kinds of the node it sits in when it starts an item, and the
/// fixed words of its property's values otherwise.
pub fn complete(text: &str, at: usize) -> Option<Completion> {
    let ctx = lang::context_at(text, at);
    let (span, prefix) = ctx.word.clone()?;
    let kind = ctx.nodes.last().and_then(|n| schema::node(n));
    let mut words: Vec<&'static str> = Vec::new();
    if ctx.at_item_start {
        match kind {
            Some(kind) => {
                words.extend(kind.keys());
                words.extend(kind.children.iter().copied());
            }
            None => words.push("canvas"),
        }
    } else if let (Some(kind), Some(key)) = (kind, ctx.key.as_deref())
        && let Some(property) = kind.property(key)
    {
        words.extend(
            property
                .takes
                .split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
                .filter(|w| !w.is_empty()),
        );
    }
    words.retain(|w| w.starts_with(prefix.as_str()) && *w != prefix);
    words.sort_unstable();
    words.dedup();
    if words.is_empty() {
        return None;
    }
    Some(Completion {
        replace: span.start..at,
        candidates: words,
    })
}

/// The longest text every one of `words` starts with.
pub fn common_prefix<'a>(words: &[&'a str]) -> &'a str {
    let Some((first, rest)) = words.split_first() else {
        return "";
    };
    let mut len = first.len();
    for w in rest {
        len = first
            .bytes()
            .zip(w.bytes())
            .take(len)
            .take_while(|(a, b)| a == b)
            .count();
    }
    &first[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_line_keeps_the_indent_and_adds_one_after_a_brace() {
        let old = "canvas {\n    box {}";
        let new = "canvas {\n    box {\n}";
        let at = old.find("{}").unwrap() + 2;
        assert_eq!(
            after_edit(old, new, at),
            Some(("canvas {\n    box {\n        }".into(), at + 8))
        );
    }

    #[test]
    fn a_closing_brace_first_on_a_line_moves_left() {
        let old = "canvas {\n    box {\n        ";
        let new = "canvas {\n    box {\n        }";
        assert_eq!(
            after_edit(old, new, new.len()),
            Some(("canvas {\n    box {\n    }".into(), new.len() - 4))
        );
    }

    #[test]
    fn comments_go_on_at_the_shallowest_indent_and_come_off_again() {
        let text = "a {\n    b;\n        c;\n}";
        let sel = text.find("b;").unwrap()..text.find("c;").unwrap();
        let (on, sel) = toggle_comment(text, sel);
        assert_eq!(on, "a {\n    // b;\n    //     c;\n}");
        let (off, _) = toggle_comment(&on, sel);
        assert_eq!(off, text);
    }

    #[test]
    fn the_brace_beside_the_cursor_finds_its_pair_past_a_string() {
        let text = "a { b \"}\"; c { } }";
        assert_eq!(matching_brace(text, 3), Some((2, 17)));
        assert_eq!(matching_brace(text, 18), Some((2, 17)));
        assert_eq!(matching_brace(text, 6), None);
    }

    #[test]
    fn completion_offers_a_nodes_properties_and_a_propertys_words() {
        let text = "canvas { box { st";
        let c = complete(text, text.len()).unwrap();
        assert_eq!(c.candidates, ["stroke", "stroke-width", "style"]);
        assert_eq!(common_prefix(&c.candidates), "st");
        let text = "canvas { text { align ce";
        let c = complete(text, text.len()).unwrap();
        assert_eq!(c.candidates, ["center"]);
        assert_eq!(&text[c.replace], "ce");
    }
}
