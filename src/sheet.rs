// command sheet: data-driven mova-style cheat sheet
// builtin defaults are embedded; user sheets from the config dir are merged on top

use crate::config::Config;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Builtin,
    Shell,
}

impl Default for Kind {
    fn default() -> Self {
        Kind::Shell
    }
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Builtin => "builtin",
            Kind::Shell => "shell",
        }
    }
}


#[derive(Debug, Clone, Deserialize)]
pub struct CmdEntry {
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub kind: Kind, // "builtin" | "shell"
    #[serde(default)]
    pub run: String, // builtin key or shell command
    #[serde(default)]
    pub win: Option<String>, // windows override for shell commands
}

#[derive(Debug, Deserialize)]
struct SheetFile {
    #[serde(default)]
    cmd: Vec<CmdEntry>,
}

/// one loaded sheet, for help output and diagnostics
#[derive(Debug, Clone)]
pub struct LoadedSheet {
    pub name: String,
    pub path: PathBuf,
    pub entries: usize,
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub cmds: Vec<CmdEntry>,
    loaded: Vec<LoadedSheet>,
}

impl Sheet {
    /// builtin defaults + every user sheet; broken sheets warn and are skipped,
    /// the rest of the configuration still works
    pub fn load(config: &Config) -> Sheet {
        let mut sources: Vec<Vec<CmdEntry>> = Vec::new();
        let mut loaded: Vec<LoadedSheet> = Vec::new();

        let builtin = parse(include_str!("../assets/default.toml")).unwrap_or_default();
        loaded.push(LoadedSheet {
            name: "builtin".into(),
            path: PathBuf::from("builtin"),
            entries: builtin.len(),
        });
        sources.push(builtin);

        for path in user_sheet_paths(&config) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("sheet")
                .to_string();
            match load_sheet(&path) {
                Ok(entries) => {
                    loaded.push(LoadedSheet {
                        name,
                        path,
                        entries: entries.len(),
                    });
                    sources.push(entries);
                }
                Err(e) => {
                    eprintln!("ava: sheet {} not loaded: {}", path.display(), e);
                }
            }
        }

        Sheet {
            cmds: merge(sources),
            loaded,
        }
    }

    pub fn find(&self, name: &str) -> Option<&CmdEntry> {
        self.cmds.iter().find(|c| c.name == name)
    }

    /// shell command for this entry on the current platform
    pub fn shell_for(&self, entry: &CmdEntry) -> String {
        if cfg!(windows) {
            entry.win.clone().unwrap_or_else(|| entry.run.clone())
        } else {
            entry.run.clone()
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.cmds.iter().map(|c| c.name.clone()).collect()
    }

    /// filter commands by name or description substring, case-insensitive;
    /// empty query = the full list
    pub fn filter(&self, query: &str) -> Vec<&CmdEntry> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.cmds.iter().collect();
        }
        self.cmds
            .iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn user_sheets(&self) -> &[LoadedSheet] {
        &self.loaded
    }
}

/// merge sheets in load order; later sheets override earlier ones, same as a user
/// shadowing a builtin
pub fn merge(sheets: Vec<Vec<CmdEntry>>) -> Vec<CmdEntry> {
    let mut cmds: Vec<CmdEntry> = Vec::new();
    for sheet_cmds in sheets {
        for entry in sheet_cmds {
            match cmds.iter_mut().find(|c| c.name == entry.name) {
                Some(existing) => *existing = entry,
                None => cmds.push(entry),
            }
        }
    }
    cmds
}

/// read + parse one sheet file
pub fn load_sheet(path: &Path) -> Result<Vec<CmdEntry>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {}", e))?;
    parse(&text).map_err(|e| format!("invalid TOML: {}", e))
}

pub fn parse(text: &str) -> Result<Vec<CmdEntry>, toml::de::Error> {
    match toml::from_str::<SheetFile>(text) {
        Ok(f) => Ok(f.cmd),
        Err(e) => Err(e),
    }
}

/// user sheets to load, in order:
///   1. legacy ~/.ava/sheet.toml
///   2. configured sheet directories (sorted, *.toml only)
///   3. explicitly configured sheet files
///   4. ./sheets relative to the current directory
pub fn user_sheet_paths(config: &Config) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Some(p) = user_sheet_path() {
        if p.is_file() {
            paths.push(p);
        }
    }
    for dir in &config.sheets.directories {
        if let Some(mut files) = toml_files(dir) {
            files.sort();
            paths.extend(files);
        }
    }
    for f in &config.sheets.files {
        if f.is_file() {
            paths.push(f.clone());
        }
    }
    if let Some(dir) = std::env::current_dir()
        .ok()
        .map(|d| d.join("sheets"))
        .filter(|d| d.is_dir())
    {
        if let Some(mut files) = toml_files(&dir) {
            files.sort();
            paths.extend(files);
        }
    }
    paths
}

fn toml_files(dir: &Path) -> Option<Vec<PathBuf>> {
    let rd = std::fs::read_dir(dir).ok()?;
    let files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|e| e == "toml").unwrap_or(false))
        .collect();
    if files.is_empty() {
        return None;
    }
    Some(files)
}

/// legacy single user sheet, kept for backwards compatibility
pub fn user_sheet_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ava").join("sheet.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: Kind, run: &str) -> CmdEntry {
        CmdEntry {
            name: name.into(),
            desc: "test".into(),
            kind,
            run: run.into(),
            win: None,
        }
    }

    #[test]
    fn merge_builtin_then_override() {
        let builtin = vec![entry("0", Kind::Builtin, "sysinfo")];
        let user = vec![entry("0", Kind::Shell, "df -h /")];
        let cmds = merge(vec![builtin, user]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "0");
        assert_eq!(cmds[0].kind, Kind::Shell);
        assert_eq!(cmds[0].run, "df -h /");
    }

    #[test]
    fn merge_later_sheets_override_earlier() {
        let a = vec![entry("ports", Kind::Shell, "a")];
        let b = vec![entry("ports", Kind::Shell, "b")];
        let cmds = merge(vec![a, b]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].run, "b");
    }

    #[test]
    fn merge_keeps_unrelated_entries() {
        let builtin = vec![entry("0", Kind::Builtin, "sysinfo")];
        let user = vec![entry("my-server", Kind::Shell, "ssh admin@1.2.3.4")];
        let cmds = merge(vec![builtin, user]);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[1].name, "my-server");
    }

    #[test]
    fn parse_rejects_invalid_toml() {
        assert!(parse("this is not toml [[[").is_err());
        assert!(parse("").is_ok());
        assert!(parse("[[cmd]]\nname = \"x\"\ndesc = \"y\"\n").is_ok());
    }

    #[test]
    fn kind_defaults_to_shell() {
        let f: SheetFile = toml::from_str("[[cmd]]\nname = \"x\"\ndesc = \"y\"\nrun = \"echo hi\"\n").unwrap();
        let e = f.cmd.into_iter().next().unwrap();
        assert_eq!(e.kind, Kind::Shell);
        assert_eq!(e.run, "echo hi");
    }

    fn sheet_with(cmds: Vec<CmdEntry>) -> Sheet {
        Sheet {
            cmds,
            loaded: vec![],
        }
    }

    #[test]
    fn filter_empty_query_returns_all() {
        let s = sheet_with(vec![
            entry("ports", Kind::Shell, "ss"),
            entry("publicip", Kind::Shell, "curl"),
        ]);
        assert_eq!(s.filter("").len(), 2);
        assert_eq!(s.filter("   ").len(), 2);
    }

    #[test]
    fn filter_matches_name_case_insensitive() {
        let s = sheet_with(vec![
            entry("publicip", Kind::Shell, "curl"),
            entry("ports", Kind::Shell, "ss"),
        ]);
        assert_eq!(s.filter("PUBLICIP").len(), 1);
        assert_eq!(s.filter("pub").len(), 1);
    }

    #[test]
    fn filter_matches_description_substring() {
        let s = sheet_with(vec![CmdEntry {
            name: "sysinfo".into(),
            desc: "Simple system info (uptime, load, RAM)".into(),
            kind: Kind::Builtin,
            run: "sysinfo".into(),
            win: None,
        }]);
        assert_eq!(s.filter("uptime").len(), 1);
        assert_eq!(s.filter("network").len(), 0);
    }

    #[test]
    fn filter_no_match_is_empty() {
        let s = sheet_with(vec![entry("ports", Kind::Shell, "ss")]);
        assert!(s.filter("zzzz").is_empty());
    }
}
