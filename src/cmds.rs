// builtins + dispatch: one registry, one dispatch fn - grow mova style

use crate::banner;
use crate::sheet::Sheet;
use std::io::Write;
use std::process::Command;

pub enum Outcome {
    /// command ran; carries an exit code for one-shot mode
    Done(i32),
    Exit(i32),
}

pub type Builtin = fn(&str, &Sheet) -> Outcome;

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
pub fn dispatch(line: &str, sheet: &Sheet) -> Outcome {
    log(&format!("EXECUTE: {}", line));
    let line = line.trim();
    if line.is_empty() {
        return Outcome::Done(0);
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();

    if let Some(entry) = sheet.find(head) {
        if entry.kind == "builtin" {
            match builtin(&entry.run) {
                Some(f) => return f(arg, sheet),
                None => {
                    eprintln!("{}ava: builtin '{}' not compiled in.{}", RED, entry.run, RESET);
                    return Outcome::Done(1);
                }
            }
        }
        let cmd = sheet.shell_for(entry);
        return run_shell(&cmd);
    }

    // unknown: pass to the real shell (mova fallback)
    println!("{}not an ava command. Trying shell...{}", DIM, RESET);
    run_shell(line)
}

pub fn run_shell(cmd: &str) -> Outcome {
    let (prog, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    match Command::new(prog).arg(flag).arg(cmd).status() {
        Ok(status) => Outcome::Done(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}shell error: {}{}", RED, e, RESET);
            Outcome::Done(1)
        }
    }
}

// ---------------- builtins ----------------

fn sysinfo_cmd(_arg: &str, _sheet: &Sheet) -> Outcome {
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
        CYAN,
        banner::user(),
        System::host_name().unwrap_or_else(|| "host".into()),
        RESET
    );
    println!(
        "{}",
        DIM.to_string() + &"-".repeat(20) + RESET
    );
    println!("OS      : {}", System::name().unwrap_or_else(|| "?".into()));
    println!(
        "Kernel  : {}",
        System::kernel_version().unwrap_or_else(|| "?".into())
    );
    println!("Uptime  : {}d {}h {}m", up / 86400, (up % 86400) / 3600, (up % 3600) / 60);
    println!(
        "Load    : {:.2} {:.2} {:.2}",
        load.one, load.five, load.fifteen
    );
    println!(
        "{}RAM     : {}/{} GB {} {}%{}",
        color_for(pct),
        to_gb(used),
        to_gb(total),
        bar(pct, 10),
        pct,
        RESET
    );
    println!();
    Outcome::Done(0)
}

fn net_cmd(_arg: &str, _sheet: &Sheet) -> Outcome {
    println!();
    let ok = if cfg!(windows) {
        run_prog("ipconfig", &["/all"]) & run_prog("route", &["print"])
    } else {
        run_prog("ip", &["addr"]) & run_prog("ip", &["route"])
    };
    if !ok {
        println!("{}network tools failed on this box.{}", RED, RESET);
    }
    println!();
    Outcome::Done(0)
}

fn ports_cmd(_arg: &str, _sheet: &Sheet) -> Outcome {
    println!();
    println!("{}{:<8}{:<22}{}", DIM, "PORT", "LOCAL", RESET);
    for (port, local) in ports_list() {
        println!("{:<8}{:<22}", port, local);
    }
    println!("{}end{}", DIM, RESET);
    println!();
    Outcome::Done(0)
}

fn dash_cmd(_arg: &str, sheet: &Sheet) -> Outcome {
    crate::dash::run(sheet)
}

fn help_cmd(_arg: &str, sheet: &Sheet) -> Outcome {
    banner::help(sheet);
    Outcome::Done(0)
}

fn clear_cmd(_arg: &str, _sheet: &Sheet) -> Outcome {
    print!("\x1b[2J\x1b[H");
    let _ = std::io::stdout().flush();
    Outcome::Done(0)
}

fn exit_cmd(_arg: &str, _sheet: &Sheet) -> Outcome {
    Outcome::Exit(0)
}

// ---------------- shared helpers ----------------

fn run_prog(bin: &str, args: &[&str]) -> bool {
    match Command::new(bin).args(args).status() {
        Ok(s) => s.success(),
        Err(_) => false,
    }
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
                    "*".to_string()
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

pub fn log(entry: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = if cfg!(windows) {
        dirs::data_local_dir().map(|d| d.join("ava").join("ava.log"))
    } else {
        dirs::home_dir().map(|h| h.join(".ava.log"))
    };
    if let Some(p) = path {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(
                f,
                "{} | {}@{} | {}",
                ts,
                banner::user(),
                banner::hostname(),
                entry
            );
        }
    }
}
