// ava v0.4 - the mova concept for linux (and windows), written in rust
// short names, long reach. add commands in ~/.ava/sheet.toml - no rebuild needed.

mod banner;
mod cmds;
mod color;
mod config;
mod dash;
mod mag;
mod repl;
mod sheet;

use cmds::Outcome;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // one config snapshot for the whole process: sheets, color, history, dashboard
    let config = config::Config::load();
    color::set_enabled(config.interface.color);
    let sheet = sheet::Sheet::load(&config);

    // no args: interactive repl
    if args.is_empty() {
        banner::banner();
        std::process::exit(repl::run(&sheet, &config));
    }

    // one-shot: ava <command> [args]
    match args[0].as_str() {
        "--help" | "-h" => {
            banner::help(&sheet);
        }
        "--version" | "-V" => {
            println!("ava {} · visionmaxx", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            // rebuild a shell-safe line: joining raw args would eat quoting
            // (ava echo "a  b" must not become `echo a b`)
            let line = shell_line(&args);
            let code = match cmds::dispatch(&line, &sheet, &config, false) {
                Outcome::Done(c) => c,
                Outcome::Exit(c) => c,
            };
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
    }
}

/// reconstruct a command line from argv, quoting only where needed
fn shell_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            let safe = !a.is_empty()
                && a.bytes().all(|b| {
                    b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b)
                });
            if safe {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_line_preserves_spaces_and_quotes() {
        let args = vec![
            "echo".to_string(),
            "a  b".to_string(),
            "it's".to_string(),
        ];
        assert_eq!(shell_line(&args), "echo 'a  b' 'it'\\''s'");
    }

    #[test]
    fn shell_line_leaves_simple_args_alone() {
        let args = vec!["echo".to_string(), "hello".to_string(), "-n".to_string()];
        assert_eq!(shell_line(&args), "echo hello -n");
    }
}
