//! velella board — the keyboard cockpit. A pure client: it speaks the same
//! NDJSON API as the CLI and CI, holds no state beyond the view. lazygit
//! rules: tab moves panes, letters act, q leaves.

use crate::adapter::strip_ansi;
use crate::client;
use anyhow::Context;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::style::Color;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Tasks,
    Sessions,
}

struct App {
    focus: Focus,
    tasks: Vec<serde_json::Value>,
    sessions: Vec<serde_json::Value>,
    task_sel: usize,
    sess_sel: usize,
    watching: Option<String>,
    out_offset: u64,
    out_buf: String,
    last_fetch: Instant,
    status: String,
    daemon_error: Option<String>,
    /// Bottom-right pane shows the selected task's event log instead of the
    /// session stream.
    show_events: bool,
    events: Vec<serde_json::Value>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            focus: Focus::Tasks,
            tasks: Default::default(),
            sessions: Default::default(),
            task_sel: 0,
            sess_sel: 0,
            watching: None,
            out_offset: 0,
            out_buf: String::new(),
            last_fetch: Instant::now() - Duration::from_secs(1),
            status: "connected?".into(),
            daemon_error: None,
            show_events: false,
            events: Default::default(),
        }
    }
}

fn state_color(state: &str) -> Color {
    match state {
        "done" | "in_review" | "merged" => Color::Green,
        "waiting_permission" | "waiting_input" | "escalated" | "verifying" => Color::Yellow,
        "failed" => Color::Red,
        "running" => Color::Cyan,
        _ => Color::DarkGray,
    }
}

fn clamp(sel: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        sel.min(len - 1)
    }
}

impl App {
    fn poll_data(&mut self) {
        if self.last_fetch.elapsed() < Duration::from_millis(800) {
            return;
        }
        self.last_fetch = Instant::now();

        match client::call(serde_json::json!({ "cmd": "task.list" })) {
            Ok(r) => {
                self.tasks = r["tasks"].as_array().cloned().unwrap_or_default();
                self.task_sel = clamp(self.task_sel, self.tasks.len());
                self.daemon_error = None;
                self.status = "connected".into();
            }
            Err(e) => self.daemon_error = Some(e.to_string()),
        }

        match client::call(serde_json::json!({ "cmd": "session.list" })) {
            Ok(r) => {
                self.sessions = r["sessions"].as_array().cloned().unwrap_or_default();
                self.sess_sel = clamp(self.sess_sel, self.sessions.len());
                self.follow_selected();
            }
            Err(e) => self.daemon_error = Some(e.to_string()),
        }

        if self.show_events {
            if let Some(name) = self
                .tasks
                .get(self.task_sel)
                .and_then(|t| t["name"].as_str().map(str::to_string))
            {
                match client::call(serde_json::json!({ "cmd": "task.events", "name": name })) {
                    Ok(r) => self.events = r["events"].as_array().cloned().unwrap_or_default(),
                    Err(e) => self.daemon_error = Some(e.to_string()),
                }
            }
        }
    }

    /// Incrementally stream the selected session's output into the pane.
    fn follow_selected(&mut self) {
        let Some(id) = self
            .sessions
            .get(self.sess_sel)
            .and_then(|s| s["id"].as_str().map(str::to_string))
        else {
            self.watching = None;
            return;
        };
        if self.watching.as_deref() != Some(id.as_str()) {
            self.watching = Some(id);
            self.out_offset = 0;
            self.out_buf.clear();
        }
        let Some(watch) = self.watching.clone() else {
            return;
        };
        match client::call(serde_json::json!({
            "cmd": "session.output", "id": watch, "since": self.out_offset,
        })) {
            Ok(r) => {
                if r["from"].as_u64().unwrap_or(0) > self.out_offset {
                    self.out_buf.push_str("\n[older output truncated]\n");
                }
                self.out_buf.push_str(r["data"].as_str().unwrap_or(""));
                if self.out_buf.len() > 128 * 1024 {
                    let cut = self.out_buf.len() - 96 * 1024;
                    let boundary = self.out_buf[cut..].find('\n').map(|i| cut + i).unwrap_or(cut);
                    self.out_buf.drain(..boundary);
                }
                self.out_offset = r["end"].as_u64().unwrap_or(self.out_offset);
            }
            Err(e) => self.daemon_error = Some(e.to_string()),
        }
    }

    fn act(&mut self, code: KeyCode, shift: bool) -> anyhow::Result<()> {
        match (code, shift) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => anyhow::bail!(QUIT),
            (KeyCode::Tab, _) => {
                self.focus = match self.focus {
                    Focus::Tasks => Focus::Sessions,
                    Focus::Sessions => Focus::Tasks,
                };
            }
            (KeyCode::Down, false) | (KeyCode::Char('j'), false) => match self.focus {
                Focus::Tasks => self.task_sel = clamp(self.task_sel + 1, self.tasks.len()),
                Focus::Sessions => self.sess_sel = clamp(self.sess_sel + 1, self.sessions.len()),
            },
            (KeyCode::Up, false) | (KeyCode::Char('k'), false) => match self.focus {
                Focus::Tasks => self.task_sel = self.task_sel.saturating_sub(1),
                Focus::Sessions => self.sess_sel = self.sess_sel.saturating_sub(1),
            },
            (KeyCode::Char('k') | KeyCode::Char('K'), true) => self.kill_selected_session()?,
            (KeyCode::Char('v'), _) => self.verify_selected_task()?,
            (KeyCode::Char('a'), false) => self.approve_selected_session(),
            (KeyCode::Char('e'), false) => {
                self.show_events = !self.show_events;
                self.status = if self.show_events {
                    "showing selected task's event log".into()
                } else {
                    "showing session output".into()
                };
                self.last_fetch = Instant::now() - Duration::from_secs(1);
            }
            (KeyCode::Char('r'), _) => self.last_fetch = Instant::now() - Duration::from_secs(1),
            _ => {}
        }
        Ok(())
    }

    /// Answer the selected session's permission dialog (the board twin of
    /// the HTTP approve door).
    fn approve_selected_session(&mut self) {
        let Some(id) = self.sessions.get(self.sess_sel).and_then(|s| s["id"].as_str()) else {
            self.status = "no session selected".into();
            return;
        };
        match client::call(serde_json::json!({
            "cmd": "session.send", "id": id, "text": "1", "enter": true,
        })) {
            Ok(_) => self.status = format!("approved: {id}"),
            Err(e) => self.status = format!("approve failed: {e}"),
        }
        self.last_fetch = Instant::now() - Duration::from_secs(1);
    }

    fn kill_selected_session(&mut self) -> anyhow::Result<()> {
        let Some(id) = self.sessions.get(self.sess_sel).and_then(|s| s["id"].as_str()) else {
            self.status = "no session selected".into();
            return Ok(());
        };
        match client::call(serde_json::json!({ "cmd": "session.kill", "id": id })) {
            Ok(_) => self.status = format!("kill requested: {id}"),
            Err(e) => self.status = format!("kill failed: {e}"),
        }
        self.last_fetch = Instant::now() - Duration::from_secs(1);
        Ok(())
    }

    fn verify_selected_task(&mut self) -> anyhow::Result<()> {
        let Some(name) = self.tasks.get(self.task_sel).and_then(|t| t["name"].as_str()) else {
            self.status = "no task selected".into();
            return Ok(());
        };
        match client::call(serde_json::json!({ "cmd": "task.verify", "name": name })) {
            Ok(r) => {
                self.status = r["message"].as_str().unwrap_or("done").to_string();
                self.last_fetch = Instant::now() - Duration::from_secs(1);
            }
            Err(e) => self.status = format!("verify: {e}"),
        }
        Ok(())
    }
}

const QUIT: &str = "__quit__";

fn board_loop(terminal: &mut Terminal<impl ratatui::backend::Backend>) -> anyhow::Result<()> {
    let mut app = App::default();
    loop {
        app.poll_data();
        terminal.draw(|f| draw(f, &app))?;
        if event::poll(Duration::from_millis(300))? {
            if let Event::Key(k) = event::read()? {
                // Windows emits Release events too; act on presses only
                if k.kind == KeyEventKind::Press {
                    match app.act(k.code, k.modifiers.contains(KeyModifiers::SHIFT)) {
                        Ok(()) => {}
                        Err(e) if e.to_string() == QUIT => return Ok(()),
                        Err(e) => app.status = format!("{e}"),
                    }
                }
            }
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let rows = Layout::vertical([
        Constraint::Min(6),
        Constraint::Percentage(40),
        Constraint::Length(1),
    ])
    .split(f.area());
    let (tasks_area, lower, footer) = (rows[0], rows[1], rows[2]);
    let cols = Layout::horizontal([Constraint::Percentage(45), Constraint::Min(30)]).split(lower);
    let (sessions_area, output_area) = (cols[0], cols[1]);

    // tasks table
    let task_rows: Vec<Row> = app
        .tasks
        .iter()
        .map(|t| {
            let state = t["state"].as_str().unwrap_or("?");
            let pr = match t["pr_url"].as_str() {
                Some(_url) => "PR ✓".to_string(),
                None => String::new(),
            };
            Row::new(vec![
                Cell::from(t["name"].as_str().unwrap_or("?").to_string()),
                Cell::from(state.to_string()).style(Style::default().fg(state_color(state))),
                Cell::from(t["agent"].as_str().unwrap_or("?").to_string()),
                Cell::from(pr),
                Cell::from(short_id(t["id"].as_str().unwrap_or("?"))),
            ])
        })
        .collect();
    let task_table = Table::new(
        task_rows,
        [
            Constraint::Fill(3),
            Constraint::Length(18),
            Constraint::Length(14),
            Constraint::Length(5),
            Constraint::Fill(4),
        ],
    )
    .header(Row::new(["NAME", "STATE", "AGENT", "PR", "ID"]).bold())
    .block(Block::bordered().title(format!(" tasks ({}) ", app.tasks.len())))
    .row_highlight_style(Style::default().bg(Color::Rgb(60, 60, 70)));
    let mut ts = ratatui::widgets::TableState::default()
        .with_selected(if app.focus == Focus::Tasks { Some(app.task_sel) } else { None });
    f.render_stateful_widget(task_table, tasks_area, &mut ts);

    // sessions table
    let sess_rows: Vec<Row> = app
        .sessions
        .iter()
        .map(|s| {
            let state = s["state"].as_str().unwrap_or("?");
            Row::new(vec![
                Cell::from(short_id(s["id"].as_str().unwrap_or("?"))),
                Cell::from(state.to_string()).style(Style::default().fg(state_color(state))),
                Cell::from(s["program"].as_str().unwrap_or("?").to_string()),
            ])
        })
        .collect();
    let sess_table = Table::new(
        sess_rows,
        [Constraint::Length(16), Constraint::Length(19), Constraint::Fill(6)],
    )
    .header(Row::new(["SESSION", "STATE", "PROGRAM"]).bold())
    .block(Block::bordered().title(format!(" sessions ({}) ", app.sessions.len())))
    .row_highlight_style(Style::default().bg(Color::Rgb(60, 60, 70)));
    let mut ss = ratatui::widgets::TableState::default().with_selected(if app.focus == Focus::Sessions {
        Some(app.sess_sel)
    } else {
        None
    });
    f.render_stateful_widget(sess_table, sessions_area, &mut ss);

    // bottom-right: session stream, or the selected task's event audit log
    if app.show_events {
        let lines: Vec<String> = app
            .events
            .iter()
            .map(|e| {
                format!(
                    "#{:>3} {:<22} {}",
                    e["seq"].as_i64().unwrap_or(0),
                    e["kind"].as_str().unwrap_or("?").trim_matches('"'),
                    e["payload"]
                )
            })
            .collect();
        let ev = Paragraph::new(lines.join("\n")).block(Block::bordered().title(format!(
            " events {} ",
            app.tasks
                .get(app.task_sel)
                .and_then(|t| t["name"].as_str())
                .unwrap_or("-")
        )));
        f.render_widget(ev, output_area);
    } else {
        let watched = app
            .sessions
            .get(app.sess_sel)
            .and_then(|s| s["id"].as_str())
            .unwrap_or("-");
        let out = Paragraph::new(strip_ansi(&app.out_buf))
            .block(Block::bordered().title(format!(" output {watched} ")));
        f.render_widget(out, output_area);
    }

    // footer
    let status = match &app.daemon_error {
        Some(e) => format!("daemon unreachable: {e}"),
        None => app.status.clone(),
    };
    let footer_line = Paragraph::new(format!(
        "tab ↑↓ a:approve K:kill v:verify e:events r:refresh q:quit  |  {status}"
    ))
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer_line, footer);
}

fn short_id(id: &str) -> String {
    // UUIDv7 sessions spawned in the same second share their prefix, so a
    // prefix-only cut is ambiguous: keep head + tail ("ses_01a12ad6-6…397a").
    if id.len() <= 18 {
        return id.to_string();
    }
    format!("{}…{}", &id[..14], &id[id.len() - 4..])
}

pub fn run() -> anyhow::Result<()> {
    use ratatui::crossterm::tty::IsTty;
    if !io::stdout().is_tty() {
        anyhow::bail!("velella board needs a real terminal — run it inside Windows Terminal / your shell");
    }
    enable_raw_mode().context("velella board needs a real terminal (tty)")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    let res = board_loop(&mut terminal);
    // teardown no matter what — never strand the user's terminal
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = io::stdout().flush();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_app() -> App {
        let mut app = App::default();
        app.tasks = vec![
            serde_json::json!({
                "name": "upgrade-vite", "state": "running", "agent": "claude-code",
                "id": "tsk_01a12a53", "pr_url": null
            }),
            serde_json::json!({
                "name": "fix-flaky-test", "state": "in_review", "agent": "claude-code",
                "id": "tsk_01a12a54", "pr_url": "https://github.com/acme/repo/pull/7"
            }),
            serde_json::json!({
                "name": "migrate-db", "state": "waiting_permission", "agent": "claude-code",
                "id": "tsk_01a12a55", "pr_url": null
            }),
        ];
        app.sessions = vec![
            serde_json::json!({
                "id": "ses_01a12a53-9f98", "state": "running",
                "program": "claude --version", "idle_secs": 2, "exit": null
            }),
            serde_json::json!({
                "id": "ses_01a12a55-cd32", "state": "waiting_permission",
                "program": "cmd /c claude", "idle_secs": 9, "exit": null
            }),
        ];
        app.status = "connected".into();
        app
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..buf.area.height {
            let mut line = String::new();
            for x in 0..buf.area.width {
                line += buf[(x, y)].symbol();
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn renders_board_layout() {
        let app = sample_app();
        let frame = render(&app, 110, 24);
        println!("---- board @110x24 ----\n{frame}\n-----------------------");
        assert!(frame.contains("tasks (3)"), "tasks pane title");
        assert!(frame.contains("sessions (2)"), "sessions pane title");
        assert!(frame.contains("NAME"), "tasks header");
        assert!(frame.contains("waiting_permission"), "classified state shown");
        assert!(frame.contains("in_review"), "state shown");
        assert!(frame.contains("PR ✓"), "pr marker");
        assert!(frame.contains("approve"), "footer hint");
        assert!(frame.contains("output ses_01a12a53"), "output pane follows first session");
    }

    #[test]
    fn state_colors_have_semantics() {
        assert_eq!(state_color("in_review"), Color::Green);
        assert_eq!(state_color("waiting_permission"), Color::Yellow);
        assert_eq!(state_color("escalated"), Color::Yellow);
        assert_eq!(state_color("failed"), Color::Red);
        assert_eq!(state_color("running"), Color::Cyan);
        assert_eq!(state_color("queued"), Color::DarkGray);
    }

    #[test]
    fn selection_clamps() {
        assert_eq!(clamp(0, 0), 0);
        assert_eq!(clamp(5, 3), 2);
        assert_eq!(clamp(1, 3), 1);
    }

    #[test]
    fn short_id_trims_uuid() {
        assert_eq!(
            short_id("tsk_01a12a53-55a7-7473-a8ed"),
            "tsk_01a12a53-5…a8ed",
            "same-second spawns must stay distinguishable"
        );
        assert_eq!(short_id("weird"), "weird");
        assert_eq!(short_id("ses_01a12ad6-65da"), "ses_01a12ad6-65da");
    }
}
