// dash: live status header (the mova banner, but alive) + commands + little terminal
//
// The terminal pane is a REAL interactive shell in a pseudo-terminal (portable-pty):
// it has its own process, its own state (cwd, env, history, exports) and runs
// bash/fish/zsh/sh straight from the configured `[shell] name`. ava never
// interprets what you type - keystrokes are forwarded as raw bytes to the pty,
// and the pty's ANSI output is rendered with a vt100 emulator. There is no exec
// wrapper and no controlled command environment here.

use crate::cmds::{self, Outcome};
use crate::color;
use crate::config::Config;
use crate::sheet::{Kind, Sheet};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::DefaultTerminal;
use std::io::{self, IsTerminal, Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};
use vt100::{Cell, Color as VtColor, MouseProtocolEncoding, MouseProtocolMode, Parser, Screen};

struct Dash {
    sys: System,
    disks: Disks,
    nets: Networks,
    net_rx: u64,
    net_tx: u64,
    net_rate_rx: f64,
    net_rate_tx: f64,
    last_refresh: Instant,
    sel: usize,
    query: String,
    mode: Mode,
    focus: Focus,
    term_area: Rect,
    /// inner (border-free) rect: this is the actual pty size and mouse target
    term_inner: Rect,
    session: Option<ShellSession>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Mode {
    Normal,
    Search,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
    /// rows back from the live screen (0 = following the prompt)
    scroll: usize,
    scroll_max: usize,
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
            parser: Parser::new(rows, cols, 5000),
            rows,
            cols,
            scroll: 0,
            scroll_max: 0,
        })
    }

    /// feed any pending pty output into the terminal emulator
    fn drain(&mut self) {
        while let Ok(chunk) = self.rx.try_recv() {
            self.answer_queries(&chunk);
            self.parser.process(&chunk);
        }
        // vt100 keeps the view anchored automatically when output arrives while
        // scrolled; mirror that offset into our own state for the indicator.
        self.scroll = self.parser.screen().scrollback();
        self.scroll_max = self.probe_scrollback();
        self.parser.set_scrollback(self.scroll);
    }

    fn probe_scrollback(&mut self) -> usize {
        self.parser.set_scrollback(usize::MAX);
        let max = self.parser.screen().scrollback();
        self.parser.set_scrollback(self.scroll);
        max
    }

    /// scroll the visible history; negative = up into scrollback
    fn scroll(&mut self, delta: isize) {
        self.scroll_max = self.probe_scrollback();
        self.scroll = (self.scroll as isize + delta).clamp(0, self.scroll_max as isize) as usize;
        self.parser.set_scrollback(self.scroll);
    }

    fn follow(&mut self) {
        self.scroll = 0;
        self.parser.set_scrollback(0);
    }

    fn scroll_info(&self) -> (usize, usize) {
        (self.scroll, self.scroll_max)
    }

    fn mouse_state(&self) -> (MouseProtocolMode, MouseProtocolEncoding) {
        let screen = self.parser.screen();
        (screen.mouse_protocol_mode(), screen.mouse_protocol_encoding())
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

    fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// keeps mouse capture from leaking into the caller's terminal on any exit path
struct MouseGuard;
impl Drop for MouseGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
}

pub fn run(sheet: &Sheet, config: &Config) -> Outcome {
    if !config.interface.dashboard {
        eprintln!(
            "{}dash is disabled by [interface] dashboard = false{}",
            color::seq(cmds::RED),
            color::seq(cmds::RESET)
        );
        return Outcome::Done(1);
    }
    // ratatui needs raw mode; fail cleanly when ava is used from a script/pipe
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!(
            "{}dash needs an interactive terminal{}",
            color::seq(cmds::RED),
            color::seq(cmds::RESET)
        );
        return Outcome::Done(1);
    }

    match run_inner(sheet, config) {
        Ok(true) => Outcome::Exit(0),
        Ok(false) => Outcome::Done(0),
        Err(e) => {
            eprintln!(
                "{}dash: {}{}",
                color::seq(cmds::RED),
                e,
                color::seq(cmds::RESET)
            );
            Outcome::Done(1)
        }
    }
}

fn run_inner(sheet: &Sheet, config: &Config) -> io::Result<bool> {
    let shell = config.shell().unwrap_or("bash");
    let mut terminal = ratatui::try_init()?;
    execute!(io::stdout(), EnableMouseCapture)?;
    let _mouse = MouseGuard;
    let result = event_loop(&mut terminal, sheet, shell, config);
    ratatui::restore();
    result
}

/// what a single key press should do at the dashboard level
enum Nav {
    Stay,
    Done,
    Exit,
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    sheet: &Sheet,
    shell: &str,
    config: &Config,
) -> io::Result<bool> {
    let nets = Networks::new_with_refreshed_list();
    let (net_rx, net_tx) = network_totals(&nets);
    let mut dash = Dash {
        sys: System::new_all(),
        disks: Disks::new_with_refreshed_list(),
        nets,
        net_rx,
        net_tx,
        net_rate_rx: 0.0,
        net_rate_tx: 0.0,
        last_refresh: Instant::now(),
        sel: 0,
        query: String::new(),
        mode: Mode::Normal,
        focus: Focus::List,
        term_area: Rect::default(),
        term_inner: Rect::default(),
        session: None,
    };
    refresh_stats(&mut dash);

    loop {
        if let Some(s) = dash.session.as_mut() {
            s.drain();
        }
        refresh_stats(&mut dash);
        terminal.draw(|f| ui(f, &mut dash, sheet))?;
        // the pane can change size on focus switch or window resize: keep the
        // pty in sync with what was actually rendered (this was the "tiny
        // terminal even when focused" bug).
        sync_term_size(&mut dash);

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(terminal, &mut dash, sheet, shell, config, key)? {
                        Nav::Stay => {}
                        Nav::Done => {
                            kill_session(&mut dash);
                            return Ok(false);
                        }
                        Nav::Exit => {
                            kill_session(&mut dash);
                            return Ok(true);
                        }
                    }
                }
                Event::Mouse(mouse) => handle_mouse(&mut dash, mouse)?,
                _ => {}
            }
        }
    }
}

fn kill_session(dash: &mut Dash) {
    if let Some(s) = dash.session.as_mut() {
        s.kill();
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// gather expensive stats at most once a second; network counters are deltas
fn refresh_stats(dash: &mut Dash) {
    let now = Instant::now();
    let elapsed = now.duration_since(dash.last_refresh);
    if elapsed < Duration::from_millis(1000) {
        return;
    }
    let secs = elapsed.as_secs_f64().max(0.001);
    let (rx, tx) = network_totals(&dash.nets);
    dash.net_rate_rx = rx.saturating_sub(dash.net_rx) as f64 / secs;
    dash.net_rate_tx = tx.saturating_sub(dash.net_tx) as f64 / secs;
    dash.net_rx = rx;
    dash.net_tx = tx;
    dash.last_refresh = now;

    dash.sys.refresh_cpu_all();
    dash.sys.refresh_memory();
    dash.sys.refresh_processes(ProcessesToUpdate::All, true);
    dash.disks.refresh(true);
    dash.nets.refresh(true);
}

fn network_totals(nets: &Networks) -> (u64, u64) {
    let mut rx = 0;
    let mut tx = 0;
    for (name, data) in nets {
        if name == "lo" || name.starts_with("lo") {
            continue;
        }
        rx += data.received();
        tx += data.transmitted();
    }
    (rx, tx)
}

fn human_rate(bytes: f64) -> String {
    if bytes >= 1024.0 * 1024.0 {
        format!("{:.1}M", bytes / 1024.0 / 1024.0)
    } else if bytes >= 1024.0 {
        format!("{:.1}K", bytes / 1024.0)
    } else {
        format!("{:.0}B", bytes)
    }
}

fn sync_term_size(dash: &mut Dash) {
    let inner = dash.term_inner;
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if let Some(s) = dash.session.as_mut() {
        s.resize(inner.height, inner.width);
    }
}

/// reuse the running shell; only spawn a new one on first use or if it exited
fn ensure_session(dash: &mut Dash, shell: &str) -> io::Result<()> {
    if dash.session.as_mut().is_some_and(|s| !s.alive()) {
        dash.session = None;
    }
    if dash.session.is_none() {
        let rows = dash.term_inner.height.max(4);
        let cols = dash.term_inner.width.max(20);
        dash.session = Some(ShellSession::spawn(shell, rows, cols)?);
    }
    Ok(())
}

fn scroll_session(dash: &mut Dash, delta: isize) {
    if let Some(s) = dash.session.as_mut() {
        s.scroll(delta);
    }
}

fn handle_key(
    terminal: &mut DefaultTerminal,
    dash: &mut Dash,
    sheet: &Sheet,
    shell: &str,
    config: &Config,
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

        // ava scroll keys use alt, so programs like tmux/less still get their
        // own page-up/page-down and normal key sequences untouched.
        if key.modifiers.contains(KeyModifiers::ALT) {
            let page = dash.term_inner.height.max(1) as isize;
            let scroll = match key.code {
                KeyCode::Up => Some(-1),
                KeyCode::Down => Some(1),
                KeyCode::PageUp => Some(-page),
                KeyCode::PageDown => Some(page),
                KeyCode::Home => Some(isize::MIN),
                KeyCode::End => Some(isize::MAX),
                _ => None,
            };
            if let Some(delta) = scroll {
                scroll_session(dash, delta);
                return Ok(Nav::Stay);
            }
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
                // typing means the user wants to see the live prompt again
                s.follow();
                s.write(&bytes);
            }
        }
        return Ok(Nav::Stay);
    }

    // ---- list focus ---------------------------------------------------------
    let page = dash.term_inner.height.max(1) as isize;
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
            KeyCode::Enter => return run_selected(terminal, dash, sheet, shell, config),
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
            KeyCode::PageUp => move_sel(dash, sheet, -page),
            KeyCode::PageDown => move_sel(dash, sheet, page),
            // ctrl+t: jump into the terminal (reuse the live shell, or spawn one)
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                dash.focus = Focus::Term;
                ensure_session(dash, shell)?;
            }
            KeyCode::Enter => return run_selected(terminal, dash, sheet, shell, config),
            _ => {}
        },
    }
    Ok(Nav::Stay)
}

/// mouse wheel scrolls the pane; clicks are forwarded when the shell enabled
/// mouse reporting (so tmux, vim, less, ... keep working normally).
fn handle_mouse(dash: &mut Dash, mouse: MouseEvent) -> io::Result<()> {
    let inner = dash.term_inner;
    let inside = mouse.column >= inner.x
        && mouse.column < inner.x.saturating_add(inner.width)
        && mouse.row >= inner.y
        && mouse.row < inner.y.saturating_add(inner.height);
    if !inside {
        return Ok(());
    }

    let (mode, encoding) = dash
        .session
        .as_ref()
        .map(|s| s.mouse_state())
        .unwrap_or((MouseProtocolMode::None, MouseProtocolEncoding::Default));

    let wheel = matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    );
    if wheel && mode == MouseProtocolMode::None {
        scroll_session(
            dash,
            if mouse.kind == MouseEventKind::ScrollUp {
                -3
            } else {
                3
            },
        );
        return Ok(());
    }
    if dash.focus != Focus::Term || mode == MouseProtocolMode::None {
        return Ok(());
    }

    let (button, press) = match mouse.kind {
        MouseEventKind::Down(b) => (mouse_button(b), true),
        MouseEventKind::Up(b) => (mouse_button(b), false),
        MouseEventKind::Drag(b) => (mouse_button(b), true),
        MouseEventKind::Moved => (32, true),
        MouseEventKind::ScrollUp => (64, true),
        MouseEventKind::ScrollDown => (65, true),
        MouseEventKind::ScrollLeft => (66, true),
        MouseEventKind::ScrollRight => (67, true),
    };
    let mut code = button;
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        code |= 4;
    }
    if mouse.modifiers.contains(KeyModifiers::ALT) {
        code |= 8;
    }
    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
        code |= 16;
    }
    let col = mouse.column.saturating_sub(inner.x) + 1;
    let row = mouse.row.saturating_sub(inner.y) + 1;

    let seq = match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = if press { 'M' } else { 'm' };
            format!("\x1b[<{};{};{}{}", code, col, row, end).into_bytes()
        }
        _ => vec![
            0x1b,
            b'[',
            b'M',
            code.wrapping_add(32) as u8,
            (col as u8).wrapping_add(32),
            (row as u8).wrapping_add(32),
        ],
    };
    if let Some(s) = dash.session.as_mut() {
        s.follow();
        s.write(&seq);
    }
    Ok(())
}

fn mouse_button(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// run the highlighted command: shell entries are typed into the live terminal,
/// ava builtins suspend the TUI and run normally.
fn run_selected(
    terminal: &mut DefaultTerminal,
    dash: &mut Dash,
    sheet: &Sheet,
    shell: &str,
    config: &Config,
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
            // honor the [win] override on windows, just like the repl dispatch
            let command = sheet.shell_for(entry);
            s.follow();
            s.write(command.as_bytes());
            s.write(b"\r");
        }
        Ok(Nav::Stay)
    } else if run_in_terminal(terminal, &entry.name, sheet, config) {
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
fn run_in_terminal(
    terminal: &mut DefaultTerminal,
    name: &str,
    sheet: &Sheet,
    config: &Config,
) -> bool {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    println!();
    let quit = matches!(cmds::dispatch(name, sheet, config, true), Outcome::Exit(_));
    if !quit {
        println!(
            "{}press enter to return to dash{}",
            color::seq(cmds::DIM),
            color::seq(cmds::RESET)
        );
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }
    match ratatui::try_init() {
        Ok(t) => *terminal = t,
        Err(e) => eprintln!("dash: cannot restore terminal: {}", e),
    }
    let _ = execute!(io::stdout(), EnableMouseCapture);
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
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    render_status(f, dash, cols[0]);
    render_system(f, dash, cols[1]);
}

fn render_status(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
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

    let dim_red = if color::enabled() {
        Style::default().fg(Color::Red).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("ava", dim_red),
            Span::styled("::", fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{}@{}",
                    System::host_name().unwrap_or_default(),
                    crate::banner::user()
                ),
                fg(Color::Cyan),
            ),
        ]),
        Line::from(format!("Uptime : {}", cmds::format_uptime(up))),
        Line::from(format!(
            "Load   : {:.2} {:.2} {:.2}",
            load.one, load.five, load.fifteen
        )),
    ];
    let block = Block::bordered().title(" status ");
    f.render_widget(Paragraph::new(lines).block(block), area);

    let gauge_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(2),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let gauge = Gauge::default()
        .gauge_style(fg(gauge_color))
        .percent(pct.min(100) as u16)
        .label(format!(
            "RAM {}/{} GB ({}%)",
            cmds::to_gb(used),
            cmds::to_gb(total),
            pct
        ));
    f.render_widget(gauge, gauge_area);
}

fn render_system(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
    let sys = &dash.sys;
    let cpu = sys.global_cpu_usage().round() as u64;
    let cores = sys.cpus().len();

    let disk_line = dash
        .disks
        .iter()
        .find(|d| d.mount_point().to_string_lossy() == "/")
        .or_else(|| dash.disks.iter().next())
        .map(|d| {
            let total = d.total_space();
            let used = total.saturating_sub(d.available_space());
            let pct = if total > 0 { used * 100 / total } else { 0 };
            format!(
                "Disk   : {}/{} GB {} {}%",
                cmds::to_gb(used),
                cmds::to_gb(total),
                cmds::bar(pct, 6),
                pct
            )
        })
        .unwrap_or_else(|| "Disk   : n/a".to_string());

    let lines = vec![
        Line::from(format!(
            "CPU    : {}% ({} cores) {}",
            cpu,
            cores,
            cmds::bar(cpu.min(100), 6)
        )),
        Line::from(disk_line),
        Line::from(format!(
            "Net    : ↓{:>7}/s ↑{:>7}/s",
            human_rate(dash.net_rate_rx),
            human_rate(dash.net_rate_tx)
        )),
        Line::from(format!("Procs  : {}", sys.processes().len())),
    ];
    let block = Block::bordered().title(" system ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_body(f: &mut ratatui::Frame, dash: &mut Dash, sheet: &Sheet, area: Rect) {
    // focus List: command list on top, little terminal underneath.
    // focus Term: the terminal fills the whole body.
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

    let term_area = chunks[chunks.len() - 1];
    dash.term_area = term_area;
    render_terminal(f, dash, term_area);
    if dash.focus == Focus::List {
        render_list(f, dash, sheet, chunks[0]);
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
        .highlight_style(fg(Color::Magenta));
    let mut state = ListState::default();
    if !entries.is_empty() {
        state.select(Some(dash.sel.min(entries.len() - 1)));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_terminal(f: &mut ratatui::Frame, dash: &mut Dash, area: Rect) {
    let focused = dash.focus == Focus::Term;
    let (scroll, scroll_max) = dash
        .session
        .as_ref()
        .map(|s| s.scroll_info())
        .unwrap_or((0, 0));
    let size = dash.session.as_ref().map(|s| s.size());
    let title = if focused {
        match size {
            Some((cols, rows)) => format!(" terminal [live] · {}x{} ", cols, rows),
            None => " terminal [live] ".to_string(),
        }
    } else if dash.session.is_some() {
        " terminal [running - ctrl+t] ".to_string()
    } else {
        " terminal ".to_string()
    };
    let border_style = if focused {
        fg(Color::Magenta)
    } else {
        Style::default()
    };
    let block = Block::bordered().title(title).border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // the exact content rect drives pty resize and mouse coordinate mapping
    dash.term_inner = inner;

    let lines: Vec<Line<'static>> = match dash.session.as_ref() {
        Some(s) => screen_lines(s.parser.screen()),
        None => vec![Line::from(Span::styled(
            "no live shell - press ctrl+t to open one",
            fg(Color::DarkGray),
        ))],
    };
    f.render_widget(Paragraph::new(lines), inner);

    // draw the shell cursor as a reversed cell when we are following output
    if let Some(s) = dash.session.as_ref() {
        let screen = s.parser.screen();
        if focused && !screen.hide_cursor() && scroll == 0 {
            let (cy, cx) = screen.cursor_position();
            if cy < inner.height && cx < inner.width {
                let cell = Rect {
                    x: inner.x + cx,
                    y: inner.y + cy,
                    width: 1,
                    height: 1,
                };
                f.render_widget(
                    Paragraph::new(Span::styled(
                        " ",
                        Style::default().add_modifier(Modifier::REVERSED),
                    )),
                    cell,
                );
            }
        }
    }

    if scroll > 0 {
        let label = format!(" scroll {}/{} · end to follow ", scroll, scroll_max);
        let y = inner.y + inner.height.saturating_sub(1);
        f.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::default().bg(Color::DarkGray).fg(Color::White),
            )),
            Rect {
                x: inner.x,
                y,
                width: inner.width.min(40),
                height: 1,
            },
        );
    }
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
    if !color::enabled() {
        return Style::default();
    }
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

/// foreground style that respects [interface] color = false
fn fg(color: Color) -> Style {
    if crate::color::enabled() {
        Style::default().fg(color)
    } else {
        Style::default()
    }
}

fn map_color(c: VtColor) -> Color {
    match c {
        VtColor::Default => Color::Reset,
        VtColor::Idx(i) => Color::Indexed(i),
        VtColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_rates_are_compact() {
        assert_eq!(human_rate(0.0), "0B");
        assert_eq!(human_rate(999.0), "999B");
        assert_eq!(human_rate(2048.0), "2.0K");
        assert_eq!(human_rate(1024.0 * 1024.0 * 3.0), "3.0M");
    }

    #[test]
    fn mouse_buttons_map_to_terminal_codes() {
        assert_eq!(mouse_button(MouseButton::Left), 0);
        assert_eq!(mouse_button(MouseButton::Middle), 1);
        assert_eq!(mouse_button(MouseButton::Right), 2);
    }
}

fn render_footer(f: &mut ratatui::Frame, dash: &Dash, area: Rect) {
    let content = if dash.focus == Focus::Term {
        " alt+↑↓/wheel scroll · ctrl+t/esc back to list · tab completes ".to_string()
    } else if dash.mode == Mode::Normal {
        " / search · ctrl+t terminal · ↑↓/jk move · enter run · q exit ".to_string()
    } else {
        format!("{} · enter run · esc clear ", dash.query)
    };
    let footer = Paragraph::new(Span::styled(content, fg(Color::DarkGray)));
    f.render_widget(footer, area);
}
