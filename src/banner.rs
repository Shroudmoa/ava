// banner + help - the mova first impression

use crate::cmds::{bar, color_for, CYAN, DARKRED, DIM, GREEN, MAGENTA, RESET};
use crate::sheet::Sheet;
use sysinfo::System;

pub fn user() -> String {
    if cfg!(windows) {
        std::env::var("USERNAME").unwrap_or_else(|_| "you".into())
    } else {
        std::env::var("USER").unwrap_or_else(|_| "you".into())
    }
}

pub fn hostname() -> String {
    System::host_name().unwrap_or_else(|| "host".into())
}

pub fn cwd() -> String {
    let cwd = match std::env::current_dir() {
        Ok(p) => p.display().to_string(),
        Err(_) => return "?".into(),
    };
    if let Some(home) = dirs::home_dir() {
        if let Some(home) = home.to_str() {
            if let Some(rest) = cwd.strip_prefix(home) {
                return format!("~{}", rest);
            }
        }
    }
    cwd
}

pub fn banner() {
    let mut sys = System::new();
    sys.refresh_memory();

    let total = sys.total_memory();
    let avail = sys.available_memory();
    let used = total.saturating_sub(avail);
    let pct = if total > 0 { used * 100 / total } else { 0 };
    let up = System::uptime();
    let load = System::load_average();

    let art = [
        "     __         ",
        "  /\\|  |/\\  ava ",
        " /          \\   ",
        "/  \\____/\\  \\  ",
        "         \\__\\/  ",
    ];

    let info = [
        format!("{}@{}", user(), hostname()),
        format!("Uptime : {}d {}h {}m", up / 86400, (up % 86400) / 3600, (up % 3600) / 60),
        format!("Load   : {:.2} {:.2} {:.2}", load.one, load.five, load.fifteen),
        format!("RAM    : {}/{} GB {} {}%", to_gb(used), to_gb(total), bar(pct, 10), pct),
    ];
    let info_colors = [CYAN, DIM, DIM, color_for(pct)];

    println!();
    for i in 0..art.len() {
        print!("{}{}", DIM, art[i]);
        let pad = 17 - art[i].chars().count();
        print!("{}", " ".repeat(pad));
        if i < info.len() {
            println!("{}{}", info_colors[i], info[i]);
        } else if i == art.len() - 1 {
            println!("{}", RESET);
        } else {
            println!("{}", RESET);
        }
    }
    println!(
        "{}type {}999{} for commands · {}d{} dashboard · {}e{} exit{}\n",
        DIM, CYAN, DIM, MAGENTA, DIM, MAGENTA, DIM, RESET
    );
}

pub fn help(sheet: &Sheet) {
    println!();
    println!("  {}ava{} :: command overview", DARKRED, RESET);
    println!("  {}-----------------------{}", DIM, RESET);
    for entry in &sheet.cmds {
        let col = if entry.name.starts_with(|c: char| c.is_ascii_digit()) {
            CYAN
        } else {
            GREEN
        };
        print!("  {}{:<10} {}{}", col, entry.name, RESET, entry.desc);
        println!();
    }
    println!();
    println!(
        "  {}user sheet: ~/.ava/sheet.toml (add or shadow commands, no rebuild){}",
        DIM, RESET
    );
    println!(
        "  {}unknown input is passed to your shell (like mova){}",
        DIM, RESET
    );
    println!();
}

fn to_gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}
