// end-to-end checks for the real binary: config, dispatch, quoting, sheets

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Home {
    root: PathBuf,
}

impl Home {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ava-cli-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ava")).unwrap();
        Home { root }
    }

    fn write_config(&self, text: &str) {
        fs::write(self.root.join("ava").join("config.toml"), text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ava"))
            .args(args)
            .env("XDG_CONFIG_HOME", &self.root)
            .env("HOME", &self.root)
            .env("USER", "tester")
            .output()
            .unwrap()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn version_flag_works() {
    let home = Home::new();
    let out = home.run(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("ava 0."));
}

#[test]
fn missing_config_keeps_defaults() {
    let home = Home::new();
    let out = home.run(&["--help"]);
    assert!(out.status.success());
    assert!(out.stdout.contains(&0x1b)); // color on by default
}

#[test]
fn color_false_strips_all_ansi() {
    let home = Home::new();
    home.write_config("[interface]\ncolor = false\n");
    let out = home.run(&["--help"]);
    assert!(!out.stdout.contains(&0x1b));
    assert!(String::from_utf8_lossy(&out.stdout).contains("command overview"));
}

#[test]
fn dashboard_can_be_disabled_in_config() {
    let home = Home::new();
    home.write_config("[interface]\ndashboard = false\n");
    let out = home.run(&["d"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("dash is disabled"));
}

#[test]
fn dashboard_fails_cleanly_without_a_tty() {
    let home = Home::new();
    let out = home.run(&["d"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("interactive terminal"));
}

#[test]
fn one_shot_preserves_argument_spacing() {
    let home = Home::new();
    let out = home.run(&["echo", "a  b"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "a  b");
}

#[test]
fn user_sheet_shadows_builtin() {
    let home = Home::new();
    let sheet = home.root.join("local.toml");
    fs::write(
        &sheet,
        "[[cmd]]\nname = \"999\"\ndesc = \"override\"\nkind = \"shell\"\nrun = \"echo overridden\"\n",
    )
    .unwrap();
    home.write_config(&format!(
        "[sheets]\nfiles = [\"{}\"]\n",
        sheet.display()
    ));

    let out = home.run(&["999"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "overridden");
}

#[test]
fn invalid_config_falls_back_to_defaults() {
    let home = Home::new();
    home.write_config("this = [not valid\n");
    let out = home.run(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("config parse error"));
}
