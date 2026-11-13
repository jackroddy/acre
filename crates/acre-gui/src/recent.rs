use std::fs;
use std::path::{Path, PathBuf};

const KEPT: usize = 10;

/// Recently opened files, newest first, kept in
/// `$XDG_STATE_HOME/acre/recent` with one path per line.
pub struct Recent {
    store: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

impl Recent {
    pub fn load() -> Self {
        let store = xdg_dir("XDG_STATE_HOME", ".local/state").map(|d| d.join("acre/recent"));
        let files = store
            .as_ref()
            .and_then(|s| fs::read_to_string(s).ok())
            .map(|text| text.lines().map(PathBuf::from).collect())
            .unwrap_or_default();
        Recent { store, files }
    }

    /// Put `file` first and write the list out.
    pub fn touch(&mut self, file: &Path) -> Result<(), String> {
        let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        push_front(&mut self.files, file);
        let Some(store) = &self.store else {
            return Ok(());
        };
        let mut text = String::new();
        for f in &self.files {
            text.push_str(&f.to_string_lossy());
            text.push('\n');
        }
        let write = || -> std::io::Result<()> {
            if let Some(dir) = store.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(store, text)
        };
        write().map_err(|e| format!("cannot write {}: {e}", store.display()))
    }
}

fn push_front(files: &mut Vec<PathBuf>, file: PathBuf) {
    files.retain(|f| *f != file);
    files.insert(0, file);
    files.truncate(KEPT);
}

/// `$var` when it holds an absolute path, else `$HOME/fallback`.
pub fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(var).map(PathBuf::from)
        && dir.is_absolute()
    {
        return Some(dir);
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reopened_file_moves_to_the_front_without_a_duplicate() {
        let mut files: Vec<PathBuf> = ["a", "b", "c"].map(PathBuf::from).to_vec();
        push_front(&mut files, PathBuf::from("b"));
        assert_eq!(files, ["b", "a", "c"].map(PathBuf::from));
    }

    #[test]
    fn the_list_keeps_the_newest_ten() {
        let mut files = Vec::new();
        for i in 0..12 {
            push_front(&mut files, PathBuf::from(i.to_string()));
        }
        assert_eq!(files.len(), 10);
        assert_eq!(files[0], PathBuf::from("11"));
        assert_eq!(files[9], PathBuf::from("2"));
    }
}
