//! acre-gui: a viewer for acre drawings. It renders a file and follows
//! its changes, or renders what an editor sends it over stdin.

mod app;
mod link;
mod recent;
mod view;

use std::path::PathBuf;

use clap::Parser;
use eframe::egui;

#[derive(Parser)]
#[command(name = "acre-gui", about = "a viewer for acre drawings")]
struct Cli {
    /// Render the source an editor sends as JSON lines on stdin, and answer on stdout
    #[arg(long, conflicts_with = "file")]
    stdio: bool,

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
            .with_inner_size([900.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "acre-gui",
        options,
        Box::new(move |cc| {
            // egui's own Ctrl+= and Ctrl+- zoom of the whole window
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            let link = cli.stdio.then(|| link::Link::start(cc.egui_ctx.clone()));
            Ok(Box::new(app::App::new(cli.file, link)))
        }),
    )?;
    Ok(())
}
