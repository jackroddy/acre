//! acre-gui: a viewer for acre drawings. It renders a file and follows
//! its changes, or renders what an editor sends it over stdin or over a
//! socket.

mod anchor;
mod app;
mod arrange;
mod document;
mod drag;
mod field;
mod handle;
mod link;
mod marks;
mod neovim;
mod nudge;
mod playback;
mod preview;
mod properties;
mod recent;
mod reorder;
mod screen;
mod selection;
mod snap;
mod tabs;
mod time_bar;
mod tool;
mod view;

use std::env;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use clap::Parser;

use crate::link::{Link, Wake};

#[derive(Parser)]
#[command(name = "acre-gui", about = "a viewer for acre drawings")]
struct Cli {
    /// Render the source an editor sends as JSON lines on stdin, and answer on stdout
    #[arg(long, conflicts_with = "file")]
    stdio: bool,

    /// Start with every edit to the source refused until the toolbar's
    /// Lock is turned off
    #[arg(long)]
    locked: bool,

    /// The drawing to show [default: the last one opened]
    #[arg(value_name = "file")]
    file: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let window = una::App::new("acre-gui")?;
    let waker = window.waker();
    let wake: Wake = Arc::new(move || waker.wake());
    let link = if cli.stdio {
        Some(Link::stdio(wake))
    } else {
        let path = link::socket_path(
            env::var_os("XDG_RUNTIME_DIR").as_deref(),
            env::var_os("USER").as_deref(),
            process::id(),
        );
        Link::listen(wake, path)
            .inspect_err(|e| eprintln!("acre-gui: no socket for an editor: {e}"))
            .ok()
    };
    let recent = recent::Recent::load();
    let mut app = app::App::new(cli.file, recent, link, cli.locked);
    window.run(|ui| app.frame(ui));
    Ok(())
}
