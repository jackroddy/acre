//! acre-gui: an editor for acre drawings, with the source beside its
//! render.

mod app;
mod config;
mod recent;

use std::path::PathBuf;

use clap::Parser;
use eframe::egui;

#[derive(Parser)]
#[command(name = "acre-gui", about = "an editor for acre drawings")]
struct Cli {
    /// The drawing to open [default: the last one opened]
    #[arg(value_name = "file")]
    file: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let (config, config_error) = config::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("acre")
            .with_app_id("acre-gui")
            .with_inner_size([1200.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "acre-gui",
        options,
        Box::new(move |_cc| Ok(Box::new(app::App::new(config, config_error, cli.file)))),
    )?;
    Ok(())
}
