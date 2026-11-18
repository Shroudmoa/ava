// configuration: platform-standard config directory
// linux:   ~/.config/ava/
// windows: %APPDATA%\ava\
// the file is optional - defaults kick in when it is missing or invalid

use serde::Deserialize;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "ava";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub sheets: Sheets,
    pub interface: Interface,
    pub history: History,
    pub shell: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Sheets {
    /// directories scanned for *.toml command sheets, in load order
    pub directories: Vec<PathBuf>,
    /// explicit sheet files, in load order
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub color: bool,
    pub dashboard: bool,
}

impl Default for Interface {
    fn default() -> Self {
        Interface {
            color: true,
            dashboard: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct History {
    pub enabled: bool,
}

impl Default for History {
    fn default() -> Self {
        History { enabled: true }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    sheets: RawSheets,
    #[serde(default)]
    interface: RawInterface,
    #[serde(default)]
    history: RawHistory,
    #[serde(default)]
    shell: RawShell,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawShell {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSheets {
    #[serde(default)]
    directories: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawInterface {
    #[serde(default = "bool_true")]
    color: bool,
    #[serde(default = "bool_true")]
    dashboard: bool,
}

// derived Default would silently turn features off when config.toml is absent
impl Default for RawInterface {
    fn default() -> Self {
        RawInterface {
            color: true,
            dashboard: true,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawHistory {
    #[serde(default = "bool_true")]
    enabled: bool,
}

impl Default for RawHistory {
    fn default() -> Self {
        RawHistory { enabled: true }
    }
}

fn bool_true() -> bool {
    true
}

impl Config {
    /// load the user configuration, falling back to defaults when absent/invalid
    pub fn load() -> Config {
        let path = Config::path();
        let raw = path
            .as_ref()
            .and_then(|p| match std::fs::read_to_string(p) {
                Ok(text) => match toml::from_str::<RawConfig>(&text) {
                    Ok(raw) => Some(raw),
                    Err(e) => {
                        eprintln!(
                            "ava: config parse error at {}: {} - using defaults",
                            p.display(),
                            e
                        );
                        None
                    }
                },
                Err(e) if p.exists() => {
                    eprintln!(
                        "ava: cannot read config at {}: {} - using defaults",
                        p.display(),
                        e
                    );
                    None
                }
                Err(_) => None, // no config file at all is a normal, supported case
            })
            .unwrap_or_default();

        let mut config = Config::from_raw(raw, path.as_deref().unwrap_or(Path::new(".")));
        if config.sheets.directories.is_empty() {
            if let Some(dir) = Config::config_dir() {
                config.sheets.directories.push(dir.join("sheets"));
            }
        }
        config
    }

    /// pure builder so the merge of defaults + file content stays testable
    fn from_raw(raw: RawConfig, path: &Path) -> Config {
        let base = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        let mut config = Config::default();
        config.sheets.directories = raw
            .sheets
            .directories
            .iter()
            .map(|d| resolve(&base, d))
            .collect();
        config.sheets.files = raw
            .sheets
            .files
            .iter()
            .map(|f| resolve(&base, f))
            .collect();
        config.interface.color = raw.interface.color;
        config.interface.dashboard = raw.interface.dashboard;
        config.history.enabled = raw.history.enabled;
        config.shell = raw.shell.name;
        config
    }

    /// ~/.config/ava/config.toml on linux, %APPDATA%\ava\config.toml on windows
    pub fn path() -> Option<PathBuf> {
        let dir = dirs::config_dir()?.join(APP_DIR);
        Some(dir.join(CONFIG_FILE))
    }

    pub fn config_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(APP_DIR))
    }

    pub fn history_path() -> Option<PathBuf> {
        dirs::config_dir()
            .map(|d| d.join(APP_DIR))
            .map(|d| d.join("history"))
    }

    /// the shell shell-commands run through inside the dash terminals
    /// (sh/bash/fish, from `[shell] name` in the config) - None = default `sh`
    pub fn shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }
}

fn resolve(base: &Path, p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let c = Config::default();
        assert!(c.interface.color);
        assert!(c.interface.dashboard);
        assert!(c.history.enabled);
        assert!(c.sheets.directories.is_empty());
    }

    #[test]
    fn raw_config_builds_expected_values() {
        let raw = RawConfig {
            sheets: RawSheets {
                directories: vec!["sheets".into()],
                files: vec![],
            },
            interface: RawInterface {
                color: false,
                dashboard: false,
            },
            history: RawHistory { enabled: false },
            shell: RawShell {
                name: Some("fish".into()),
            },
        };
        let c = Config::from_raw(raw, Path::new("/tmp/ava/config.toml"));
        assert!(!c.interface.color);
        assert!(!c.interface.dashboard);
        assert!(!c.history.enabled);
        assert_eq!(c.shell.as_deref(), Some("fish"));
        // relative sheet dirs resolve against the config dir
        assert_eq!(c.sheets.directories[0], PathBuf::from("/tmp/ava/sheets"));
    }

    #[test]
    fn raw_defaults_keep_features_on() {
        let raw = RawConfig::default();
        assert!(raw.interface.color);
        assert!(raw.interface.dashboard);
        assert!(raw.history.enabled);
    }

    #[test]
    fn from_raw_stays_pure() {
        let c = Config::from_raw(RawConfig::default(), Path::new("/tmp/ava/config.toml"));
        assert!(c.sheets.directories.is_empty());
        assert!(c.sheets.files.is_empty());
    }
}
