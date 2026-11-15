// dash: live status header (the mova banner, but alive) + commands + ports

use crate::cmds::{self, Outcome};
use crate::sheet::Sheet;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, ListState, Paragraph};
use ratatui::DefaultTerminal;
use std::io;
use std::time::Duration;
use sysinfo::System;

struct Dash {
    sys: System,
    sel: usize,
}

pub fn run(sheet: &Sheet) -> Outcome {
    match run_inner(sheet) {
        Ok(true) => Outcome::Exit(0),
        Ok(false) => Outcome::Done(0),
        Err(e) => {
            eprintln!("{}dash: {}{}", cmds::RED, e, cmds::RESET);
            Outcome::Done(1)
        }
    }
}

fn run_inner(sheet: &Sheet) -> io::Result<bool> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, sheet);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, sheet: &Sheet) -> io::Result<bool> {
    let mut dash = Dash {
        sys: System::new(),
        sel: 0,
    };
    dash.sys.refresh_memory();

    loop {
        terminal.draw(|f| ui(f, &mut dash, sheet))?;

        if event::poll(Duration::from_secs(1))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(false),
                    KeyCode::Up | KeyCode::Char('k') => {
                        dash.sel = dash.sel.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        dash.sel = (dash.sel + 1).min(sheet.cmds.len().saturating_sub(1));
                    }
                    KeyCode::Enter => {
                        if let Some(entry) = sheet.cmds.get(dash.sel) {
                            let name = entry.name.clone();
                            if run_in_terminal(terminal, &name, sheet) {
                                return Ok(true); // e inside dash = exit ava
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            // tick: refresh the live parts
            dash.sys.refresh_memory();
        }
    }
}

/// suspend the tui, run the command with normal output, wait, resume.
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
            Constraint::Min(3),     // body: commands + ports
            Constraint::Length(1),  // footer
        ])
        .split(f.area());

    render_header(f, dash, chunks[0]);
    render_body(f, dash, sheet, chunks[1]);
    render_footer(f, chunks[2]);
}

fn render_header(f: &mut ratatui::Frame, dash: &Dash, area: ratatui::layout::Rect) {
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
                    .add_modifier(ratatui::style::Modifier::DIM),
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

    // gauge overlays the bottom line of the header block
    let gauge_area = ratatui::layout::Rect {
        x: area.x + 1,
        y: area.y + area.height - 2,
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

fn render_body(f: &mut ratatui::Frame, dash: &mut Dash, sheet: &Sheet, area: ratatui::layout::Rect) {
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = sheet
        .cmds
        .iter()
        .map(|c| ListItem::new(Line::from(format!("{:<10} {}", c.name, c.desc))))
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(" commands "))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Magenta));
    let mut state = ListState::default();
    state.select(Some(dash.sel.min(sheet.cmds.len().saturating_sub(1))));
    f.render_stateful_widget(list, body[0], &mut state);

    let ports: Vec<Line> = cmds::ports_list()
        .into_iter()
        .map(|(p, local)| Line::from(format!("{:<8}{}", p, local)))
        .collect();
    let ports = Paragraph::new(ports).block(Block::bordered().title(" listening ports "));
    f.render_widget(ports, body[1]);
}

fn render_footer(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let footer = Paragraph::new(Span::styled(
        " ↑↓/jk move · enter run · q back to shell ",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(footer, area);
}
