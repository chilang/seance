mod session;
mod ui;

pub mod app {
    use crate::session::Session;

    #[derive(PartialEq)]
    pub enum Mode {
        Normal,
        Filter,
        Preview,
    }

    pub struct App {
        pub sessions: Vec<Session>,
        pub selected: usize,
        pub scroll_offset: usize,
        pub filter_text: String,
        pub mode: Mode,
        pub show_alive_only: bool,
        pub show_dead_only: bool,
        pub preview_scroll: usize,
        pub loading_done: bool,
        pub copied_flash: u8, // countdown frames to show "copied!" feedback
    }

    impl App {
        pub fn new(sessions: Vec<Session>) -> Self {
            Self {
                sessions,
                selected: 0,
                scroll_offset: 0,
                filter_text: String::new(),
                mode: Mode::Normal,
                show_alive_only: false,
                show_dead_only: false,
                preview_scroll: 0,
                loading_done: false,
                copied_flash: 0,
            }
        }

        pub fn filtered_indices(&self) -> Vec<usize> {
            self.sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if self.show_alive_only && !s.is_alive {
                        return false;
                    }
                    if self.show_dead_only && s.is_alive {
                        return false;
                    }
                    if !self.filter_text.is_empty() {
                        let q = self.filter_text.to_lowercase();
                        let matches = s.project_name.to_lowercase().contains(&q)
                            || s.cwd.to_lowercase().contains(&q)
                            || s.id.starts_with(&q)
                            || s.all_prompts.iter().any(|p| p.to_lowercase().contains(&q));
                        if !matches {
                            return false;
                        }
                    }
                    true
                })
                .map(|(i, _)| i)
                .collect()
        }

        pub fn move_up(&mut self) {
            if self.selected > 0 {
                self.selected -= 1;
                if self.selected < self.scroll_offset {
                    self.scroll_offset = self.selected;
                }
            }
        }

        pub fn move_down(&mut self, visible_height: usize) {
            let total = self.filtered_indices().len();
            if self.selected + 1 < total {
                self.selected += 1;
                if self.selected >= self.scroll_offset + visible_height {
                    self.scroll_offset = self.selected.saturating_sub(visible_height - 1);
                }
            }
        }

        pub fn page_up(&mut self, visible_height: usize) {
            self.selected = self.selected.saturating_sub(visible_height);
            self.scroll_offset = self.scroll_offset.saturating_sub(visible_height);
        }

        pub fn page_down(&mut self, visible_height: usize) {
            let total = self.filtered_indices().len();
            self.selected = (self.selected + visible_height).min(total.saturating_sub(1));
            let max_offset = total.saturating_sub(visible_height);
            self.scroll_offset = (self.scroll_offset + visible_height).min(max_offset);
        }

        pub fn selected_session(&self) -> Option<&Session> {
            let filtered = self.filtered_indices();
            filtered
                .get(self.selected)
                .and_then(|&i| self.sessions.get(i))
        }

        pub fn clamp_selection(&mut self) {
            let total = self.filtered_indices().len();
            if total == 0 {
                self.selected = 0;
                self.scroll_offset = 0;
            } else if self.selected >= total {
                self.selected = total - 1;
            }
        }

        /// Merge more sessions (from background load), maintaining sort order.
        pub fn append_sessions(&mut self, mut more: Vec<Session>) {
            self.sessions.append(&mut more);
            self.sessions
                .sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        }
    }
}

use app::{App, Mode};
use color_eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    color_eyre::install()?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "list" | "ls" => return print_session_list(),
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => {}
        }
    }

    // Incremental load: parse 60 newest files first (covers ~last week)
    eprintln!("🔮 Summoning sessions...");
    let (initial, remaining, alive_ids) = session::discover_sessions_incremental(60);
    let initial_count = initial.len();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(initial);

    // Background thread to parse remaining session files
    let (tx, rx) = mpsc::channel();
    if remaining.is_empty() {
        app.loading_done = true;
    } else {
        thread::spawn(move || {
            let more = session::load_remaining(&remaining, &alive_ids);
            let _ = tx.send(more);
        });
    }

    let result = run_app(&mut terminal, &mut app, rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    if initial_count > 0 && !app.loading_done {
        eprintln!(
            "   Loaded {} sessions (more loading in background)",
            initial_count
        );
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    bg_rx: mpsc::Receiver<Vec<session::Session>>,
) -> Result<()> {
    loop {
        // Check for background load completion
        if !app.loading_done {
            if let Ok(more) = bg_rx.try_recv() {
                app.append_sessions(more);
                app.loading_done = true;
                app.clamp_selection();
            }
        }

        // Tick down copy flash
        if app.copied_flash > 0 {
            app.copied_flash -= 1;
        }

        let visible_height = terminal.size()?.height.saturating_sub(13) as usize;
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll with timeout so we can refresh for background loads / flash
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            match app.mode {
                Mode::Filter => match key.code {
                    KeyCode::Esc => {
                        app.mode = Mode::Normal;
                        app.filter_text.clear();
                        app.clamp_selection();
                    }
                    KeyCode::Enter => {
                        app.mode = Mode::Normal;
                        app.clamp_selection();
                    }
                    KeyCode::Backspace => {
                        app.filter_text.pop();
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char(c) => {
                        app.filter_text.push(c);
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    _ => {}
                },
                Mode::Preview => {
                    let preview_lines = app
                        .selected_session()
                        .map(|s| preview_line_count(s))
                        .unwrap_or(0);
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('p') | KeyCode::Char(' ') | KeyCode::Char('q') => {
                            app.mode = Mode::Normal;
                            app.preview_scroll = 0;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.preview_scroll = app.preview_scroll.saturating_add(1).min(preview_lines.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(20);
                        }
                        KeyCode::PageDown => {
                            app.preview_scroll = app.preview_scroll.saturating_add(20).min(preview_lines.saturating_sub(1));
                        }
                        KeyCode::Home | KeyCode::Char('g') => {
                            app.preview_scroll = 0;
                        }
                        KeyCode::End | KeyCode::Char('G') => {
                            app.preview_scroll = preview_lines.saturating_sub(1);
                        }
                        KeyCode::Enter => {
                            if let Some(s) = app.selected_session().cloned() {
                                app.mode = Mode::Normal;
                                disable_raw_mode()?;
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                terminal.show_cursor()?;
                                launch_in_ghostty(&s);
                                enable_raw_mode()?;
                                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                                terminal.hide_cursor()?;
                                terminal.clear()?;
                            }
                        }
                        _ => {}
                    }
                }
                Mode::Normal => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(visible_height),
                    KeyCode::PageUp => app.page_up(visible_height),
                    KeyCode::PageDown => app.page_down(visible_height),
                    KeyCode::Home | KeyCode::Char('g') => {
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::End | KeyCode::Char('G') => {
                        let total = app.filtered_indices().len();
                        app.selected = total.saturating_sub(1);
                        app.scroll_offset = total.saturating_sub(visible_height);
                    }
                    KeyCode::Char('/') => {
                        app.mode = Mode::Filter;
                    }
                    KeyCode::Char('a') => {
                        app.show_alive_only = !app.show_alive_only;
                        app.show_dead_only = false;
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char('d') => {
                        app.show_dead_only = !app.show_dead_only;
                        app.show_alive_only = false;
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char('p') | KeyCode::Char(' ') => {
                        if app.selected_session().is_some() {
                            app.mode = Mode::Preview;
                            app.preview_scroll = 0;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(s) = app.selected_session().cloned() {
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            launch_in_ghostty(&s);
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.hide_cursor()?;
                            terminal.clear()?;
                        }
                    }
                    KeyCode::Char('r') => {
                        if let Some(s) = app.selected_session().cloned() {
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            resume_in_current_terminal(&s);
                            return Ok(());
                        }
                    }
                    KeyCode::Char('y') => {
                        if let Some(s) = app.selected_session() {
                            let cmd = format!("cd {} && claude -r {}", s.cwd, s.id);
                            let _ = Command::new("pbcopy")
                                .stdin(std::process::Stdio::piped())
                                .spawn()
                                .and_then(|mut child| {
                                    use std::io::Write;
                                    if let Some(ref mut stdin) = child.stdin {
                                        stdin.write_all(cmd.as_bytes())?;
                                    }
                                    child.wait()
                                });
                            app.copied_flash = 15; // ~1.5s at 100ms poll
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

fn preview_line_count(session: &session::Session) -> usize {
    // Each prompt takes ~3 lines (header + text + blank)
    session.all_prompts.len() * 3 + 2
}

fn launch_in_ghostty(session: &session::Session) {
    let cmd = format!(
        "cd {} && claude -r {}",
        shell_escape(&session.cwd),
        session.id
    );

    let script = format!(
        r#"
        tell application "Ghostty"
            activate
        end tell
        delay 0.2
        tell application "System Events" to tell process "Ghostty"
            keystroke "t" using command down
            delay 0.4
            keystroke "{}"
            key code 36
        end tell
        "#,
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let result = Command::new("osascript").arg("-e").arg(&script).output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                eprintln!(
                    "AppleScript failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                eprintln!("Manual command: {cmd}");
            }
        }
        Err(e) => {
            eprintln!("Failed to run osascript: {e}");
            eprintln!("Manual command: {cmd}");
        }
    }
}

fn resume_in_current_terminal(session: &session::Session) {
    println!(
        "\n🔮 Resuming session {} in {}...\n",
        &session.id[..8],
        session.project_name
    );

    let status = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "cd {} && exec claude -r {}",
            shell_escape(&session.cwd),
            session.id
        ))
        .status();

    match status {
        Ok(s) => {
            if !s.success() {
                eprintln!("claude exited with: {s}");
            }
        }
        Err(e) => eprintln!("Failed to launch claude: {e}"),
    }
}

fn shell_escape(s: &str) -> String {
    if s.contains(' ') || s.contains('\'') || s.contains('"') || s.contains('$') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

fn print_session_list() -> Result<()> {
    let sessions = session::discover_sessions();
    for s in &sessions {
        let status = if s.is_alive { "●" } else { "◌" };
        let time = s.last_modified.format("%m-%d %H:%M");
        let prompt = s.first_prompt.as_deref().unwrap_or("—");
        let prompt_trunc: String = prompt.chars().take(60).collect();
        println!(
            "{} {} {:<28} {:>3}msg  {}",
            status, time, s.project_name, s.user_msg_count, prompt_trunc
        );
    }
    Ok(())
}

fn print_help() {
    println!(
        r#"🔮 Séance — Claude Code Session Necromancer

USAGE:
    seance          Launch interactive TUI
    seance list     Print sessions as plain text
    seance ls       Alias for list

TUI KEYS:
    ↑/↓, j/k       Navigate sessions
    Enter           Resume session in new Ghostty tab
    r               Resume session in current terminal
    y               Copy resume command to clipboard
    p, Space        Preview all user prompts
    /               Search/filter sessions (searches all prompts)
    a               Toggle: show alive sessions only
    d               Toggle: show dead sessions only
    g/G             Jump to top/bottom
    PgUp/PgDn       Page scroll
    q, Esc          Quit

PREVIEW KEYS:
    ↑/↓, j/k       Scroll prompts
    Enter           Resume this session in Ghostty
    Esc, p, Space   Close preview
    g/G             Top/bottom"#
    );
}
