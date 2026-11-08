use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::Deserialize;

/// Settings read from `$XDG_CONFIG_HOME/acre/config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub editor_font_size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            editor_font_size: 14.0,
        }
    }
}

/// The config, and why it fell back to the defaults if the file exists
/// but could not be used.
pub fn load() -> (Config, Option<String>) {
    let Some(path) = xdg_dir("XDG_CONFIG_HOME", ".config").map(|d| d.join("acre/config.toml"))
    else {
        return (Config::default(), None);
    };
    match fs::read_to_string(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => (Config::default(), None),
        Err(e) => (
            Config::default(),
            Some(format!("cannot read {}: {e}", path.display())),
        ),
        Ok(text) => match toml::from_str(&text) {
            Ok(config) => (config, None),
            Err(e) => (
                Config::default(),
                Some(format!("{}: {}", path.display(), e.message())),
            ),
        },
    }
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
    fn a_missing_key_takes_its_default() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn keys_are_kebab_case_and_unknown_keys_are_rejected() {
        let config: Config = toml::from_str("editor-font-size = 18").unwrap();
        assert_eq!(config.editor_font_size, 18.0);
        assert!(toml::from_str::<Config>("font = 18").is_err());
    }
}
