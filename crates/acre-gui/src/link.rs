//! The messages between acre-gui and an editor that started it with
//! `--stdio`: one JSON object per line, on stdin into acre-gui and on
//! stdout out of it.
//!
//! ```text
//! in   {"type":"text","text":"canvas { ... }","path":"/a/b.acre","tick":7}
//! in   {"type":"cursor","offset":120}
//! out  {"type":"goto","offset":96}
//! out  {"type":"edit","tick":7,"edits":[{"start":20,"end":22,"text":"40"}]}
//! out  {"type":"error","offset":40,"message":"`box` needs `at`"}
//! out  {"type":"ok"}
//! ```
//!
//! Offsets count bytes from the start of the text. A `tick` names one
//! version of the text: an `edit` holds ranges in the text of its tick,
//! and the editor drops an edit whose tick is no longer its own. acre-gui
//! closes when stdin closes.

use std::io::{self, BufRead, Write};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use acre::scene::edit::TextEdit;
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Incoming {
    /// The whole source, the file it belongs to if it has one, and the
    /// editor's tick for this version of it.
    Text {
        text: String,
        path: Option<String>,
        #[serde(default)]
        tick: u64,
    },
    /// Where the editor's cursor is.
    Cursor { offset: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Outgoing {
    /// Put the editor's cursor here.
    Goto { offset: usize },
    /// The source has this error.
    Error { offset: usize, message: String },
    /// The source has no error.
    Ok,
    /// Make these changes to the source of `tick`.
    Edit { tick: u64, edits: Vec<Replace> },
}

/// The text that replaces bytes `start..end` of the source.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replace {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl From<&TextEdit> for Replace {
    fn from(edit: &TextEdit) -> Self {
        Replace {
            start: edit.span.start,
            end: edit.span.end,
            text: edit.text.clone(),
        }
    }
}

/// The editor's end of the link: what it sent, and a way to answer.
pub struct Link {
    incoming: Receiver<Option<Incoming>>,
}

impl Link {
    /// Read stdin on a thread of its own, asking `ctx` to repaint after
    /// each message. A line that is not a message is skipped.
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, incoming) = channel();
        thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(message) = serde_json::from_str::<Incoming>(&line) {
                    if tx.send(Some(message)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            }
            let _ = tx.send(None);
            ctx.request_repaint();
        });
        Link { incoming }
    }

    /// The messages that arrived since the last call. `None` among them
    /// means stdin closed.
    pub fn received(&self) -> Vec<Option<Incoming>> {
        self.incoming.try_iter().collect()
    }

    pub fn send(&self, message: &Outgoing) {
        let Ok(line) = serde_json::to_string(message) else {
            return;
        };
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_and_write_as_tagged_json() {
        let text: Incoming =
            serde_json::from_str(r#"{"type":"text","text":"canvas {}","path":null}"#).unwrap();
        assert_eq!(
            text,
            Incoming::Text {
                text: "canvas {}".into(),
                path: None,
                tick: 0
            }
        );
        let ticked: Incoming =
            serde_json::from_str(r#"{"type":"text","text":"","path":"/a.acre","tick":12}"#)
                .unwrap();
        assert_eq!(
            ticked,
            Incoming::Text {
                text: "".into(),
                path: Some("/a.acre".into()),
                tick: 12
            }
        );
        let cursor: Incoming = serde_json::from_str(r#"{"type":"cursor","offset":3}"#).unwrap();
        assert_eq!(cursor, Incoming::Cursor { offset: 3 });
        assert_eq!(
            serde_json::to_string(&Outgoing::Goto { offset: 9 }).unwrap(),
            r#"{"type":"goto","offset":9}"#
        );
        assert_eq!(
            serde_json::to_string(&Outgoing::Ok).unwrap(),
            r#"{"type":"ok"}"#
        );
        let edit = Outgoing::Edit {
            tick: 12,
            edits: vec![Replace {
                start: 3,
                end: 5,
                text: "at 1 \"2\"".into(),
            }],
        };
        assert_eq!(
            serde_json::to_string(&edit).unwrap(),
            r#"{"type":"edit","tick":12,"edits":[{"start":3,"end":5,"text":"at 1 \"2\""}]}"#
        );
    }
}
