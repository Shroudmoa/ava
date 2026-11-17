// dash: live status header (the mova banner, but alive) + commands + little terminal
//
// The terminal pane is a REAL interactive shell in a pseudo-terminal (portable-pty):
// it has its own process, its own state (cwd, env, history, exports) and runs
// bash/fish/zsh/sh straight from the configured `[shell] name`. ava never
// interprets what you type - keystrokes are forwarded as raw bytes to the pty,
// and the pty's ANSI output is rendered with a vt100 emulator. There is no exec
// wrapper and no controlled command environment here.

use crate::cmds::{self, Outcome};
use crate::sheet::{Kind, Sheet};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::DefaultTerminal;
use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::Duration;
use sysinfo::System;
use vt100::{Cell, Color as VtColor, Parser, Screen};

struct Dash {
    sys: System,
    sel: usize,
    query: String,
    mode: Mode,
    focus: Focus,
    term_area: Rect,
    session: Option<ShellSession>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Mode {
    Normal,
    Search,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Focus {
    List,
    Term,
}

/// one live, independent shell session inside the dashboard terminal
struct ShellSession {
    pair: PtyPair,
    child: Box<dyn Child + Send + Sync>,
    rx: mpsc::Receiver<Vec<u8>>,
    parser: Parser,
    rows: u16,
    cols: u16,
}

impl ShellSession {
    fn spawn(shell: &str, rows: u16, cols: u16) -> io::Result<Self> {
        // vt100 (and most shells) misbehave on degenerate sizes - keep a safe floor
        let rows = rows.max(4);
        let cols = cols.max(20);
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        // spawn the shell DIRECTLY - no `-c`, no wrapper, no ava mediation
        let child = pair
            .slave
            .spawn_command(CommandBuilder::new(shell))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 4096];
            let mut r = reader;
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(ShellSession {
            pair,
            child,
            rx,
            parser: Parser::new(rows, cols, 2000),
            rows,
            cols,
        })
    }

    /// feed any pending pty output into the terminal emulator
    fn drain(&mut self) {
        while let Ok(chunk) = self.rx.try_recv() {
            self.answer_queries(&chunk);
            self.parser.process(&chunk);
        }
    }

    /// a real terminal answers these probes; some shells (fish/zsh themes)
    /// block waiting for the reply before drawing a prompt.
    fn answer_queries(&mut self, chunk: &[u8]) {
        if contains(chunk, b"\x1b]10;?") {
            self.write(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
        }
        if contains(chunk, b"\x1b]11;?") {
            self.write(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
        }
        if contains(chunk, b"\x1b[c") || contains(chunk, b"\x1b[0c") {
            self.write(b"\x1b[?1;2c");
        }
    }

    /// send raw keystroke bytes straight to the shell
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.pair.master.write_all(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(4);
        let cols = cols.max(20);
        if rows == self.rows && cols == self.cols {
            return;
        }
        let _ = self.pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser.set_size(rows, cols);
        self.rows = rows;
        self.cols = cols;
    }

    fn alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

pub fn run(sheet: &Sheet, shell: &str) -> Outcome {
    match run_inner(sheet, shell) {
        Ok(true) => Outcome::Exit(0),
        Ok(false) => Outcome::Done(0),
        Err(e) => {
            eprintln!("{}dash: {}{}", cmds::RED, e, cmds::RESET);
            Outcome::Done(1)
        }
    }
}

fn run_inner(sheet: &Sheet, shell: &str) -> io::Result<bool> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, sheet, shell);
    ratatui::restore();
    result
}

/// what a single key press should do at the dashboard level
enum Nav {
    Stay,
    Done,
    Exit,
}

fn event_loop(terminal: &mut DefaultTerminal, sheet: &Sheet, shell: &str) -> io::Result<bool> {
    let mut dash = Dash {
        sys: System::new(),
        sel: 0,
        query: String::new(),
        mode: Mode::Normal,
        focus: Focus::List,
        term_area: Rect::default(),
        session: None,
    };
    dash.sys.refresh_memory();

    loop {
        if let Some(s) = dash.session.as_mut() {
            s.drain();
        }
        terminal.draw(|f| ui(f, &mut dash, sheet))?;

        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            match ev {
                Event::Resize(w, h) => {
                    let (rows, cols) = term_size(&dash, w, h);
                    if let Some(s) = dash.session.as_mut() {
                        s.resize(rows, cols);
                    }
                    continue;
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(terminal, &mut dash, sheet, shell, key)? {
                        Nav::Stay => {}
                        Nav::Done => {
                            if let Some(s) = dash.session.as_mut() {
                                s.kill();
                            }
                            return Ok(false);
                        }
                        Nav::Exit => {
                            if let Some(s) = dash.session.as_mut() {
                                s.kill();
                            }
                            return Ok(true);
                        }
                    }
                }
                _ => {}
            }
        } else {
            dash.sys.refresh_memory();
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// reuse the running shell; only spawn a new one on first use or if it exited
fn ensure_session(dash: &mut Dash, shell: &str) -> io::Result<()> {
    if dash.session.as_mut().is_some_and(|s| !s.alive()) {
        dash.session = None;
    }
    if dash.session.is_none() {
        let (rows, cols) = term_size(dash, 0, 0);
        dash.session = Some(ShellSession::spawn(shell, rows, cols)?);
    }
    Ok(())
}

/// decide the pty size from the terminal pane's last rendered rect
fn term_size(dash: &Dash, screen_w: u16, screen_h: u16) -> (u16, u16) {
    let a = dash.term_area;
    let rows = if a.height > 2 { a.height - 2 } else { screen_h.saturating_sub(9).max(1) };
    let cols = if a.width > 2 { a.width - 2 } else { screen_w.saturating_sub(2).max(1) };
    (rows, cols)
}

fn handle_key(
    terminal: &mut DefaultTerminal,
    dash: &mut Dash,
    sheet: &Sheet,
    shell: &str,
    key: KeyEvent,
) -> io::Result<Nav> {
    // ---- terminal focus: forward EVERYTHING to the shell --------------------
    if dash.focus == Focus::Term {
        // esc and ctrl+t just change focus - the shell KEEPS RUNNING in the
        // background (cwd, env, exports, running jobs all survive) so it is
        // still there when you come back. it is only killed when ava exits.
        if key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            dash.focus = Focus::List;
            return Ok(Nav::Stay);
        }

        // NOTE: tab is deliberately NOT a dashboard keybinding. It is passed
        // straight through to the shell so tab-completion works like normal.
        let bytes: Option<Vec<u8>> = match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // ctrl+letter -> control byte (ctrl+c, ctrl+d, ctrl+l, ...)
                if c.is_ascii_lowercase() {
                    Some(vec![c as u8 - b'a' + 1])
                } else {
                    None
                }
            }
            KeyCode::Char(c) => Some(c.to_string().into_bytes()),
            KeyCode::Enter => Some(b"\r".to_vec()),
            KeyCode::Tab => Some(b"\t".to_vec()),
            KeyCode::Backspace => Some(vec![0x7f]),
            KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
            KeyCode::Up => Some(b"\x1b[A".to_vec()),
            KeyCode::Down => Some(b"\x1b[B".to_vec()),
            KeyCode::Right => Some(b"\x1b[C".to_vec()),
            KeyCode::Left => Some(b"\x1b[D".to_vec()),
            KeyCode::Home => Some(b"\x1b[H".to_vec()),
            KeyCode::End => Some(b"\x1b[F".to_vec()),
            KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
            KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
            _ => None,
        };
        if let Some(bytes) = bytes {
            if let Some(s) = dash.session.as_mut() {
                s.write(&bytes);
            }
        }
        return Ok(Nav::Stay);
    }

    // ---- list focus ---------------------------------------------------------
    match dash.mode {
        Mode::Search => match key.code {
            KeyCode::Esc => {
                dash.query.clear();
                dash.sel = 0;
                dash.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                dash.query.pop();
                dash.sel = 0;
            }
            KeyCode::Enter => return run_selected(terminal, dash, sheet, shell),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                dash.query.push(c);
                dash.sel = 0;
            }
            _ => {}
        },
        Mode::Normal => match key.code {
            KeyCode::Char('q') => return Ok(Nav::Done),
            KeyCode::Esc => return Ok(Nav::Done),
            KeyCode::Char('/') => dash.mode = Mode::Search,
            KeyCode::Up | KeyCode::Char('k') => move_sel(dash, sheet, -1),
            KeyCode::Down | KeyCode::Char('j') => move_sel(dash, sheet, 1),
            // ctrl+t: jump into the terminal (reuse the live shell, or spawn one)
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                dash.focus = Focus::Term;
                ensure_session(dash, shell)?;
            }
            KeyCode::Enter => return run_selected(terminal, dash, sheet, shell),
            _ => {}
        },
    }
    Ok(Nav::Stay)
}

/// run the highlighted command: shell entries are typed into the live terminal,
/// ava builtins suspend the TUI and run normally.
fn run_selected(
    terminal: &mut DefaultTerminal,
    dash: &mut Dash,
    sheet: &Sheet,
    shell: &str,
) -> io::Result<Nav> {
    let entries = sheet.filter(&dash.query);
    if entries.is_empty() {
        return Ok(Nav::Stay);
    }
    let n = entries.len();
    if dash.sel >= n {
        dash.sel = n - 1;
    }
    let entry = entries[dash.sel];
    if entry.kind == Kind::Shell {
        dash.focus = Focus::Term;
        ensure_session(dash, shell)?;
        if let Some(s) = dash.session.as_mut() {
            s.write(entry.run.as_bytes());
            s.write(b"\r");
        }
        Ok(Nav::Stay)
    } else if run_in_terminal(terminal, &entry.name, sheet) {
        Ok(Nav::Exit)
    } else {
        Ok(Nav::Stay)
    }
}

/// move the cursor within the filtered list
fn move_sel(dash: &mut Dash, sheet: &Sheet, delta: isize) {
    let n = sheet.filter(&dash.query).len();
    if n == 0 {
        dash.sel = 0;
        return;
    }
    dash.sel = (dash.sel as isize + delta).clamp(0, n as isize - 1) as usize;
}

/// suspend the tui, run the ava builtin, wait, resume.
/// returns true if the command asked to exit (Outcome::Exit)
fn run_in_terminal(terminal: &mut DefaultTerminal, name: &str, sheet: &Sheet) -> bool {
    ratatui::restore();
    println!();
    let quit = matches!(cmds::dispatch(name, sheet), Outcome::Exit(_));
    if !quit {
        println!("{}press enter to return to dash{}", cmds::DIM, cmds::RESET);
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }
    *terminal = ratatui::init();
    quit
}

fn ui(f: &mut ratatui::Frame, dash: &mut Dash, sheet: &Sheet) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // live header
            Constraint::Min(3),     // body: commands + little terminal
            Constraint::Length(1), // footer
        ])
        .split(f.area());

    render_header(f, dash, chunks[0]);
    render_body(f, dash, sheet, chunks[1]);
    render_footer(f, dash, chunks[2]);
}

fn render_header(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
    let sys = &dash.sys;
    let total = sys.total_memory();
    let avail = sys.available_memory();
    let used = total.saturating_sub(avail);
    let pct = if total > 0 { used * 100 / total } else { 0 };
    let up = System::uptime();
    let load = System::load_average();

    let gauge_color = if pct >= 90 {
        Color::Red
    } else if pct >= 75 {
        Color::Yellow
    } else {
        Color::Green
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                "ava",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::DIM),
            ),
            Span::styled("::", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{}@{}",
                    System::host_name().unwrap_or_default(),
                    crate::banner::user()
                ),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(format!(
            "Uptime : {}d {}h {}m",
            up / 86400,
            (up % 86400) / 3600,
            (up % 3600) / 60
        )),
        Line::from(format!(
            "Load   : {:.2} {:.2} {:.2}",
            load.one, load.five, load.fifteen
        )),
    ];

    let para = Paragraph::new(lines).block(Block::bordered().title(" status "));
    f.render_widget(para, area);

    let gauge_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color))
        .percent(pct.min(100) as u16)
        .label(format!(
            "RAM {}/{} GB ({}%)",
            cmds::to_gb(used),
            cmds::to_gb(total),
            pct
        ));
    f.render_widget(gauge, gauge_area);
}

fn render_body(f: &mut ratatui::Frame, dash: &mut Dash, sheet: &Sheet, area: Rect) {
    // focus List: command list on top, little terminal underneath.
    // focus Term: the terminal fills the body.
    let chunks = if dash.focus == Focus::Term {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Min(3)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Percentage(45), // commands (smaller)
                Constraint::Min(3),         // little terminal
            ])
            .split(area)
    };

    let (list_area, term_area) = if dash.focus == Focus::Term {
        (None, Some(chunks[0]))
    } else {
        (Some(chunks[0]), Some(chunks[1]))
    };

    if let Some(la) = list_area {
        render_list(f, dash, sheet, la);
    }
    if let Some(ta) = term_area {
        dash.term_area = ta;
        render_terminal(f, dash, ta);
    }
}

fn render_list(f: &mut ratatui::Frame, dash: &Dash, sheet: &Sheet, area: Rect) {
    let entries = sheet.filter(&dash.query);
    let items: Vec<ListItem> = entries
        .iter()
        .map(|c| ListItem::new(Line::from(format!("{:<10} {}", c.name, c.desc))))
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(" commands "))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Magenta));
    let mut state = ListState::default();
    if !entries.is_empty() {
        state.select(Some(dash.sel.min(entries.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_terminal(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
    let focused = dash.focus == Focus::Term;
    let title = if focused {
        " terminal [live] "
    } else if dash.session.is_some() {
        " terminal [running - ctrl+t] "
    } else {
        " terminal "
    };
    let border_style = if focused {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default()
    };
    let block = Block::bordered()
        .title(title)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line<'static>> = match dash.session.as_ref() {
        Some(s) => screen_lines(s.parser.screen()),
        None => vec![Line::from(Span::styled(
            "no live shell - press ctrl+t to open one",
            Style::default().fg(Color::DarkGray),
        ))],
    };
    f.render_widget(Paragraph::new(lines), inner);
}

/// turn the vt100 screen into ratatui lines, preserving colors/attrs
fn screen_lines(screen: &Screen) -> Vec<Line<'static>> {
    let (rows, cols) = screen.size();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(rows as usize);

    for row in 0..rows {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut style = Style::default();
        let mut text = String::new();
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let cell_style = cell_style(cell);
            if cell_style != style && !text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut text), style));
                style = cell_style;
            }
            let content = cell.contents();
            text.push_str(if content.is_empty() { " " } else { &content });
        }
        if !text.is_empty() {
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    // drop trailing blank rows so the prompt sits naturally in the pane
    while lines
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
    {
        lines.pop();
    }
    lines
}

fn cell_style(cell: &Cell) -> Style {
    let mut fg = map_color(cell.fgcolor());
    let mut bg = map_color(cell.bgcolor());
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    Style::default().fg(fg).bg(bg).add_modifier(modifiers)
}

fn map_color(c: VtColor) -> Color {
    match c {
        VtColor::Default => Color::Reset,
        VtColor::Idx(i) => Color::Indexed(i),
        VtColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn render_footer(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
    let content = if dash.focus == Focus::Term {
        " ctrl+t / esc back to list (shell keeps running) · tab completes ".to_string()
    } else if dash.mode == Mode::Normal {
        " / search · ctrl+t terminal · ↑↓/jk move · enter run · q exit ".to_string()
    } else {
        format!("{} · enter run · esc clear ", dash.query)
    };
    let footer = Paragraph::new(Span::styled(content, Style::default().fg(Color::DarkGray)));
    f.render_widget(footer, area);
}
