//! The messages between acre-gui and an editor: one JSON object per
//! line. An editor that starts acre-gui with `--stdio` writes to its
//! stdin and reads its stdout. Without `--stdio`, acre-gui listens on a
//! Unix socket, and an editor that connects talks over the socket.
//!
//! ```text
//! in   {"type":"text","text":"canvas { ... }","path":"/a/b.acre","tick":7}
//! in   {"type":"cursor","offset":120}
//! out  {"type":"goto","offset":96}
//! out  {"type":"edit","tick":7,"edits":[{"start":20,"end":22,"text":"40"}]}
//! out  {"type":"error","offset":40,"message":"`box` needs `at`"}
//! out  {"type":"ok"}
//! out  {"type":"open","path":"/a/c.acre"}
//! ```
//!
//! Offsets count bytes from the start of the text. A `tick` names one
//! version of the text: an `edit` holds ranges in the text of its tick,
//! and the editor drops an edit whose tick is no longer its own.
//! acre-gui closes when stdin closes. When a socket's editor goes,
//! acre-gui watches its files again and waits for the next editor.
//!
//! acre-gui keeps each path the editor sends in a tab of its own. An
//! `open` asks the editor to show the file of the tab the person
//! switched to and send its text.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
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
    /// Show this file, and send its text in place of the text sent so
    /// far.
    Open { path: String },
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

/// The way to an editor's end of the link.
pub enum Editor {
    Stdout,
    Socket(UnixStream),
}

impl Editor {
    pub fn send(&self, message: &Outgoing) {
        let Ok(line) = serde_json::to_string(message) else {
            return;
        };
        let _ = match self {
            Editor::Stdout => write_line(&mut io::stdout().lock(), &line),
            Editor::Socket(stream) => write_line(&mut &*stream, &line),
        };
    }
}

fn write_line(out: &mut impl Write, line: &str) -> io::Result<()> {
    writeln!(out, "{line}")?;
    out.flush()
}

/// What the reading thread passes on.
enum Event {
    Opened(Editor),
    Message(Incoming),
    Closed,
}

/// What came over the link since the last look.
pub struct Received {
    /// The messages of the editor connected now.
    pub messages: Vec<Incoming>,
    /// Whether an editor went.
    pub closed: bool,
}

/// The link to an editor: over stdin and stdout, or over a socket that
/// takes one editor at a time.
pub struct Link {
    events: Receiver<Event>,
    editor: Option<Editor>,
    /// The socket's path, removed when the link is dropped.
    socket: Option<PathBuf>,
}

impl Link {
    /// Read stdin on a thread of its own, asking `ctx` to repaint after
    /// each message.
    pub fn stdio(ctx: egui::Context) -> Self {
        let (tx, events) = channel();
        thread::spawn(move || {
            read(io::stdin().lock(), &tx, &ctx);
            let _ = tx.send(Event::Closed);
            ctx.request_repaint();
        });
        Link {
            events,
            editor: Some(Editor::Stdout),
            socket: None,
        }
    }

    /// Listen on a socket at `path`, taking one editor at a time on a
    /// thread of its own, and asking `ctx` to repaint after each message.
    /// A socket left at `path` is replaced.
    pub fn listen(ctx: egui::Context, path: PathBuf) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        let (tx, events) = channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Ok(writer) = stream.try_clone() else {
                    continue;
                };
                if tx.send(Event::Opened(Editor::Socket(writer))).is_err() {
                    return;
                }
                let open = read(BufReader::new(stream), &tx, &ctx);
                if !open || tx.send(Event::Closed).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        Ok(Link {
            events,
            editor: None,
            socket: Some(path),
        })
    }

    /// Take what arrived since the last call. A message that came
    /// before an editor went is dropped with it.
    pub fn received(&mut self) -> Received {
        let mut received = Received {
            messages: Vec::new(),
            closed: false,
        };
        for event in self.events.try_iter() {
            match event {
                Event::Opened(editor) => self.editor = Some(editor),
                Event::Message(message) => received.messages.push(message),
                Event::Closed => {
                    self.editor = None;
                    received.messages.clear();
                    received.closed = true;
                }
            }
        }
        received
    }

    /// The editor connected now.
    pub fn editor(&self) -> Option<&Editor> {
        self.editor.as_ref()
    }

    /// Whether the link is over stdin and stdout, and so ends with
    /// acre-gui.
    pub fn is_stdio(&self) -> bool {
        self.socket.is_none()
    }

    /// The path of the socket an editor connects to.
    pub fn socket(&self) -> Option<&Path> {
        self.socket.as_deref()
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        if let Some(path) = &self.socket {
            let _ = fs::remove_file(path);
        }
    }
}

/// Pass on each message read from `lines` until it ends. A line that is
/// not a message is skipped. False once the receiver has gone.
fn read(lines: impl BufRead, tx: &Sender<Event>, ctx: &egui::Context) -> bool {
    for line in lines.lines() {
        let Ok(line) = line else { break };
        if let Ok(message) = serde_json::from_str::<Incoming>(&line) {
            if tx.send(Event::Message(message)).is_err() {
                return false;
            }
            ctx.request_repaint();
        }
    }
    true
}

/// The socket path for the acre-gui of process `pid`: in an `acre`
/// folder of `runtime`, `$XDG_RUNTIME_DIR`, or without one, in a folder
/// of the temporary folder named for `user`.
pub fn socket_path(runtime: Option<&OsStr>, user: Option<&OsStr>, pid: u32) -> PathBuf {
    let dir = match runtime.filter(|r| !r.is_empty()) {
        Some(runtime) => Path::new(runtime).join("acre"),
        None => {
            let user = user.map_or("user".into(), OsStr::to_string_lossy);
            std::env::temp_dir().join(format!("acre-{user}"))
        }
    };
    dir.join(format!("{pid}.sock"))
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
        assert_eq!(
            serde_json::to_string(&Outgoing::Open {
                path: "/a/c.acre".into()
            })
            .unwrap(),
            r#"{"type":"open","path":"/a/c.acre"}"#
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
