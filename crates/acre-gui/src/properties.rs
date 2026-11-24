use acre::lang::{Span, ValueKind};
use acre::scene::edit::{self, Origin};
use acre::scene::schema;
use eframe::egui;

/// The properties the panel lists, in its order, for the kinds that
/// take them.
pub const KEYS: [&str; 8] = [
    "fill",
    "stroke",
    "stroke-width",
    "dash",
    "color",
    "text-color",
    "text-size",
    "font",
];

/// The fonts every drawing has.
const FONTS: [&str; 5] = ["mono", "mono-bold", "sans", "sans-bold", "sans-italic"];

/// The value a widget starts from where the default applies.
const GREY: [u8; 4] = [128, 128, 128, 255];

/// A value as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Color([u8; 4]),
    /// `none` for a color.
    NoColor,
    Number(f64),
    Dash([f64; 2]),
    Font(String),
    /// Values no widget edits, such as `( ... )`, as written.
    Other(String),
    /// Nothing sets it, and the kind's default applies.
    Default,
    /// The selected nodes' values differ.
    Mixed,
}

/// One property in the panel, as the selected nodes have it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: &'static str,
    pub shown: Shown,
    /// Where the value comes from, or `None` when that differs between
    /// the selected nodes.
    pub origin: Option<Origin>,
    /// Whether every selected node takes `none` for it.
    pub takes_none: bool,
}

/// The panel that changes the selected nodes' colors, strokes and
/// fonts.
#[derive(Debug, Default)]
pub struct Properties {
    /// The scene version and selection that `entries` were read for.
    read_for: Option<(u64, Vec<Span>)>,
    entries: Vec<Entry>,
    /// Why the entries could not be read.
    unread: Option<String>,
    /// The fonts the drawing registers with `fonts`.
    fonts: Vec<String>,
    /// The changes not yet written, each a key and its new value as it
    /// goes in the source, for the nodes at `paths`.
    pending: Vec<(&'static str, String)>,
    paths: Vec<Vec<usize>>,
}

impl Properties {
    /// Read the entries again when the scene `version` or the selected
    /// `nodes` changed since the last read.
    pub fn read(&mut self, version: u64, source: &str, nodes: &[Span], fonts: Vec<String>) {
        let key = (version, nodes.to_vec());
        if self.read_for.as_ref() == Some(&key) {
            return;
        }
        (self.entries, self.unread) = match entries(source, nodes) {
            Ok(entries) => (entries, None),
            Err(why) => (Vec::new(), Some(why)),
        };
        self.fonts = fonts;
        self.read_for = Some(key);
    }

    /// Forget the entries, so the next [`Properties::read`] reads them.
    pub fn clear(&mut self) {
        self.read_for = None;
    }

    /// The first change not yet written for the nodes at `paths`, taken
    /// here. Changes made for other nodes are dropped.
    pub fn take(&mut self, paths: &[Vec<usize>]) -> Option<(&'static str, String)> {
        if self.paths != paths {
            self.pending.clear();
            return None;
        }
        (!self.pending.is_empty()).then(|| self.pending.remove(0))
    }

    /// Draw the panel's rows for the nodes at `paths`.
    pub fn ui(&mut self, ui: &mut egui::Ui, paths: &[Vec<usize>]) {
        if let Some(why) = &self.unread {
            ui.label(why.as_str());
            return;
        }
        if self.entries.is_empty() {
            ui.label("nothing here to change");
            return;
        }
        let mut changed = Vec::new();
        egui::Grid::new("properties")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for entry in &mut self.entries {
                    ui.label(entry.key);
                    if let Some(text) = widget(ui, entry, &self.fonts) {
                        changed.push((entry.key, text));
                    }
                    ui.horizontal(|ui| {
                        ui.weak(match &entry.origin {
                            Some(Origin::Node) => "node".to_string(),
                            Some(Origin::Style(name)) => format!("style {name}"),
                            Some(Origin::Default) => "default".to_string(),
                            None => "mixed".to_string(),
                        });
                        if entry.origin == Some(Origin::Node)
                            && ui
                                .small_button("×")
                                .on_hover_text("take the node's own value away")
                                .clicked()
                        {
                            changed.push((entry.key, String::new()));
                        }
                    });
                    ui.end_row();
                }
            });
        if changed.is_empty() {
            return;
        }
        if self.paths != paths {
            self.pending.clear();
            self.paths = paths.to_vec();
        }
        for (key, text) in changed {
            match self.pending.iter_mut().find(|(k, _)| *k == key) {
                Some(p) => p.1 = text,
                None => self.pending.push((key, text)),
            }
        }

        // the change is written on a later frame, once no button is held
        ui.ctx().request_repaint();
    }
}

/// Draw the widget that edits `entry`, and give the new value as it goes
/// in the source when it changed.
fn widget(ui: &mut egui::Ui, entry: &mut Entry, fonts: &[String]) -> Option<String> {
    let key = entry.key;
    ui.horizontal(|ui| match key {
        "fill" | "stroke" | "color" | "text-color" => {
            let mut none = entry.shown == Shown::NoColor;
            if entry.takes_none && ui.toggle_value(&mut none, "none").changed() {
                entry.shown = if none {
                    Shown::NoColor
                } else {
                    Shown::Color(GREY)
                };
                return Some(value_text(&entry.shown));
            }
            if none {
                return None;
            }
            let mut rgba = match entry.shown {
                Shown::Color(c) => c,
                _ => GREY,
            };
            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                entry.shown = Shown::Color(rgba);
                return Some(value_text(&entry.shown));
            }
            None
        }
        "stroke-width" | "text-size" => {
            let mut n = match entry.shown {
                Shown::Number(n) => n,
                _ if key == "text-size" => 16.0,
                _ => 2.0,
            };
            let drag = egui::DragValue::new(&mut n)
                .speed(0.1)
                .range(0.0..=1000.0)
                .max_decimals(2);
            if ui.add(drag).changed() {
                entry.shown = Shown::Number(n);
                return Some(value_text(&entry.shown));
            }
            None
        }
        "dash" => {
            let mut dash = match entry.shown {
                Shown::Dash(d) => d,
                _ => [6.0, 4.0],
            };
            if matches!(entry.shown, Shown::Default | Shown::Mixed) {
                if ui.button("add").clicked() {
                    entry.shown = Shown::Dash(dash);
                    return Some(value_text(&entry.shown));
                }
                return None;
            }
            let mut changed = false;
            for n in &mut dash {
                let drag = egui::DragValue::new(n)
                    .speed(0.1)
                    .range(0.0..=1000.0)
                    .max_decimals(2);
                changed |= ui.add(drag).changed();
            }
            if changed {
                entry.shown = Shown::Dash(dash);
                return Some(value_text(&entry.shown));
            }
            None
        }
        "font" => {
            let current = match &entry.shown {
                Shown::Font(f) => f.clone(),
                _ => String::new(),
            };
            let mut chosen = None;
            egui::ComboBox::from_id_salt("font")
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    for f in FONTS
                        .iter()
                        .copied()
                        .chain(fonts.iter().map(String::as_str))
                    {
                        if ui.selectable_label(f == current, f).clicked() {
                            chosen = Some(f.to_string());
                        }
                    }
                });
            let chosen = chosen.filter(|f| *f != current)?;
            entry.shown = Shown::Font(chosen);
            Some(value_text(&entry.shown))
        }
        _ => None,
    })
    .inner
}

/// The entries for the nodes at `nodes` in `source`: the properties of
/// [`KEYS`] that every node's kind takes, each with the value the nodes
/// share or [`Shown::Mixed`], or the reason for the status line that
/// they cannot be read.
pub fn entries(source: &str, nodes: &[Span]) -> Result<Vec<Entry>, String> {
    let mut entries: Option<Vec<Entry>> = None;
    for &node in nodes {
        let settings = edit::property_values(source, node, &KEYS).map_err(|e| {
            let (line, col) = e.line_col(source);
            format!("cannot read the properties: {line}:{col}: {}", e.message)
        })?;
        let kind = source[node.start..]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .next()
            .unwrap_or("");
        let takes_none = |key: &str| {
            schema::node(kind)
                .and_then(|k| k.property(key))
                .is_some_and(|p| p.takes.split(" | ").any(|t| t == "none"))
        };
        let read: Vec<Entry> = settings
            .iter()
            .map(|s| Entry {
                key: KEYS
                    .into_iter()
                    .find(|k| *k == s.key)
                    .expect("a key of KEYS"),
                shown: shown(s.key, &s.values),
                origin: Some(s.origin.clone()),
                takes_none: takes_none(s.key),
            })
            .collect();
        entries = Some(match entries {
            None => read,
            Some(before) => before
                .into_iter()
                .filter_map(|mut e| {
                    let r = read.iter().find(|r| r.key == e.key)?;
                    if r.shown != e.shown {
                        e.shown = Shown::Mixed;
                    }
                    if r.origin != e.origin {
                        e.origin = None;
                    }
                    e.takes_none &= r.takes_none;
                    Some(e)
                })
                .collect(),
        });
    }
    Ok(entries.unwrap_or_default())
}

/// `values` of `key` as the panel shows them.
fn shown(key: &str, values: &[ValueKind]) -> Shown {
    match (key, values) {
        (_, []) => Shown::Default,
        (_, [ValueKind::Color(c)]) => Shown::Color(*c),
        (_, [ValueKind::Word(w)]) if w == "none" => Shown::NoColor,
        ("font", [ValueKind::Word(w)]) => Shown::Font(w.clone()),
        ("dash", [ValueKind::Number(on), ValueKind::Number(off)]) => Shown::Dash([*on, *off]),
        (_, [ValueKind::Number(n)]) => Shown::Number(*n),
        _ => Shown::Other(
            values
                .iter()
                .map(|v| match v {
                    ValueKind::Word(w) => w.clone(),
                    ValueKind::Number(n) => number_text(*n),
                    _ => "…".to_string(),
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

/// `shown` as it goes in the source.
pub fn value_text(shown: &Shown) -> String {
    match shown {
        Shown::Color([r, g, b, 255]) => format!("#{r:02x}{g:02x}{b:02x}"),
        Shown::Color([r, g, b, a]) => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
        Shown::NoColor => "none".to_string(),
        Shown::Number(n) => number_text(*n),
        Shown::Dash([on, off]) => format!("{} {}", number_text(*on), number_text(*off)),
        Shown::Font(f) | Shown::Other(f) => f.clone(),
        Shown::Default | Shown::Mixed => String::new(),
    }
}

/// `n` rounded to two decimals, written with no trailing zeros.
fn number_text(n: f64) -> String {
    let text = format!("{:.2}", (n * 100.0).round() / 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" => "0".to_string(),
        t => t.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The span of the node of `src` that starts with `node`.
    fn span_of(src: &str, node: &str) -> Span {
        let program = acre::lang::parse(src).unwrap();
        let start = src.find(node).unwrap();
        let mut stack: Vec<&acre::lang::Node> = program.nodes.iter().collect();
        while let Some(n) = stack.pop() {
            if n.span.start == start {
                return n.span;
            }
            stack.extend(&n.children);
        }
        panic!("no node starts at `{node}`");
    }

    fn entry<'a>(entries: &'a [Entry], key: &str) -> &'a Entry {
        entries.iter().find(|e| e.key == key).unwrap()
    }

    #[test]
    fn a_value_is_written_as_the_source_takes_it() {
        assert_eq!(value_text(&Shown::Color([255, 16, 0, 255])), "#ff1000");
        assert_eq!(value_text(&Shown::Color([255, 16, 0, 128])), "#ff100080");
        assert_eq!(value_text(&Shown::NoColor), "none");
        assert_eq!(value_text(&Shown::Number(2.0)), "2");
        assert_eq!(value_text(&Shown::Number(1.5)), "1.5");
        assert_eq!(value_text(&Shown::Number(0.3333)), "0.33");
        assert_eq!(value_text(&Shown::Number(-0.001)), "0");
        assert_eq!(value_text(&Shown::Dash([6.0, 2.5])), "6 2.5");
        assert_eq!(value_text(&Shown::Font("sans-bold".into())), "sans-bold");
    }

    #[test]
    fn a_value_is_shown_by_its_kind_and_key() {
        let c = [1, 2, 3, 255];
        assert_eq!(shown("fill", &[ValueKind::Color(c)]), Shown::Color(c));
        assert_eq!(
            shown("fill", &[ValueKind::Word("none".into())]),
            Shown::NoColor
        );
        assert_eq!(
            shown("font", &[ValueKind::Word("sans".into())]),
            Shown::Font("sans".into())
        );
        assert_eq!(
            shown("dash", &[ValueKind::Number(4.0), ValueKind::Number(2.0)]),
            Shown::Dash([4.0, 2.0])
        );
        assert_eq!(
            shown("stroke-width", &[ValueKind::Number(3.0)]),
            Shown::Number(3.0)
        );
        assert_eq!(shown("stroke-width", &[]), Shown::Default);
        assert_eq!(
            shown("stroke-width", &[ValueKind::Group(vec![])]),
            Shown::Other("…".into())
        );
    }

    #[test]
    fn one_node_lists_what_its_kind_takes_and_where_each_comes_from() {
        let src = "canvas { size 100 100; style s { stroke #000000; } box a { style s; fill none; size 5 5; } }";
        let entries = entries(src, &[span_of(src, "box a")]).unwrap();
        let keys: Vec<&str> = entries.iter().map(|e| e.key).collect();
        assert_eq!(
            keys,
            [
                "fill",
                "stroke",
                "stroke-width",
                "dash",
                "text-color",
                "text-size",
                "font"
            ]
        );
        let fill = entry(&entries, "fill");
        assert_eq!(
            (&fill.shown, &fill.origin),
            (&Shown::NoColor, &Some(Origin::Node))
        );
        assert!(fill.takes_none);
        let stroke = entry(&entries, "stroke");
        assert_eq!(stroke.origin, Some(Origin::Style("s".into())));
        let width = entry(&entries, "stroke-width");
        assert_eq!(
            (&width.shown, &width.origin),
            (&Shown::Default, &Some(Origin::Default))
        );
        assert!(!entry(&entries, "text-color").takes_none);
    }

    #[test]
    fn several_nodes_list_what_every_kind_takes_and_mark_what_differs() {
        let src = "canvas { size 100 100; box a { stroke #ff0000; stroke-width 2; size 5 5; } line l { from 0 0; to 5 5; stroke #00ff00; stroke-width 2; } }";
        let entries = entries(src, &[span_of(src, "box a"), span_of(src, "line l")]).unwrap();
        let keys: Vec<&str> = entries.iter().map(|e| e.key).collect();
        assert!(!keys.contains(&"fill"), "{keys:?}");
        let stroke = entry(&entries, "stroke");
        assert_eq!(
            (&stroke.shown, &stroke.origin),
            (&Shown::Mixed, &Some(Origin::Node))
        );
        let width = entry(&entries, "stroke-width");
        assert_eq!(width.shown, Shown::Number(2.0));
    }

    #[test]
    fn a_change_for_other_nodes_is_dropped() {
        let mut properties = Properties {
            pending: vec![("fill", "#000000".into()), ("stroke", "none".into())],
            paths: vec![vec![0, 1]],
            ..Properties::default()
        };
        assert_eq!(
            properties.take(&[vec![0, 1]]),
            Some(("fill", "#000000".into()))
        );
        assert_eq!(properties.take(&[vec![0, 2]]), None);
        assert_eq!(properties.take(&[vec![0, 1]]), None);
    }
}
