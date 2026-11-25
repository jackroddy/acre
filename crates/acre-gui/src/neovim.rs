//! neovim opened on a drawing in a terminal, linked to the viewer.

use std::path::Path;
use std::process::Command;
use std::thread;

/// The terminal that neovim opens in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// decade's own ghostty, split beside the viewer's pane.
    Decade,
    /// Ghostty on macOS, opened as a new instance of the app.
    MacGhostty,
    /// Whatever `xdg-terminal-exec` starts.
    Xdg,
}

impl Terminal {
    /// The terminal for the viewer's surroundings.
    pub fn here() -> Self {
        if std::env::var_os("DECADE_SOCKET").is_some() {
            Terminal::Decade
        } else if cfg!(target_os = "macos") {
            Terminal::MacGhostty
        } else {
            Terminal::Xdg
        }
    }
}

/// The command that opens neovim on `file` in `terminal`, with neovim
/// told to link the plugin to `socket`.
pub fn command(file: &Path, socket: &Path, terminal: Terminal) -> Command {
    let mut command = match terminal {
        Terminal::Decade => {
            let mut ghostty = Command::new("ghostty");
            ghostty.args([
                "--window-decoration=none",
                "--gtk-titlebar=false",
                "--gtk-single-instance=false",
                "-e",
            ]);

            // decade splits a new window beside the pane `DECADE_PANE`
            // names, or into the pane `DECADE_INTO` names. The viewer's
            // own name the pane it was started from, so both go, and
            // decade splits the focused pane, the viewer's, instead
            ghostty.env_remove("DECADE_PANE").env_remove("DECADE_INTO");
            ghostty
        }
        Terminal::MacGhostty => {
            let mut open = Command::new("open");
            open.args(["-na", "Ghostty", "--args", "-e"]);
            open
        }
        Terminal::Xdg => Command::new("xdg-terminal-exec"),
    };

    // the socket goes on neovim's command line, since a terminal that
    // `open` starts on macOS does not inherit the viewer's environment
    command.arg("nvim").arg(file).arg(format!(
        "+lua require(\"acre\").connect({})",
        lua_string(socket)
    ));
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        command.current_dir(dir);
    }
    command
}

/// `path` as a Lua string literal.
fn lua_string(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Open neovim on `file` linked to `socket`, or give the reason for the
/// status line that it cannot start.
pub fn open(file: &Path, socket: &Path) -> Result<(), String> {
    let mut command = command(file, socket, Terminal::here());
    let mut child = command.spawn().map_err(|e| {
        let program = command.get_program().to_string_lossy();
        format!("cannot start {program}: {e}")
    })?;

    // waited on so the terminal leaves no zombie when it exits
    thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn args(command: &Command) -> Vec<&OsStr> {
        command.get_args().collect()
    }

    fn env<'a>(command: &'a Command, name: &str) -> Option<Option<&'a OsStr>> {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    const CONNECT: &str = "+lua require(\"acre\").connect(\"/run/acre/7.sock\")";

    #[test]
    fn outside_decade_the_terminal_comes_from_xdg_terminal_exec() {
        let command = command(
            Path::new("/a/b c.acre"),
            Path::new("/run/acre/7.sock"),
            Terminal::Xdg,
        );
        assert_eq!(command.get_program(), "xdg-terminal-exec");
        assert_eq!(args(&command), ["nvim", "/a/b c.acre", CONNECT]);
        assert_eq!(env(&command, "DECADE_PANE"), None);
        assert_eq!(command.get_current_dir(), Some(Path::new("/a")));
    }

    #[test]
    fn inside_decade_ghostty_opens_beside_the_focused_pane() {
        let command = command(
            Path::new("/a/b.acre"),
            Path::new("/run/acre/7.sock"),
            Terminal::Decade,
        );
        assert_eq!(command.get_program(), "ghostty");
        assert_eq!(
            args(&command),
            [
                "--window-decoration=none",
                "--gtk-titlebar=false",
                "--gtk-single-instance=false",
                "-e",
                "nvim",
                "/a/b.acre",
                CONNECT
            ]
        );
        assert_eq!(env(&command, "DECADE_PANE"), Some(None));
        assert_eq!(env(&command, "DECADE_INTO"), Some(None));
    }

    #[test]
    fn on_macos_open_starts_a_new_ghostty() {
        let command = command(
            Path::new("/a/b.acre"),
            Path::new("/run/acre/7.sock"),
            Terminal::MacGhostty,
        );
        assert_eq!(command.get_program(), "open");
        assert_eq!(
            args(&command),
            [
                "-na",
                "Ghostty",
                "--args",
                "-e",
                "nvim",
                "/a/b.acre",
                CONNECT
            ]
        );
    }

    #[test]
    fn a_quote_in_the_socket_path_stays_inside_the_lua_string() {
        assert_eq!(lua_string(Path::new("/x\"y\\z")), "\"/x\\\"y\\\\z\"");
    }

    #[test]
    fn a_bare_file_name_keeps_the_working_directory() {
        let command = command(Path::new("b.acre"), Path::new("/s"), Terminal::Xdg);
        assert_eq!(command.get_current_dir(), None);
    }
}
