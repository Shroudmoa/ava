// ava v0.2 - the mova concept for linux (and windows), written in rust
// short names, long reach. add commands in ~/.ava/sheet.toml - no rebuild needed.

mod banner;
mod cmds;
mod config;
mod dash;
mod repl;
mod sheet;

use cmds::Outcome;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sheet = sheet::Sheet::load();

    // no args: interactive repl
    if args.is_empty() {
        banner::banner();
        std::process::exit(repl::run(&sheet));
    }

    // one-shot: ava <command> [args]
    match args[0].as_str() {
        "--help" | "-h" => {
            banner::help(&sheet);
        }
        "--version" | "-V" => {
            println!("ava {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            let line = args.join(" ");
            let code = match cmds::dispatch(&line, &sheet) {
                Outcome::Done(c) => c,
                Outcome::Exit(c) => c,
            };
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
    }
}
