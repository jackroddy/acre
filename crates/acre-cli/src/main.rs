//! acre: animation creation and rendering engine.
//!
//! Reads a drawing written in acre's own language and renders it to an
//! image.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::{Args, Parser, Subcommand};

use acre::scene::Scene;
use acre::{lang, render, scene};

#[derive(Parser)]
#[command(name = "acre", about = "animation creation and rendering engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a drawing and report its errors without rendering.
    Check(CheckArgs),
    /// Render a drawing to a PNG image.
    Render(RenderArgs),
    /// Rewrite drawings in acre's one layout.
    Fmt(FmtArgs),
}

#[derive(Args)]
struct FmtArgs {
    /// Report the files that would change, and write nothing
    #[arg(long)]
    check: bool,

    /// The drawings to format, or `-` to format stdin to stdout
    #[arg(value_name = "file", required = true)]
    files: Vec<PathBuf>,
}

#[derive(Args)]
struct CheckArgs {
    /// The drawing to check
    #[arg(value_name = "file")]
    file: PathBuf,
}

#[derive(Args)]
struct RenderArgs {
    /// Where the image goes [default: the file with a .png extension]
    #[arg(long, value_name = "path")]
    out: Option<PathBuf>,

    /// Image pixels to each canvas pixel
    #[arg(long, value_name = "n", default_value_t = 1.0)]
    scale: f64,

    /// The drawing to render
    #[arg(value_name = "file")]
    file: PathBuf,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Check(args) => {
            load(&args.file)?;
        }
        Command::Fmt(args) if args.files == [Path::new("-")] => {
            let mut src = String::new();
            io::stdin()
                .read_to_string(&mut src)
                .context("cannot read stdin")?;
            let formatted = lang::format(&src).map_err(|e| {
                let (line, col) = e.line_col(&src);
                anyhow!("<stdin>:{line}:{col}: {}", e.message)
            })?;
            if args.check {
                if formatted != src {
                    return Err(anyhow!("not formatted: <stdin>"));
                }
            } else {
                io::stdout()
                    .write_all(formatted.as_bytes())
                    .context("cannot write stdout")?;
            }
        }
        Command::Fmt(args) => {
            let mut unformatted = Vec::new();
            for path in &args.files {
                let src = fs::read_to_string(path)
                    .with_context(|| format!("cannot read {}", path.display()))?;
                let formatted = lang::format(&src).map_err(|e| {
                    let (line, col) = e.line_col(&src);
                    anyhow!("{}:{line}:{col}: {}", path.display(), e.message)
                })?;
                if formatted == src {
                    continue;
                }
                if args.check {
                    unformatted.push(path.display().to_string());
                } else {
                    fs::write(path, formatted)
                        .with_context(|| format!("cannot write {}", path.display()))?;
                }
            }
            if !unformatted.is_empty() {
                return Err(anyhow!("not formatted: {}", unformatted.join(", ")));
            }
        }
        Command::Render(args) => {
            let scene = load(&args.file)?;
            let out = args.out.unwrap_or_else(|| args.file.with_extension("png"));
            render::draw_frame(&scene, 0.0, args.scale)
                .save_png(&out)
                .with_context(|| format!("cannot write {}", out.display()))?;
        }
    }
    Ok(())
}

fn load(path: &Path) -> anyhow::Result<Scene> {
    let src =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let located = |e: lang::Error| {
        let (line, col) = e.line_col(&src);
        anyhow!("{}:{line}:{col}: {}", path.display(), e.message)
    };
    let program = lang::parse(&src).map_err(located)?;
    scene::build(&program).map_err(located)
}
