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

    /// run with a prepended PATH dir (for the fake-curl mag tests)
    #[cfg(unix)]
    fn run_with_bin(&self, args: &[&str], bin: &std::path::Path) -> Output {
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_ava"))
            .args(args)
            .env("XDG_CONFIG_HOME", &self.root)
            .env("HOME", &self.root)
            .env("USER", "tester")
            .env("PATH", path)
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

#[test]
fn mag_usage_errors_are_clean() {
    let home = Home::new();
    let out = home.run(&["mag", "bogus"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown subcommand"));

    let out = home.run(&["mag", "get"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs a name"));

    // invalid names are rejected before any network access
    let out = home.run(&["mag", "get", "../evil"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid mag name"));

    let out = home.run(&["mag", "help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("mag get <name>"));
}

#[cfg(unix)]
mod mag {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// a local "GitHub repo" plus a fake curl on PATH that serves files from it,
    /// so the full fetch -> validate -> install flow is tested without network
    struct FakeRepo {
        home: Home,
        repo: PathBuf,
        bin: PathBuf,
    }

    impl FakeRepo {
        fn new() -> Self {
            let home = Home::new();
            let repo = home.root.join("repo");
            let bin = home.root.join("bin");
            fs::create_dir_all(&repo).unwrap();
            fs::create_dir_all(&bin).unwrap();
            fs::write(
                repo.join("mags.toml"),
                "[[mag]]\nname = \"demo\"\nfile = \"demo.toml\"\ndesc = \"demo mag\"\n",
            )
            .unwrap();
            fs::write(
                repo.join("demo.toml"),
                "[[cmd]]\nname = \"demo-cmd\"\ndesc = \"demo\"\nkind = \"shell\"\nrun = \"echo from-mag\"\n",
            )
            .unwrap();
            // fake curl: ignore flags, take the last arg (the URL), serve its file name
            let curl = bin.join("curl");
            fs::write(
                &curl,
                format!(
                    "#!/bin/sh\nfor last; do :; done\ncat \"{}/${{last##*/}}\"\n",
                    repo.display()
                ),
            )
            .unwrap();
            fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
            FakeRepo { home, repo, bin }
        }

        fn run(&self, args: &[&str]) -> Output {
            self.home.run_with_bin(args, &self.bin)
        }

        fn installed(&self) -> PathBuf {
            self.home.root.join("ava").join("sheets").join("demo.toml")
        }
    }

    #[test]
    fn get_installs_a_mag_and_it_loads() {
        let r = FakeRepo::new();
        let out = r.run(&["mag", "get", "demo"]);
        assert!(out.status.success(), "{:?}", out);
        assert!(String::from_utf8_lossy(&out.stdout).contains("installed"));
        assert!(r.installed().is_file());

        // the new command is live on the next ava start
        let out = r.run(&["demo-cmd"]);
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "from-mag");
    }

    #[test]
    fn list_marks_installed_mags() {
        let r = FakeRepo::new();
        let out = r.run(&["mag", "list"]);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("demo"));
        assert!(!text.contains("\x1b[32m+"), "no installed marker expected yet");

        assert!(r.run(&["mag", "get", "demo"]).status.success());
        let out = r.run(&["mag", "list"]);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("\x1b[32m+"), "installed marker missing: {}", text);
    }

    #[test]
    fn update_refetches_installed_mags() {
        let r = FakeRepo::new();
        assert!(r.run(&["mag", "get", "demo"]).status.success());
        fs::write(
            r.repo.join("demo.toml"),
            "[[cmd]]\nname = \"demo-cmd\"\ndesc = \"demo v2\"\nkind = \"shell\"\nrun = \"echo updated-mag\"\n",
        )
        .unwrap();
        let out = r.run(&["mag", "update"]);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("updated"));
        let out = r.run(&["demo-cmd"]);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "updated-mag");
    }

    #[test]
    fn get_rejects_a_broken_sheet() {
        let r = FakeRepo::new();
        fs::write(r.repo.join("demo.toml"), "this is not valid toml [[[").unwrap();
        let out = r.run(&["mag", "get", "demo"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("not a valid sheet"));
        assert!(!r.installed().exists(), "broken sheet must not be installed");
    }

    #[test]
    fn rm_removes_an_installed_mag() {
        let r = FakeRepo::new();
        assert!(r.run(&["mag", "get", "demo"]).status.success());
        let out = r.run(&["mag", "rm", "demo"]);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("removed"));
        assert!(!r.installed().exists());
    }
}
