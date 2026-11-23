// repl: rustyline edition - history, tab completion, inline hints

use crate::cmds::{self, Outcome};
use crate::color;
use crate::config::Config as AvaConfig;
use crate::sheet::Sheet;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::hint::{Hint, Hinter};
use rustyline::highlight::Highlighter;
use rustyline::validate::Validator;
use rustyline::{ColorMode, Config, Context, Editor, Helper};
use std::borrow::Cow;

struct AvaHelper {
    names: Vec<String>,
}

impl Completer for AvaHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // complete the first word only
        if line[..pos].contains(char::is_whitespace) {
            return Ok((pos, Vec::new()));
        }
        let candidates = self
            .names
            .iter()
            .filter(|n| n.starts_with(&line[..pos]))
            .map(|n| Pair {
                display: n.clone(),
                replacement: n.clone(),
            })
            .collect();
        Ok((0, candidates))
    }
}

/// inline grey hint of the rest of a unique command name
struct AvaHint(String);

impl Hint for AvaHint {
    fn display(&self) -> &str {
        &self.0
    }

    fn completion(&self) -> Option<&str> {
        Some(&self.0)
    }
}

impl Hinter for AvaHelper {
    type Hint = AvaHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<AvaHint> {
        if line.is_empty() || pos != line.len() || line.contains(char::is_whitespace) {
            return None;
        }
        let matches: Vec<&String> = self
            .names
            .iter()
            .filter(|n| n.starts_with(line) && *n != line)
            .collect();
        if matches.len() == 1 {
            Some(AvaHint(matches[0][line.len()..].to_string()))
        } else {
            None
        }
    }
}

impl Highlighter for AvaHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!(
            "{}{}{}",
            color::seq("\x1b[90m"),
            hint,
            color::seq("\x1b[0m")
        ))
    }
}

impl Validator for AvaHelper {}
impl Helper for AvaHelper {}

struct AvaPrompt {
    raw: String,
    styled: String,
}

impl rustyline::Prompt for AvaPrompt {
    fn raw(&self) -> &str {
        &self.raw
    }
    fn styled(&self) -> &str {
        &self.styled
    }
}

fn prompt() -> AvaPrompt {
    let s = color::seq;
    let user = crate::banner::user();
    let host = crate::banner::hostname();
    let dir = crate::banner::cwd();
    AvaPrompt {
        raw: format!("ava::{}@{} {} › ", host, user, dir),
        styled: format!(
            "{dr}ava{r}{d}::{r}{c}{host}{r}{d}@{r}{b}{user}{r}{y} {dir}{r}{m} ›{r} ",
            dr = s(crate::cmds::DARKRED),
            r = s(crate::cmds::RESET),
            d = s(crate::cmds::DIM),
            c = s(crate::cmds::CYAN),
            b = s(crate::cmds::BLUE),
            y = s(crate::cmds::YELLOW),
            m = s(crate::cmds::MAGENTA),
            host = host,
            user = user,
            dir = dir,
        ),
    }
}

pub fn run(sheet: &Sheet, config: &AvaConfig) -> i32 {
    let builder = Config::builder()
        .completion_type(rustyline::CompletionType::List)
        .auto_add_history(config.history.enabled);
    let builder = if config.interface.color {
        builder.color_mode(ColorMode::Enabled)
    } else {
        builder.color_mode(ColorMode::Disabled)
    };

    let mut rl: Editor<AvaHelper, rustyline::history::DefaultHistory> =
        match Editor::with_config(builder.build()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("ava: readline init failed: {}", e);
                return 1;
            }
        };
    rl.set_helper(Some(AvaHelper {
        names: sheet.names(),
    }));

    // [history] enabled = false means no read, no write, no inline additions
    let hist = if config.history.enabled {
        crate::config::Config::history_path()
    } else {
        None
    };
    if let Some(h) = &hist {
        let _ = rl.load_history(h);
    }

    let mut code = 0;
    loop {
        match rl.readline(&prompt()) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match cmds::dispatch(&line, sheet, config, true) {
                    Outcome::Done(_) => {}
                    Outcome::Exit(c) => {
                        code = c;
                        log_exit();
                        break;
                    }
                }
            }
            Err(ReadlineError::Interrupted) => continue, // ctrl-c = cancel line, mova style
            Err(ReadlineError::Eof) => {
                log_exit();
                break;
            }
            Err(e) => {
                eprintln!("ava: readline error: {}", e);
                code = 1;
                break;
            }
        }
    }

    if let Some(h) = &hist {
        let _ = rl.save_history(h);
    }
    code
}

fn log_exit() {
    cmds::log("EXIT");
    println!(
        "{}bye. · visionmaxx{}",
        color::seq(crate::cmds::DIM),
        color::seq(crate::cmds::RESET)
    );
}
