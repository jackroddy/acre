//! acre: animation creation and rendering engine.
//!
//! Reads an animation written in acre's own language and renders it to
//! frames.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acre", about = "animation creation and rendering engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse an animation and report its errors without rendering.
    Check(CheckArgs),
    /// Render an animation to frames.
    Render(RenderArgs),
}

#[derive(Args)]
struct CheckArgs {
    /// The animation to check
    #[arg(value_name = "file")]
    file: PathBuf,
}

#[derive(Args)]
struct RenderArgs {
    /// Where the frames go
    #[arg(long, value_name = "dir")]
    out: Option<PathBuf>,

    /// The animation to render
    #[arg(value_name = "file")]
    file: PathBuf,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Check(_) => anyhow::bail!("check is not written yet"),
        Command::Render(_) => anyhow::bail!("render is not written yet"),
    }
}
