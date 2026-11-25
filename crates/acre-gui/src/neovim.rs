//! neovim opened on a drawing in a terminal, linked to the viewer.

use std::path::Path;
use std::process::Command;
use std::thread;

/// The command that opens neovim on `file` in a terminal, with
/// `ACRE_SOCKET` set to `socket` for the plugin to connect to. In decade
/// the terminal is decade's own, ghostty, and elsewhere the one
/// `xdg-terminal-exec` starts.
pub fn command(file: &Path, socket: &Path, in_decade: bool) -> Command {
    let mut command = if in_decade {
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
    } else {
        Command::new("xdg-terminal-exec")
    };
    command.arg("nvim").arg(file).env("ACRE_SOCKET", socket);
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        command.current_dir(dir);
    }
    command
}

/// Open neovim on `file` linked to `socket`, or give the reason for the
/// status line that it cannot start.
pub fn open(file: &Path, socket: &Path) -> Result<(), String> {
    let in_decade = std::env::var_os("DECADE_SOCKET").is_some();
    let mut command = command(file, socket, in_decade);
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

    #[test]
    fn outside_decade_the_terminal_comes_from_xdg_terminal_exec() {
        let command = command(
            Path::new("/a/b c.acre"),
            Path::new("/run/acre/7.sock"),
            false,
        );
        assert_eq!(command.get_program(), "xdg-terminal-exec");
        assert_eq!(args(&command), ["nvim", "/a/b c.acre"]);
        assert_eq!(
            env(&command, "ACRE_SOCKET"),
            Some(Some(OsStr::new("/run/acre/7.sock")))
        );
        assert_eq!(env(&command, "DECADE_PANE"), None);
        assert_eq!(command.get_current_dir(), Some(Path::new("/a")));
    }

    #[test]
    fn inside_decade_ghostty_opens_beside_the_focused_pane() {
        let command = command(Path::new("/a/b.acre"), Path::new("/run/acre/7.sock"), true);
        assert_eq!(command.get_program(), "ghostty");
        assert_eq!(
            args(&command),
            [
                "--window-decoration=none",
                "--gtk-titlebar=false",
                "--gtk-single-instance=false",
                "-e",
                "nvim",
                "/a/b.acre"
            ]
        );
        assert_eq!(
            env(&command, "ACRE_SOCKET"),
            Some(Some(OsStr::new("/run/acre/7.sock")))
        );
        assert_eq!(env(&command, "DECADE_PANE"), Some(None));
        assert_eq!(env(&command, "DECADE_INTO"), Some(None));
    }

    #[test]
    fn a_bare_file_name_keeps_the_working_directory() {
        let command = command(Path::new("b.acre"), Path::new("/s"), false);
        assert_eq!(command.get_current_dir(), None);
    }
}
