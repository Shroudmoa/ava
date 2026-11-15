// command sheet: data-driven mova-style cheat sheet
// builtin defaults are embedded; user sheet (~/.ava/sheet.toml) adds or shadows

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct CmdEntry {
    pub name: String,
    pub desc: String,
    #[serde(default = "default_kind")]
    pub kind: String, // "builtin" | "shell"
    #[serde(default)]
    pub run: String,  // builtin key or shell command
    #[serde(default)]
    pub win: Option<String>, // windows override for shell commands
}

fn default_kind() -> String {
    "shell".to_string()
}

#[derive(Debug, Deserialize)]
struct SheetFile {
    #[serde(default)]
    cmd: Vec<CmdEntry>,
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub cmds: Vec<CmdEntry>,
}

impl Sheet {
    pub fn load() -> Sheet {
        let mut sheet = Sheet {
            cmds: parse(include_str!("../assets/default.toml")),
        };
        if let Some(p) = user_sheet_path() {
            if let Ok(text) = std::fs::read_to_string(&p) {
                for entry in parse(&text) {
                    sheet.upsert(entry);
                }
            }
        }
        sheet
    }

    pub fn find(&self, name: &str) -> Option<&CmdEntry> {
        self.cmds.iter().find(|c| c.name == name)
    }

    pub fn upsert(&mut self, entry: CmdEntry) {
        if let Some(existing) = self.cmds.iter_mut().find(|c| c.name == entry.name) {
            *existing = entry; // shadow builtin
        } else {
            self.cmds.push(entry);
        }
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
}

pub fn user_sheet_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ava").join("sheet.toml"))
}

fn parse(text: &str) -> Vec<CmdEntry> {
    match toml::from_str::<SheetFile>(text) {
        Ok(f) => f.cmd,
        Err(e) => {
            eprintln!("ava: sheet parse error: {}", e);
            Vec::new()
        }
    }
}
