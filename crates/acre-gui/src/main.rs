//! acre-gui: an editor for acre drawings, with the source beside its
//! render.

mod app;
mod config;
mod recent;

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use eframe::egui;

#[derive(Parser)]
#[command(name = "acre-gui", about = "an editor for acre drawings")]
struct Cli {
    /// Write a desktop entry for acre-gui so app launchers list it, then exit
    #[arg(long)]
    install_desktop_entry: bool,

    /// The drawing to open [default: the last one opened]
    #[arg(value_name = "file")]
    file: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if cli.install_desktop_entry {
        let path = install_desktop_entry()?;
        println!("wrote {}", path.display());
        return Ok(());
    }

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

fn install_desktop_entry() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let dir = config::xdg_dir("XDG_DATA_HOME", ".local/share")
        .ok_or("neither XDG_DATA_HOME nor HOME is set")?
        .join("applications");
    fs::create_dir_all(&dir)?;
    let path = dir.join("acre-gui.desktop");
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=acre\n\
         Comment=Edit and render acre drawings\n\
         Exec={} %f\n\
         Terminal=false\n\
         Categories=Graphics;\n",
        exec_quote(&exe.to_string_lossy())
    );
    fs::write(&path, entry)?;
    Ok(path)
}

/// `arg` quoted for a desktop entry's `Exec` key.
fn exec_quote(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        // the Exec quoting rule escapes these four with a
        // backslash, and the file's own string escaping then
        // doubles every backslash
        match c {
            '\\' => out.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quote_escapes_twice_for_the_desktop_entry_format() {
        assert_eq!(
            exec_quote("/home/j/bin/acre-gui"),
            "\"/home/j/bin/acre-gui\""
        );
        assert_eq!(exec_quote("/a b/$x"), "\"/a b/\\\\$x\"");
        assert_eq!(exec_quote("/a\\b"), "\"/a\\\\\\\\b\"");
    }
}
