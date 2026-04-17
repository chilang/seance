mod session;
mod ui;
mod usage;

pub mod app {
    use crate::session::Session;

    #[derive(PartialEq)]
    pub enum Mode {
        Normal,
        Filter,
        Preview,
        Usage,
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
        pub cached_filtered: Vec<usize>,
        pub usage_analysis: Option<crate::usage::models::SessionAnalysis>,
        pub usage_ui_state: crate::usage::ui::AppState,
        pub privacy_mode: bool,
        filter_dirty: bool,
    }

    impl App {
        pub fn new(sessions: Vec<Session>) -> Self {
            let mut app = Self {
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
                cached_filtered: Vec::new(),
                usage_analysis: None,
                usage_ui_state: crate::usage::ui::AppState {
                    show_bars: false,
                    scroll_offset: 0,
                    selected_turn: 0,
                    privacy_mode: false,
                },
                privacy_mode: false,
                filter_dirty: true,
            };
            app.recompute_filter();
            app
        }

        fn recompute_filter(&mut self) {
            let q = self.filter_text.to_lowercase();
            self.cached_filtered = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if self.show_alive_only && !s.is_alive {
                        return false;
                    }
                    if self.show_dead_only && s.is_alive {
                        return false;
                    }
                    if !q.is_empty() {
                        let matches = s.project_name.to_lowercase().contains(&q)
                            || s.cwd.to_lowercase().contains(&q)
                            || s.id.starts_with(&q)
                            || s.all_prompts.iter().any(|p| p.to_lowercase().contains(&q))
                            || s.tool_keywords.iter().any(|k| k.contains(&q));
                        if !matches {
                            return false;
                        }
                    }
                    true
                })
                .map(|(i, _)| i)
                .collect();
            self.filter_dirty = false;
        }

        pub fn ensure_filter(&mut self) {
            if self.filter_dirty {
                self.recompute_filter();
            }
        }

        pub fn invalidate_filter(&mut self) {
            self.filter_dirty = true;
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
            self.ensure_filter();
            let total = self.cached_filtered.len();
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
            self.ensure_filter();
            let total = self.cached_filtered.len();
            self.selected = (self.selected + visible_height).min(total.saturating_sub(1));
            let max_offset = total.saturating_sub(visible_height);
            self.scroll_offset = (self.scroll_offset + visible_height).min(max_offset);
        }

        pub fn selected_session(&mut self) -> Option<&Session> {
            self.ensure_filter();
            self.cached_filtered
                .get(self.selected)
                .and_then(|&i| self.sessions.get(i))
        }

        pub fn clamp_selection(&mut self) {
            self.ensure_filter();
            let total = self.cached_filtered.len();
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
            self.invalidate_filter();
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
        app.ensure_filter();
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
                        app.invalidate_filter();
                        app.clamp_selection();
                    }
                    KeyCode::Enter => {
                        app.mode = Mode::Normal;
                        app.clamp_selection();
                    }
                    KeyCode::Backspace => {
                        app.filter_text.pop();
                        app.invalidate_filter();
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char(c) => {
                        app.filter_text.push(c);
                        app.invalidate_filter();
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    _ => {}
                },
                Mode::Usage => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('u') | KeyCode::Char(' ') => {
                        app.mode = Mode::Normal;
                    }
                    KeyCode::Char('b') => {
                        app.usage_ui_state.show_bars = !app.usage_ui_state.show_bars;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.usage_ui_state.selected_turn > 0 {
                            app.usage_ui_state.selected_turn -= 1;
                        }
                        if app.usage_ui_state.selected_turn < app.usage_ui_state.scroll_offset {
                            app.usage_ui_state.scroll_offset = app.usage_ui_state.selected_turn;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(analysis) = &app.usage_analysis {
                            let total_turns = analysis.turns.len();
                            if app.usage_ui_state.selected_turn + 1 < total_turns {
                                app.usage_ui_state.selected_turn += 1;
                            }
                            let visible = (visible_height + 13).saturating_sub(36).max(8);
                            if app.usage_ui_state.selected_turn
                                >= app.usage_ui_state.scroll_offset + visible
                            {
                                app.usage_ui_state.scroll_offset =
                                    app.usage_ui_state.selected_turn.saturating_sub(visible - 1);
                            }
                        }
                    }
                    KeyCode::PageUp => {
                        let visible = (visible_height + 13).saturating_sub(36).max(8);
                        app.usage_ui_state.selected_turn =
                            app.usage_ui_state.selected_turn.saturating_sub(visible);
                        app.usage_ui_state.scroll_offset =
                            app.usage_ui_state.scroll_offset.saturating_sub(visible);
                    }
                    KeyCode::PageDown => {
                        if let Some(analysis) = &app.usage_analysis {
                            let total_turns = analysis.turns.len();
                            let visible = (visible_height + 13).saturating_sub(36).max(8);
                            app.usage_ui_state.selected_turn = (app.usage_ui_state.selected_turn
                                + visible)
                                .min(total_turns.saturating_sub(1));
                            app.usage_ui_state.scroll_offset = (app.usage_ui_state.scroll_offset
                                + visible)
                                .min(total_turns.saturating_sub(visible));
                        }
                    }
                    KeyCode::Char(']') | KeyCode::Char('L') | KeyCode::Char('n') => {
                        if let Some(analysis) = &app.usage_analysis {
                            // Find next session group start
                            let current_idx = app.usage_ui_state.selected_turn;
                            let mut next_idx = current_idx;
                            let mut current_turn_count = 0;

                            for session in &analysis.session_stats.sessions {
                                current_turn_count += session.turn_count;
                                if current_turn_count > current_idx {
                                    if current_turn_count < analysis.turns.len() {
                                        next_idx = current_turn_count;
                                    } else {
                                        next_idx = analysis.turns.len().saturating_sub(1);
                                    }
                                    break;
                                }
                            }

                            app.usage_ui_state.selected_turn = next_idx;
                            let visible = (visible_height + 13).saturating_sub(36).max(8);
                            if app.usage_ui_state.selected_turn
                                >= app.usage_ui_state.scroll_offset + visible
                            {
                                app.usage_ui_state.scroll_offset =
                                    app.usage_ui_state.selected_turn.saturating_sub(visible - 1);
                            } else if app.usage_ui_state.selected_turn
                                < app.usage_ui_state.scroll_offset
                            {
                                app.usage_ui_state.scroll_offset = app.usage_ui_state.selected_turn;
                            }
                        }
                    }
                    KeyCode::Char('[') | KeyCode::Char('H') | KeyCode::Char('N') => {
                        if let Some(analysis) = &app.usage_analysis {
                            // Find previous session group start
                            let current_idx = app.usage_ui_state.selected_turn;
                            let mut prev_idx = 0;
                            let mut current_turn_count = 0;

                            for session in &analysis.session_stats.sessions {
                                if current_turn_count > 0 && current_turn_count < current_idx {
                                    prev_idx = current_turn_count;
                                }
                                current_turn_count += session.turn_count;
                                if current_turn_count >= current_idx {
                                    break;
                                }
                            }

                            app.usage_ui_state.selected_turn = prev_idx;
                            let visible = (visible_height + 13).saturating_sub(36).max(8);
                            if app.usage_ui_state.selected_turn < app.usage_ui_state.scroll_offset {
                                app.usage_ui_state.scroll_offset = app.usage_ui_state.selected_turn;
                            } else if app.usage_ui_state.selected_turn
                                >= app.usage_ui_state.scroll_offset + visible
                            {
                                app.usage_ui_state.scroll_offset =
                                    app.usage_ui_state.selected_turn.saturating_sub(visible - 1);
                            }
                        }
                    }
                    KeyCode::Home | KeyCode::Char('g') => {
                        app.usage_ui_state.selected_turn = 0;
                        app.usage_ui_state.scroll_offset = 0;
                    }
                    KeyCode::End | KeyCode::Char('G') => {
                        if let Some(analysis) = &app.usage_analysis {
                            let total_turns = analysis.turns.len();
                            let visible = (visible_height + 13).saturating_sub(36).max(8);
                            app.usage_ui_state.selected_turn = total_turns.saturating_sub(1);
                            app.usage_ui_state.scroll_offset = total_turns.saturating_sub(visible);
                        }
                    }
                    _ => {}
                },
                Mode::Preview => {
                    let preview_lines = app
                        .selected_session()
                        .map(|s| preview_line_count(s))
                        .unwrap_or(0);
                    match key.code {
                        KeyCode::Esc
                        | KeyCode::Char('p')
                        | KeyCode::Char(' ')
                        | KeyCode::Char('q') => {
                            app.mode = Mode::Normal;
                            app.preview_scroll = 0;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.preview_scroll = app
                                .preview_scroll
                                .saturating_add(1)
                                .min(preview_lines.saturating_sub(1));
                        }
                        KeyCode::PageUp => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(20);
                        }
                        KeyCode::PageDown => {
                            app.preview_scroll = app
                                .preview_scroll
                                .saturating_add(20)
                                .min(preview_lines.saturating_sub(1));
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
                        let total = app.cached_filtered.len();
                        app.selected = total.saturating_sub(1);
                        app.scroll_offset = total.saturating_sub(visible_height);
                    }
                    KeyCode::Char('/') => {
                        app.mode = Mode::Filter;
                    }
                    KeyCode::Char('a') => {
                        app.show_alive_only = !app.show_alive_only;
                        app.show_dead_only = false;
                        app.invalidate_filter();
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char('d') => {
                        app.show_dead_only = !app.show_dead_only;
                        app.show_alive_only = false;
                        app.invalidate_filter();
                        app.selected = 0;
                        app.scroll_offset = 0;
                    }
                    KeyCode::Char('x') => {
                        app.privacy_mode = !app.privacy_mode;
                        app.usage_ui_state.privacy_mode = app.privacy_mode;
                    }
                    KeyCode::Char('p') | KeyCode::Char(' ') => {
                        if app.selected_session().is_some() {
                            app.mode = Mode::Preview;
                            app.preview_scroll = 0;
                        }
                    }
                    KeyCode::Char('u') => {
                        if let Some(s) = app.selected_session() {
                            let path = format!(
                                "{}/.claude/projects/{}/{}.jsonl",
                                dirs::home_dir().unwrap().to_string_lossy(),
                                s.cwd.replace('/', "-"),
                                s.id
                            );

                            let parsed = crate::usage::parsers::parse_jsonl_file(
                                std::path::Path::new(&path),
                                true,
                            )
                            .unwrap_or_default();

                            if !parsed.is_empty() {
                                if let Ok(analysis) = crate::usage::analyzers::analyze_session(
                                    parsed,
                                    crate::usage::models::SessionId(s.id.clone()),
                                    crate::usage::models::CostMode::Auto,
                                ) {
                                    app.usage_analysis = Some(analysis);
                                    app.usage_ui_state.selected_turn = 0;
                                    app.usage_ui_state.scroll_offset = 0;
                                    app.mode = Mode::Usage;
                                }
                            }
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

fn format_tokens(t: u64) -> String {
    if t >= 1_000_000 {
        format!("{:.1}M", t as f64 / 1_000_000.0)
    } else if t >= 1_000 {
        format!("{:.0}k", t as f64 / 1_000.0)
    } else {
        format!("{}", t)
    }
}

fn format_percentage(pct: f64) -> String {
    format!("{:.0}%", pct * 100.0)
}

fn print_session_list() -> Result<()> {
    let sessions = session::discover_sessions();
    for s in &sessions {
        let status = if s.is_alive { "●" } else { "◌" };
        let time = s.last_modified.format("%m-%d %H:%M");
        let prompt = s.first_prompt.as_deref().unwrap_or("—");
        let prompt_trunc: String = prompt.chars().take(60).collect();
        let tokens_str = format_tokens(s.total_tokens);
        let cache_str = format_percentage(s.cache_hit_rate);
        println!(
            "{} {} {:<28} {:>3}msg {:>5} {:>4}  {}",
            status, time, s.project_name, s.user_msg_count, tokens_str, cache_str, prompt_trunc
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
    x               Toggle privacy mode (obscure paths and titles)
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
