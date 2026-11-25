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
mod neovim;
mod nudge;
mod playback;
mod preview;
mod properties;
mod recent;
mod reorder;
mod selection;
mod snap;
mod tabs;
mod tool;
mod view;

use std::env;
use std::path::PathBuf;
use std::process;

use clap::Parser;
use eframe::egui;

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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("acre")
            .with_app_id("acre-gui")
            .with_inner_size([900.0, 720.0])
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "acre-gui",
        options,
        Box::new(move |cc| {
            // egui's own Ctrl+= and Ctrl+- zoom of the whole window
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            let ctx = cc.egui_ctx.clone();
            let link = if cli.stdio {
                Some(link::Link::stdio(ctx))
            } else {
                let path = link::socket_path(
                    env::var_os("XDG_RUNTIME_DIR").as_deref(),
                    env::var_os("USER").as_deref(),
                    process::id(),
                );
                link::Link::listen(ctx, path)
                    .inspect_err(|e| eprintln!("acre-gui: no socket for an editor: {e}"))
                    .ok()
            };
            let recent = recent::Recent::load();
            Ok(Box::new(app::App::new(cli.file, recent, link, cli.locked)))
        }),
    )?;
    Ok(())
}
