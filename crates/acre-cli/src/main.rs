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

mod mp4;

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
    /// Render a drawing to a PNG, SVG, PDF or MP4 file.
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

    /// The instant to draw, in seconds from the start of the animation
    #[arg(long, value_name = "seconds", default_value_t = 0.0)]
    time: f64,

    /// (mp4) frames per second [default: the canvas's fps]
    #[arg(long, value_name = "n", value_parser = clap::value_parser!(u32).range(1..))]
    fps: Option<u32>,

    /// (mp4) threads that draw frames
    #[arg(long, value_name = "n", default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    jobs: u32,

    /// The drawing to render
    #[arg(value_name = "file")]
    file: PathBuf,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Png,
    Svg,
    Pdf,
    Mp4,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Mp4 => "mp4",
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
            quiet_on_broken_pipe(write_reference(&mut io::stdout().lock(), &kinds))
                .context("cannot write stdout")?;
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
                quiet_on_broken_pipe(io::stdout().lock().write_all(formatted.as_bytes()))
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
                        "mp4" => Format::Mp4,
                        _ => {
                            return Err(anyhow!(
                                "cannot tell a format from {}: name it .png, .svg, .pdf or .mp4, or give --format",
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
            if args.fps.is_some() && !matches!(format, Format::Mp4) {
                return Err(anyhow!("--fps applies only to MP4"));
            }
            let bytes = match format {
                Format::Mp4 => {
                    return mp4::export(&scene, &out, args.scale, args.fps, args.jobs as usize);
                }
                Format::Png => render::draw_frame(&scene, args.time, args.scale)
                    .encode_png()
                    .context("cannot encode the image")?,
                Format::Svg => render::draw_svg(&scene, args.time, args.scale).into_bytes(),
                Format::Pdf => render::draw_pdf(&scene, args.time, args.scale)
                    .map_err(|e| anyhow!("cannot write a PDF: {e}"))?,
            };
            fs::write(&out, bytes).with_context(|| format!("cannot write {}", out.display()))?;
        }
    }
    Ok(())
}

fn write_reference(out: &mut impl Write, kinds: &[&scene::schema::NodeKind]) -> io::Result<()> {
    for (k, kind) in kinds.iter().enumerate() {
        if k > 0 {
            writeln!(out)?;
        }
        writeln!(out, "{}: {}", kind.name, kind.about)?;
        let width = kind
            .properties
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(0);
        for p in kind.properties {
            writeln!(out, "  {:width$}  {}", p.name, p.takes)?;
            writeln!(out, "  {:width$}  {}", "", p.about)?;
        }
        if !kind.children.is_empty() {
            writeln!(out, "  holds {}", kind.children.join(", "))?;
        }
    }
    Ok(())
}

/// Treat a closed output pipe as the reader being done, not as a failure.
fn quiet_on_broken_pipe(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
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
    let mut data = Vec::new();
    for file in scene::data_files(&program).map_err(located)? {
        let bytes = fs::read(dir.join(&file.path)).map_err(|e| {
            located(lang::Error::new(
                file.span,
                format!("cannot read data file {}: {e}", file.path),
            ))
        })?;
        data.push(bytes);
    }
    let scene = scene::build(&program, &fonts, &data).map_err(located)?;
    for c in scene.missing_glyphs() {
        eprintln!(
            "{}: warning: no font has a glyph for '{c}' (U+{:04X})",
            path.display(),
            c as u32
        );
    }
    Ok(scene)
}
