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
    /// Render a drawing to a PNG, SVG or PDF file.
    Render(RenderArgs),
    /// Rewrite drawings in acre's one layout.
    Fmt(FmtArgs),
    /// List the node kinds a drawing can hold and the properties each takes.
    Reference(ReferenceArgs),
}

#[derive(Args)]
struct ReferenceArgs {
    /// Only this node kind [default: every kind]
    #[arg(value_name = "kind")]
    kind: Option<String>,
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
    /// Where the image goes, in the format its extension names [default: the
    /// file with the format's extension]
    #[arg(long, value_name = "path")]
    out: Option<PathBuf>,

    /// The format to write [default: from the extension of --out, else png]
    #[arg(long, value_enum, value_name = "format")]
    format: Option<Format>,

    /// Image pixels to each canvas pixel, or for SVG and PDF, CSS pixels
    #[arg(long, value_name = "n", default_value_t = 1.0)]
    scale: f64,

    /// The drawing to render
    #[arg(value_name = "file")]
    file: PathBuf,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Png,
    Svg,
    Pdf,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
        }
    }
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Check(args) => {
            load(&args.file)?;
        }
        Command::Reference(args) => {
            let kinds: Vec<&scene::schema::NodeKind> = match &args.kind {
                None => scene::schema::NODES.iter().collect(),
                Some(name) => vec![
                    scene::schema::node(name)
                        .ok_or_else(|| anyhow!("acre has no node kind `{name}`"))?,
                ],
            };
            for (k, kind) in kinds.iter().enumerate() {
                if k > 0 {
                    println!();
                }
                println!("{}: {}", kind.name, kind.about);
                let width = kind
                    .properties
                    .iter()
                    .map(|p| p.name.len())
                    .max()
                    .unwrap_or(0);
                for p in kind.properties {
                    println!("  {:width$}  {}", p.name, p.takes);
                    println!("  {:width$}  {}", "", p.about);
                }
                if !kind.children.is_empty() {
                    println!("  holds {}", kind.children.join(", "));
                }
            }
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
            let format = match (args.format, &args.out) {
                (Some(format), _) => format,
                (None, None) => Format::Png,
                (None, Some(out)) => {
                    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("");
                    match ext.to_ascii_lowercase().as_str() {
                        "png" => Format::Png,
                        "svg" => Format::Svg,
                        "pdf" => Format::Pdf,
                        _ => {
                            return Err(anyhow!(
                                "cannot tell a format from {}: name it .png, .svg or .pdf, or give --format",
                                out.display()
                            ));
                        }
                    }
                }
            };
            let scene = load(&args.file)?;
            let out = args
                .out
                .unwrap_or_else(|| args.file.with_extension(format.extension()));
            let bytes = match format {
                Format::Png => render::draw_frame(&scene, 0.0, args.scale)
                    .encode_png()
                    .context("cannot encode the image")?,
                Format::Svg => render::draw_svg(&scene, 0.0, args.scale).into_bytes(),
                Format::Pdf => render::draw_pdf(&scene, 0.0, args.scale)
                    .map_err(|e| anyhow!("cannot write a PDF: {e}"))?,
            };
            fs::write(&out, bytes).with_context(|| format!("cannot write {}", out.display()))?;
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
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut fonts = Vec::new();
    for file in scene::font_files(&program).map_err(located)? {
        let font = fs::read(dir.join(&file.path)).map_err(|e| {
            located(lang::Error::new(
                file.span,
                format!("cannot read font {}: {e}", file.path),
            ))
        })?;
        fonts.push(font);
    }
    scene::build(&program, &fonts).map_err(located)
}
