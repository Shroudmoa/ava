// banner + help - the mova first impression

use crate::cmds::{bar, color_for, format_uptime, to_gb, CYAN, DARKRED, DIM, GREEN, MAGENTA, RESET};
use crate::color;
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
        format!("Uptime : {}", format_uptime(up)),
        format!("Load   : {:.2} {:.2} {:.2}", load.one, load.five, load.fifteen),
        format!(
            "RAM    : {}/{} GB {} {}%",
            to_gb(used),
            to_gb(total),
            bar(pct, 10),
            pct
        ),
    ];
    let info_colors = [CYAN, DIM, DIM, color_for(pct)];

    println!();
    for (i, line) in art.iter().enumerate() {
        print!("{}{}", color::seq(DIM), line);
        let pad = 17 - line.chars().count();
        print!("{}", " ".repeat(pad));
        if i < info.len() {
            println!("{}{}", color::seq(info_colors[i]), info[i]);
        } else {
            println!("{}", color::seq(RESET));
        }
    }
    println!(
        "{}type {}999{} for commands · {}d{} dashboard · {}e{} exit{}\n",
        color::seq(DIM),
        color::seq(CYAN),
        color::seq(DIM),
        color::seq(MAGENTA),
        color::seq(DIM),
        color::seq(MAGENTA),
        color::seq(DIM),
        color::seq(RESET)
    );
}

pub fn help(sheet: &Sheet) {
    println!();
    println!(
        "  {}ava{} :: command overview",
        color::seq(DARKRED),
        color::seq(RESET)
    );
    println!(
        "  {}-----------------------{}",
        color::seq(DIM),
        color::seq(RESET)
    );
    for entry in &sheet.cmds {
        let col = if entry.name.starts_with(|c: char| c.is_ascii_digit()) {
            CYAN
        } else {
            GREEN
        };
        println!(
            "  {}{:<10} {}{}",
            color::seq(col),
            entry.name,
            color::seq(RESET),
            entry.desc
        );
    }
    println!();
    println!("  {}commands: {}{}", color::seq(DIM), sheet.cmds.len(), color::seq(RESET));
    println!("  {}loaded sheets:", color::seq(DIM));
    for s in sheet.user_sheets() {
        println!("    {}{}", color::seq(GREEN), s.name);
        println!("      {} - {}", color::seq(DIM), s.path.display());
        if s.entries == 1 {
            println!("      1 command");
        } else {
            println!("      {} commands", s.entries);
        }
    }
    println!(
        "  {}config: {}{}",
        color::seq(DIM),
        crate::config::Config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(default)".into()),
        color::seq(RESET)
    );
    println!(
        "  {}unknown input is passed to your shell (like mova){}",
        color::seq(DIM),
        color::seq(RESET)
    );
    println!();
}
