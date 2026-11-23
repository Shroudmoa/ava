// mag: pull command configs ("mags") from a GitHub repo.
// a mag is just an ava sheet; the repo keeps an index (mags.toml) beside them.
// downloads go through curl - no extra deps, same plumbing as `publicip`.
//
//   mag list          show the repo's mag catalog
//   mag get <name>    download + validate + install into the sheets dir
//   mag update        re-fetch every installed mag
//   mag rm <name>     remove an installed mag
//
// mags execute shell commands with your permissions - only use repos you trust.

use crate::color;
use crate::cmds::{Outcome, CYAN, DIM, GREEN, RED, RESET};
use crate::config::Config;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

/// one catalog entry from the repo's mags.toml index
#[derive(Debug, Clone, Deserialize)]
pub struct MagDef {
    pub name: String,
    pub file: String,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Default, Deserialize)]
struct MagIndex {
    #[serde(default)]
    mag: Vec<MagDef>,
}

/// entry point for the `mag` builtin; arg is everything after the command name
pub fn run(arg: &str, config: &Config) -> Outcome {
    let mut parts = arg.split_whitespace();
    let sub = parts.next().unwrap_or("list");
    let names: Vec<&str> = parts.collect();
    match sub {
        "list" | "ls" => list(config),
        "get" | "install" => get(config, &names),
        "update" | "up" => update(config),
        "rm" | "remove" => rm(config, &names),
        "help" => {
            usage(config);
            Outcome::Done(0)
        }
        _ => {
            eprintln!(
                "{}mag: unknown subcommand '{}'{}",
                color::seq(RED),
                sub,
                color::seq(RESET)
            );
            usage(config);
            Outcome::Done(1)
        }
    }
}

fn usage(config: &Config) {
    println!();
    println!("  {}mag{} :: command configs from GitHub", color::seq(CYAN), color::seq(RESET));
    println!("  {}mag list{}        show available mags", color::seq(GREEN), color::seq(RESET));
    println!("  {}mag get <name>{}  install a mag into your sheets dir", color::seq(GREEN), color::seq(RESET));
    println!("  {}mag update{}      re-fetch installed mags", color::seq(GREEN), color::seq(RESET));
    println!("  {}mag rm <name>{}   remove an installed mag", color::seq(GREEN), color::seq(RESET));
    println!(
        "  {}repo: {} ([mags] repo/branch/path in config.toml){}",
        color::seq(DIM),
        config.mags.repo,
        color::seq(RESET)
    );
    println!(
        "  {}mags run shell commands - only install from repos you trust{}",
        color::seq(DIM),
        color::seq(RESET)
    );
    println!();
}

// ---------------- catalog ----------------

fn index(config: &Config) -> Result<Vec<MagDef>, String> {
    let text = fetch(&raw_url(config, "mags.toml"))?;
    let idx: MagIndex =
        toml::from_str(&text).map_err(|e| format!("bad mag index: {}", e))?;
    let mut mags = Vec::new();
    for m in idx.mag {
        if valid_name(&m.name) && valid_file(&m.file) {
            mags.push(m);
        } else {
            eprintln!(
                "{}mag: skipping invalid index entry '{}' ({}){}",
                color::seq(RED),
                m.name,
                m.file,
                color::seq(RESET)
            );
        }
    }
    Ok(mags)
}

fn list(config: &Config) -> Outcome {
    let mags = match index(config) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let dir = install_dir(config);
    println!();
    println!(
        "  {}mags :: {}{}{}",
        color::seq(CYAN),
        color::seq(DIM),
        config.mags.repo,
        color::seq(RESET)
    );
    if mags.is_empty() {
        println!("  {}(no mags in the index){}", color::seq(DIM), color::seq(RESET));
    }
    for m in &mags {
        let mark = if dir.join(&m.file).is_file() {
            format!("{}+{}", color::seq(GREEN), color::seq(RESET))
        } else {
            " ".to_string()
        };
        println!(
            " {} {}{:<16}{} {}",
            mark,
            color::seq(CYAN),
            m.name,
            color::seq(RESET),
            m.desc
        );
    }
    println!();
    println!(
        "  {}+ installed · ava mag get <name> to install{}",
        color::seq(DIM),
        color::seq(RESET)
    );
    println!();
    Outcome::Done(0)
}

// ---------------- get / update / rm ----------------

fn get(config: &Config, names: &[&str]) -> Outcome {
    if names.is_empty() {
        eprintln!(
            "{}mag: get needs a name - try `mag list`{}",
            color::seq(RED),
            color::seq(RESET)
        );
        return Outcome::Done(1);
    }
    for n in names {
        if !valid_name(n) {
            return fail(format!("invalid mag name '{}'", n));
        }
    }
    let mags = match index(config) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let mut code = 0;
    for n in names {
        match mags.iter().find(|m| m.name == *n) {
            Some(m) => match install(config, m) {
                Ok(dest) => println!(
                    "{}installed{} {} {}→ {}{}",
                    color::seq(GREEN),
                    color::seq(RESET),
                    m.name,
                    color::seq(DIM),
                    dest.display(),
                    color::seq(RESET)
                ),
                Err(e) => {
                    code = 1;
                    eprintln!("{}mag: {}: {}{}", color::seq(RED), n, e, color::seq(RESET));
                }
            },
            None => {
                code = 1;
                eprintln!(
                    "{}mag: '{}' not in the index - try `mag list`{}",
                    color::seq(RED),
                    n,
                    color::seq(RESET)
                );
            }
        }
    }
    if code == 0 {
        println!(
            "{}new commands load on the next ava start{}",
            color::seq(DIM),
            color::seq(RESET)
        );
    }
    Outcome::Done(code)
}

fn update(config: &Config) -> Outcome {
    let mags = match index(config) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let dir = install_dir(config);
    let installed: Vec<&MagDef> = mags.iter().filter(|m| dir.join(&m.file).is_file()).collect();
    if installed.is_empty() {
        println!(
            "{}no mags installed yet - `mag get <name>` first{}",
            color::seq(DIM),
            color::seq(RESET)
        );
        return Outcome::Done(0);
    }
    let mut code = 0;
    for m in installed {
        match install(config, m) {
            Ok(_) => println!(
                "{}updated{} {}",
                color::seq(GREEN),
                color::seq(RESET),
                m.name
            ),
            Err(e) => {
                code = 1;
                eprintln!("{}mag: {}: {}{}", color::seq(RED), m.name, e, color::seq(RESET));
            }
        }
    }
    Outcome::Done(code)
}

fn rm(config: &Config, names: &[&str]) -> Outcome {
    if names.is_empty() {
        eprintln!(
            "{}mag: rm needs a name - try `mag list`{}",
            color::seq(RED),
            color::seq(RESET)
        );
        return Outcome::Done(1);
    }
    let mags = match index(config) {
        Ok(m) => m,
        Err(e) => return fail(e),
    };
    let dir = install_dir(config);
    let mut code = 0;
    for n in names {
        match mags.iter().find(|m| m.name == *n) {
            Some(m) => {
                let path = dir.join(&m.file);
                if path.is_file() {
                    match std::fs::remove_file(&path) {
                        Ok(_) => println!(
                            "{}removed{} {} {}({}){}",
                            color::seq(GREEN),
                            color::seq(RESET),
                            m.name,
                            color::seq(DIM),
                            path.display(),
                            color::seq(RESET)
                        ),
                        Err(e) => {
                            code = 1;
                            eprintln!("{}mag: {}: {}{}", color::seq(RED), n, e, color::seq(RESET));
                        }
                    }
                } else {
                    println!("{}{} is not installed{}", color::seq(DIM), n, color::seq(RESET));
                }
            }
            None => {
                code = 1;
                eprintln!(
                    "{}mag: '{}' not in the index{}",
                    color::seq(RED),
                    n,
                    color::seq(RESET)
                );
            }
        }
    }
    Outcome::Done(code)
}

// ---------------- download + install ----------------

/// download, validate as a sheet, then write atomically (tmp file + rename)
fn install(config: &Config, m: &MagDef) -> Result<PathBuf, String> {
    let text = fetch(&raw_url(config, &m.file))?;
    crate::sheet::parse(&text).map_err(|e| format!("not a valid sheet: {}", e))?;
    let dir = install_dir(config);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    let dest = dir.join(&m.file);
    let tmp = dir.join(format!(".{}.tmp", m.file));
    std::fs::write(&tmp, &text).map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("cannot install {}: {}", dest.display(), e))?;
    Ok(dest)
}

fn fetch(url: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-time",
            "30",
            url,
        ])
        .output()
        .map_err(|_| "mag needs curl on PATH".to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("download failed: {}", url)
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// raw.githubusercontent.com URL for a file in the configured repo
fn raw_url(config: &Config, file: &str) -> String {
    let path = config.mags.path.trim_matches('/');
    if path.is_empty() {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}",
            config.mags.repo, config.mags.branch, file
        )
    } else {
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            config.mags.repo, config.mags.branch, path, file
        )
    }
}

/// where mags land: the first configured sheets dir (default ~/.config/ava/sheets)
fn install_dir(config: &Config) -> PathBuf {
    config
        .sheets
        .directories
        .first()
        .cloned()
        .or_else(|| Config::config_dir().map(|d| d.join("sheets")))
        .unwrap_or_else(|| PathBuf::from("sheets"))
}

fn fail(msg: String) -> Outcome {
    eprintln!("{}mag: {}{}", color::seq(RED), msg, color::seq(RESET));
    Outcome::Done(1)
}

// ---------------- validation (untrusted index input) ----------------

/// mag names: plain identifiers, nothing that could escape a path or a shell
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// index file names: a bare *.toml file name, no directories, no traversal
pub fn valid_file(s: &str) -> bool {
    s.ends_with(".toml")
        && s.len() > 5
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains("..")
}

/// owner/repo, both parts plain identifiers
pub fn valid_repo(s: &str) -> bool {
    let mut it = s.split('/');
    match (it.next(), it.next(), it.next()) {
        (Some(o), Some(r), None) => safe_segment(o) && safe_segment(r),
        _ => false,
    }
}

/// branch / path segments: identifiers plus '/' for nested paths, never '..'
pub fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && !s.contains("..")
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn names_are_plain_identifiers() {
        assert!(valid_name("linux"));
        assert!(valid_name("health-check"));
        assert!(valid_name("my_mag2"));
        assert!(!valid_name(""));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a.toml"));
    }

    #[test]
    fn files_are_bare_toml_names() {
        assert!(valid_file("linux.toml"));
        assert!(!valid_file("linux.txt"));
        assert!(!valid_file(".toml"));
        assert!(!valid_file("../evil.toml"));
        assert!(!valid_file("sub/evil.toml"));
        assert!(!valid_file("sub\\evil.toml"));
        assert!(!valid_file("..toml"));
    }

    #[test]
    fn repo_must_be_owner_slash_repo() {
        assert!(valid_repo("Shroudmoa/ava"));
        assert!(valid_repo("some-org/some_repo.js"));
        assert!(!valid_repo("Shroudmoa"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("/ava"));
        assert!(!valid_repo("a/"));
        assert!(!valid_repo("a/../b"));
    }

    #[test]
    fn segments_reject_traversal() {
        assert!(safe_segment("main"));
        assert!(safe_segment("feature/x"));
        assert!(safe_segment("examples"));
        assert!(!safe_segment(""));
        assert!(!safe_segment("../etc"));
        assert!(!safe_segment("a b"));
    }

    #[test]
    fn url_points_at_raw_github() {
        let c = cfg();
        assert_eq!(
            raw_url(&c, "linux.toml"),
            "https://raw.githubusercontent.com/Shroudmoa/ava/main/examples/linux.toml"
        );
    }

    #[test]
    fn url_supports_repo_root() {
        let mut c = cfg();
        c.mags.path = "".into();
        assert_eq!(
            raw_url(&c, "mags.toml"),
            "https://raw.githubusercontent.com/Shroudmoa/ava/main/mags.toml"
        );
    }

    #[test]
    fn index_parses_and_defaults_desc() {
        let idx: MagIndex = toml::from_str(
            "[[mag]]\nname = \"linux\"\nfile = \"linux.toml\"\n\n[[mag]]\nname = \"net\"\nfile = \"network.toml\"\ndesc = \"diagnostics\"\n",
        )
        .unwrap();
        assert_eq!(idx.mag.len(), 2);
        assert_eq!(idx.mag[0].desc, "");
        assert_eq!(idx.mag[1].desc, "diagnostics");
        assert!(toml::from_str::<MagIndex>("this is not toml [[[").is_err());
    }
}
