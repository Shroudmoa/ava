// repl: rustyline edition - history, tab completion, inline hints

use crate::cmds::{self, Outcome};
use crate::sheet::Sheet;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::hint::{Hinter, Hint};
use rustyline::highlight::Highlighter;
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};
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
        Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint))
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
    let user = crate::banner::user();
    let host = crate::banner::hostname();
    let dir = crate::banner::cwd();
    AvaPrompt {
        raw: format!("ava::{}@{} {} › ", host, user, dir),
        styled: format!(
            "{dr}ava{r}{d}::{r}{c}{host}{r}{d}@{r}{b}{user}{r}{y} {dir}{r}{m} ›{r} ",
            dr = crate::cmds::DARKRED,
            r = crate::cmds::RESET,
            d = crate::cmds::DIM,
            c = crate::cmds::CYAN,
            b = crate::cmds::BLUE,
            y = crate::cmds::YELLOW,
            m = crate::cmds::MAGENTA,
            host = host,
            user = user,
            dir = dir,
        ),
    }
}

pub fn run(sheet: &Sheet) -> i32 {
    let config = Config::builder()
        .completion_type(rustyline::CompletionType::List)
        .color_mode(rustyline::ColorMode::Enabled)
        .auto_add_history(true)
        .build();

    let mut rl: Editor<AvaHelper, rustyline::history::DefaultHistory> =
        match Editor::with_config(config) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("ava: readline init failed: {}", e);
                return 1;
            }
        };
    rl.set_helper(Some(AvaHelper {
        names: sheet.names(),
    }));

    let hist = dirs::home_dir().map(|h| h.join(".ava_history"));
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
                match cmds::dispatch(&line, sheet) {
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
    println!("{}bye.{}", crate::cmds::DIM, crate::cmds::RESET);
}
