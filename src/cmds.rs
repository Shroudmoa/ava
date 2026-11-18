// builtins + dispatch: one registry, one dispatch fn - grow mova style

use crate::banner;
use crate::color;
use crate::config::Config;
use crate::sheet::Kind;
use crate::sheet::Sheet;
use std::io::Write;
use std::process::Command;

pub enum Outcome {
    /// command ran; carries an exit code for one-shot mode
    Done(i32),
    Exit(i32),
}

pub type Builtin = fn(&str, &Sheet, &Config) -> Outcome;

/// lookup for `kind = "builtin"` entries by their `run` key
pub fn builtin(key: &str) -> Option<Builtin> {
    Some(match key {
        "sysinfo" => sysinfo_cmd,
        "net" => net_cmd,
        "ports" => ports_cmd,
        "dash" => dash_cmd,
        "help" => help_cmd,
        "clear" => clear_cmd,
        "exit" => exit_cmd,
        _ => return None,
    })
}

/// the heart: one line in, side effects out (mova's switch, but data-driven)
pub fn dispatch(line: &str, sheet: &Sheet, config: &Config, announce_fallback: bool) -> Outcome {
    log(&format!("EXECUTE: {}", line));
    let line = line.trim();
    if line.is_empty() {
        return Outcome::Done(0);
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();

    if let Some(entry) = sheet.find(head) {
        if entry.kind == Kind::Builtin {
            match builtin(&entry.run) {
                Some(f) => return f(arg, sheet, config),
                None => {
                    eprintln!(
                        "{}ava: builtin '{}' ({}) not compiled in.{}",
                        color::seq(RED), entry.run, entry.kind.as_str(), color::seq(RESET)
                    );
                    return Outcome::Done(1);
                }
            }
        }
        let cmd = sheet.shell_for(entry);
        return run_shell(&cmd);
    }

    // unknown: pass to the real shell (mova fallback). one-shot mode stays
    // quiet so scripts get clean output; the interactive repl keeps the hint.
    if announce_fallback {
        println!(
            "{}not an ava command. Trying shell...{}",
            color::seq(DIM),
            color::seq(RESET)
        );
    }
    run_shell(line)
}

pub fn run_shell(cmd: &str) -> Outcome {
    let (code, stdout, stderr) = run_shell_with(&shell_default(), cmd);
    print!("{}", stdout);
    if !stderr.is_empty() {
        print!("{}", stderr);
    }
    let _ = std::io::stdout().flush();
    Outcome::Done(code)
}

/// run a shell line with the given shell (`sh`, `bash`, `fish, ...`) and capture
/// stdout+stderr, so callers (the dash's little terminal) can show what ran
pub fn run_shell_with(shell: &str, cmd: &str) -> (i32, String, String) {
    let flag = if cfg!(windows) { "/C" } else { "-c" };
    match Command::new(shell).arg(flag).arg(cmd).output() {
        Ok(out) => (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(e) => {
            eprintln!(
                "{}shell error: {}{}",
                color::seq(RED),
                e,
                color::seq(RESET)
            );
            (1, String::new(), String::new())
        }
    }
}

fn shell_default() -> &'static str {
    if cfg!(windows) { "cmd" } else { "sh" }
}

/// mask secret values before anything is written to the log.
/// handles `KEY=value`, `--flag value`, `Authorization: Bearer value` and
/// `Bearer value`, without mangling unrelated flags such as `-p...`.
pub fn redact(line: &str) -> String {
    const SECRET_KEYS: [&str; 9] = [
        "password", "passwd", "secret", "token", "credential", "apikey", "auth", "private",
        "bearer",
    ];

    fn secret_key(token: &str) -> bool {
        let key = token
            .split_once('=')
            .map(|(k, _)| k)
            .unwrap_or(token)
            .trim_start_matches("export ")
            .trim_end_matches(':');
        // single-dash flags (-psecret) are deliberately left alone
        if key.starts_with('-') && !key.starts_with("--") {
            return false;
        }
        let key = key.strip_prefix("--").unwrap_or(key).to_ascii_lowercase();
        SECRET_KEYS.iter().any(|s| key.contains(s))
    }

    let mut out: Vec<String> = Vec::new();
    let mut it = line.split_whitespace().peekable();
    while let Some(token) = it.next() {
        if secret_key(token) {
            if token.contains('=') {
                out.push("****".into());
                continue;
            }
            out.push(token.to_string());
            // `Authorization: Bearer xyz` -> keep Bearer, mask only xyz
            if it.peek().is_some_and(|n| n.eq_ignore_ascii_case("bearer")) {
                out.push(it.next().unwrap().to_string());
            }
            if it.peek().is_some() {
                it.next();
                out.push("****".into());
            }
            continue;
        }
        out.push(token.to_string());
    }
    out.join(" ")
}

// ---------------- builtins ----------------

fn sysinfo_cmd(_arg: &str, _sheet: &Sheet, _config: &Config) -> Outcome {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();

    let total = sys.total_memory(); // bytes
    let avail = sys.available_memory();
    let used = total.saturating_sub(avail);
    let pct = if total > 0 { used * 100 / total } else { 0 };

    let up = System::uptime(); // seconds
    let load = System::load_average();

    println!();
    println!(
        "{}{}@{}{}",
        color::seq(CYAN),
        banner::user(),
        System::host_name().unwrap_or_else(|| "host".into()),
        color::seq(RESET)
    );
    println!(
        "{}{}{}",
        color::seq(DIM),
        "-".repeat(20),
        color::seq(RESET)
    );
    println!("OS      : {}", System::name().unwrap_or_else(|| "?".into()));
    println!(
        "Kernel  : {}",
        System::kernel_version().unwrap_or_else(|| "?".into())
    );
    println!("Uptime  : {}", format_uptime(up));
    println!(
        "Load    : {:.2} {:.2} {:.2}",
        load.one, load.five, load.fifteen
    );
    println!(
        "{}RAM     : {}/{} GB {} {}%{}",
        color::seq(color_for(pct)),
        to_gb(used),
        to_gb(total),
        bar(pct, 10),
        pct,
        color::seq(RESET)
    );
    println!();
    Outcome::Done(0)
}

fn net_cmd(_arg: &str, _sheet: &Sheet, _config: &Config) -> Outcome {
    println!();
    let ok = if cfg!(windows) {
        run_prog("ipconfig", &["/all"]) & run_prog("route", &["print"])
    } else {
        run_prog("ip", &["addr"]) & run_prog("ip", &["route"])
    };
    if !ok {
        println!(
            "{}network tools failed on this box.{}",
            color::seq(RED),
            color::seq(RESET)
        );
    }
    println!();
    Outcome::Done(0)
}

fn ports_cmd(_arg: &str, _sheet: &Sheet, _config: &Config) -> Outcome {
    println!();
    println!(
        "{}{:<8}{:<42}{}",
        color::seq(DIM),
        "PORT",
        "LOCAL",
        color::seq(RESET)
    );
    for (port, local) in ports_list() {
        println!("{:<8}{:<42}", port, local);
    }
    println!("{}end{}", color::seq(DIM), color::seq(RESET));
    println!();
    Outcome::Done(0)
}

fn dash_cmd(_arg: &str, sheet: &Sheet, config: &Config) -> Outcome {
    crate::dash::run(sheet, config)
}

fn help_cmd(_arg: &str, sheet: &Sheet, _config: &Config) -> Outcome {
    banner::help(sheet);
    Outcome::Done(0)
}

fn clear_cmd(_arg: &str, _sheet: &Sheet, _config: &Config) -> Outcome {
    print!("\x1b[2J\x1b[H");
    let _ = std::io::stdout().flush();
    Outcome::Done(0)
}

fn exit_cmd(_arg: &str, _sheet: &Sheet, _config: &Config) -> Outcome {
    Outcome::Exit(0)
}

// ---------------- shared helpers ----------------

fn run_prog(bin: &str, args: &[&str]) -> bool {
    match Command::new(bin).args(args).status() {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

/// /proc/net/tcp6 stores the address as four little-endian 32-bit words
fn parse_ipv6(hex: &str) -> Option<std::net::Ipv6Addr> {
    use std::net::Ipv6Addr;
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(8).enumerate() {
        let word = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
        bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Some(Ipv6Addr::from(bytes))
}

/// listening sockets - /proc on unix, netstat -ano on windows
pub fn ports_list() -> Vec<(u16, String)> {
    let mut rows: Vec<(u16, String)> = Vec::new();

    #[cfg(unix)]
    {
        for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
            let text = match std::fs::read_to_string(file) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for line in text.lines().skip(1) {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 4 || f[3] != "0A" {
                    continue; // 0A = LISTEN
                }
                let (addr, port) = match f[1].split_once(':') {
                    Some(x) => x,
                    None => continue,
                };
                let port = u16::from_str_radix(port, 16).unwrap_or(0);
                let local = if v6 {
                    parse_ipv6(addr)
                        .map(|a| format!("[{}]", a))
                        .unwrap_or_else(|| "*".to_string())
                } else {
                    let n = u32::from_str_radix(addr, 16).unwrap_or(0);
                    format!(
                        "{}.{}.{}.{}",
                        n & 255,
                        (n >> 8) & 255,
                        (n >> 16) & 255,
                        (n >> 24) & 255
                    )
                };
                rows.push((port, local));
            }
        }
    }

    #[cfg(windows)]
    {
        if let Ok(out) = Command::new("netstat").args(["-ano"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 4 || !f[3].eq_ignore_ascii_case("LISTENING") {
                    continue;
                }
                let (addr, port) = match f[1].rsplit_once(':') {
                    Some(x) => x,
                    None => continue,
                };
                if let Ok(port) = port.parse::<u16>() {
                    rows.push((port, addr.to_string()));
                }
            }
        }
    }

    rows.sort();
    rows.dedup();
    rows
}

// ---------------- tiny shared ui bits ----------------

pub fn to_gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

/// shared uptime formatting for banner, sysinfo and the dashboard header
pub fn format_uptime(seconds: u64) -> String {
    format!(
        "{}d {}h {}m",
        seconds / 86400,
        (seconds % 86400) / 3600,
        (seconds % 3600) / 60
    )
}

pub fn bar(pct: u64, width: usize) -> String {
    let filled = ((pct.min(100) * width as u64) / 100) as usize;
    format!("{}{}", "|".repeat(filled), ".".repeat(width - filled))
}

pub fn color_for(pct: u64) -> &'static str {
    if pct >= 90 {
        RED
    } else if pct >= 75 {
        YELLOW
    } else {
        GREEN
    }
}

// ---------------- colors + log ----------------

pub const RESET: &str = "\x1b[0m";
pub const DIM: &str = "\x1b[90m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const DARKRED: &str = "\x1b[2;31m";

const MAX_LOG_BYTES: u64 = 1024 * 1024;

fn should_rotate(size: u64) -> bool {
    size > MAX_LOG_BYTES
}

pub fn log(entry: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = if cfg!(windows) {
        dirs::data_local_dir().map(|d| d.join("ava").join("ava.log"))
    } else {
        dirs::config_dir().map(|d| d.join("ava")).map(|d| d.join("ava.log"))
    };
    if let Some(p) = path {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(&p)
            .map(|m| should_rotate(m.len()))
            .unwrap_or(false)
        {
            let _ = std::fs::rename(&p, p.with_file_name("ava.log.1"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(
                f,
                "{} | {}@{} | {}",
                ts,
                banner::user(),
                banner::hostname(),
                redact(entry)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_secret_assignments() {
        assert_eq!(redact("export API_TOKEN=abc123"), "export ****");
        assert_eq!(redact("mysql -u root -psecret123"), "mysql -u root -psecret123");
        assert_eq!(redact("FOO=bar"), "FOO=bar");
    }

    #[test]
    fn redact_keeps_normal_lines() {
        assert_eq!(redact("EXECUTE: ls -la"), "EXECUTE: ls -la");
        assert_eq!(redact(""), "");
        assert_eq!(redact("mysql -u root -psecret123"), "mysql -u root -psecret123");
    }

    #[test]
    fn redact_masks_flags_and_bearer_tokens() {
        assert_eq!(redact("--token abc123"), "--token ****");
        assert_eq!(redact("Bearer abc123"), "Bearer ****");
        assert_eq!(
            redact("Authorization: Bearer abc123"),
            "Authorization: Bearer ****"
        );
        assert_eq!(redact("export API_TOKEN=abc123"), "export ****");
        assert_eq!(redact("FOO=bar"), "FOO=bar");
    }

    #[test]
    fn uptime_is_human_readable() {
        assert_eq!(format_uptime(0), "0d 0h 0m");
        assert_eq!(format_uptime(90061), "1d 1h 1m");
        assert_eq!(format_uptime(86399), "0d 23h 59m");
    }

    #[test]
    fn log_rotates_only_when_large() {
        assert!(!should_rotate(0));
        assert!(!should_rotate(MAX_LOG_BYTES));
        assert!(should_rotate(MAX_LOG_BYTES + 1));
    }

    #[test]
    fn ipv6_addresses_are_decoded() {
        // ::1 in the little-endian word order used by /proc/net/tcp6
        assert_eq!(
            parse_ipv6("00000000000000000000000001000000").unwrap().to_string(),
            "::1"
        );
        assert_eq!(
            parse_ipv6("00000000000000000000000000000000").unwrap().to_string(),
            "::"
        );
        assert!(parse_ipv6("deadbeef").is_none());
    }

    #[test]
    fn color_helpers_produce_expected_glyphs() {
        assert_eq!(bar(0, 4), "....");
        assert_eq!(bar(50, 4), "||..");
        assert_eq!(bar(100, 4), "||||");
        assert_eq!(color_for(74), GREEN);
        assert_eq!(color_for(75), YELLOW);
        assert_eq!(color_for(90), RED);
    }
}
