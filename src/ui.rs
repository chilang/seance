use crate::app::{App, Mode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

const ALIVE_COLOR: Color = Color::Rgb(0, 255, 136);
const DEAD_COLOR: Color = Color::Rgb(255, 70, 70);
const ACCENT: Color = Color::Rgb(190, 120, 255);
const DIM: Color = Color::Rgb(100, 100, 110);
const CYAN: Color = Color::Rgb(80, 220, 255);
const GOLD: Color = Color::Rgb(255, 200, 60);
const SURFACE: Color = Color::Rgb(22, 22, 30);
const SURFACE_HL: Color = Color::Rgb(40, 38, 55);
const PINK: Color = Color::Rgb(255, 100, 180);
const MATCH_BG: Color = Color::Rgb(80, 60, 20);
const MATCH_FG: Color = Color::Rgb(255, 220, 80);
const PREVIEW_BG: Color = Color::Rgb(18, 18, 26);
const PROMPT_NUM: Color = Color::Rgb(120, 80, 200);

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

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(8),    // session list
            Constraint::Length(9), // detail panel
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_header(f, chunks[0], app);
    draw_session_list(f, chunks[1], app);
    draw_detail_panel(f, chunks[2], app);
    draw_status_bar(f, chunks[3], app);

    if app.mode == Mode::Preview {
        draw_preview_overlay(f, app);
    } else if app.mode == Mode::Usage {
        if let Some(analysis) = &app.usage_analysis {
            let area = f.area();
            let margin_h = 2u16;
            let margin_v = 2u16;
            let overlay = Rect {
                x: area.x + margin_h,
                y: area.y + margin_v,
                width: area.width.saturating_sub(margin_h * 2),
                height: area.height.saturating_sub(margin_v * 2),
            };
            f.render_widget(Clear, overlay);
            crate::usage::ui::render_session_analysis_with_state(
                f,
                analysis,
                overlay,
                &app.usage_ui_state,
            );
        }
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled("  🔮 ", Style::default()),
        Span::styled(
            "S",
            Style::default()
                .fg(Color::Rgb(200, 100, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "É",
            Style::default()
                .fg(Color::Rgb(190, 110, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "A",
            Style::default()
                .fg(Color::Rgb(180, 120, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "N",
            Style::default()
                .fg(Color::Rgb(170, 130, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "C",
            Style::default()
                .fg(Color::Rgb(160, 140, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "E",
            Style::default()
                .fg(Color::Rgb(150, 150, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(DIM)),
        Span::styled("Claude Code Session Necromancer", Style::default().fg(DIM)),
    ];

    if !app.loading_done {
        spans.push(Span::styled("  ◔ loading…", Style::default().fg(GOLD)));
    }

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(50, 48, 65)))
        .style(Style::default().bg(SURFACE));

    let header = Paragraph::new(Line::from(spans)).block(block);
    f.render_widget(header, area);
}

fn draw_session_list(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().style(Style::default().bg(SURFACE));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.sessions.is_empty() {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            "  No sessions found",
            Style::default().fg(DIM),
        )]));
        f.render_widget(empty, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let filtered = &app.cached_filtered;
    let total = filtered.len();
    let query = app.filter_text.to_lowercase();

    let mut lines: Vec<Line> = Vec::new();

    for (vi, &si) in filtered
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .take(visible_height)
    {
        let s = &app.sessions[si];
        let is_selected = vi == app.selected;
        let bg = if is_selected { SURFACE_HL } else { SURFACE };

        // Status dot
        let dot = if s.is_alive {
            Span::styled(" ● ", Style::default().fg(ALIVE_COLOR).bg(bg))
        } else {
            Span::styled(" ◌ ", Style::default().fg(DEAD_COLOR).bg(bg))
        };

        // Time
        let time_str = s.last_modified.format("%H:%M").to_string();
        let time = Span::styled(format!("{:<6}", time_str), Style::default().fg(GOLD).bg(bg));

        // Project name
        let pname: String = s.project_name.chars().take(28).collect();
        let pname_disp = obfuscate(&pname, app.privacy_mode);
        let project_spans =
            if !query.is_empty() && pname.to_lowercase().contains(&query) && !app.privacy_mode {
                highlight_match(&format!("{:<28}", pname), &query, CYAN, bg)
            } else {
                vec![Span::styled(
                    format!("{:<28}", pname_disp),
                    Style::default()
                        .fg(CYAN)
                        .bg(bg)
                        .add_modifier(if s.is_alive {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                )]
            };

        // Message count
        let count = Span::styled(
            format!("{:>3} msg ", s.user_msg_count),
            Style::default().fg(PINK).bg(bg),
        );

        // Size
        let size_str = if s.size_bytes > 1_048_576 {
            format!("{:>4}MB", s.size_bytes / 1_048_576)
        } else {
            format!("{:>4}KB", s.size_bytes / 1024)
        };
        let size = Span::styled(format!("{} ", size_str), Style::default().fg(DIM).bg(bg));

        // Tokens
        let tokens_str = format!("{:>5} ", format_tokens(s.total_tokens));
        let tokens = Span::styled(
            tokens_str,
            Style::default().fg(Color::Rgb(150, 150, 200)).bg(bg),
        );

        // Cache Hit Rate
        let cache_str = format!("{:>4} ", format_percentage(s.cache_hit_rate));
        let cache = Span::styled(
            cache_str,
            Style::default().fg(Color::Rgb(100, 200, 150)).bg(bg),
        );

        // First prompt with highlight
        let used = 3 + 6 + 28 + 8 + 6 + 5 + 7;
        let remaining = (inner.width as usize).saturating_sub(used + 1);
        let prompt_text = s.first_prompt.as_deref().unwrap_or("—");
        let prompt_oneline: String = prompt_text
            .split('\n')
            .next()
            .unwrap_or(prompt_text)
            .chars()
            .take(remaining)
            .collect();
        let prompt_fg = if is_selected {
            Color::White
        } else {
            Color::Rgb(160, 160, 170)
        };
        let prompt_oneline_disp = obfuscate(&prompt_oneline, app.privacy_mode);
        let prompt_spans = if !query.is_empty()
            && prompt_oneline.to_lowercase().contains(&query)
            && !app.privacy_mode
        {
            highlight_match(&prompt_oneline, &query, prompt_fg, bg)
        } else {
            vec![Span::styled(
                prompt_oneline_disp,
                Style::default().fg(prompt_fg).bg(bg),
            )]
        };

        // Pad
        let content_len: usize = used + prompt_oneline.chars().count();
        let pad_len = (inner.width as usize).saturating_sub(content_len);
        let pad = Span::styled(" ".repeat(pad_len), Style::default().bg(bg));

        let mut row_spans = vec![dot, time];
        row_spans.extend(project_spans);
        row_spans.push(count);
        row_spans.push(tokens);
        row_spans.push(cache);
        row_spans.push(size);
        row_spans.extend(prompt_spans);
        row_spans.push(pad);

        lines.push(Line::from(row_spans));
    }

    let list = Paragraph::new(lines);
    f.render_widget(list, inner);

    if total > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total).position(app.scroll_offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(ACCENT))
                .track_style(Style::default().fg(Color::Rgb(40, 38, 55))),
            area,
            &mut scrollbar_state,
        );
    }
}

fn draw_detail_panel(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(50, 48, 65)))
        .style(Style::default().bg(SURFACE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let filtered = &app.cached_filtered;
    let session = filtered
        .get(app.selected)
        .and_then(|&i| app.sessions.get(i));

    let Some(s) = session else {
        let empty = Paragraph::new(Span::styled(
            "  No session selected",
            Style::default().fg(DIM),
        ));
        f.render_widget(empty, inner);
        return;
    };

    let label = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::White);
    let dim_val = Style::default().fg(Color::Rgb(160, 160, 170));

    let id_short = obfuscate(
        if s.id.len() > 8 { &s.id[..8] } else { &s.id },
        app.privacy_mode,
    );
    let status_span = if s.is_alive {
        Span::styled(
            "ALIVE",
            Style::default()
                .fg(ALIVE_COLOR)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("DEAD", Style::default().fg(DEAD_COLOR))
    };

    let max_w = inner.width.saturating_sub(12) as usize;
    let first = s.first_prompt.as_deref().unwrap_or("—");
    let first_oneline: &str = first.split('\n').next().unwrap_or(first);
    let last = s.last_prompt.as_deref().unwrap_or("—");
    let last_oneline: &str = last.split('\n').next().unwrap_or(last);

    let cwd_disp = obfuscate(&trunc(&s.cwd, max_w.saturating_sub(30)), app.privacy_mode);
    let first_disp = obfuscate(&trunc(first_oneline, max_w), app.privacy_mode);
    let last_disp = obfuscate(&trunc(last_oneline, max_w), app.privacy_mode);

    let lines = vec![
        Line::from(vec![
            Span::styled("  ID ", label),
            Span::styled(id_short, value),
            Span::styled("  ", Style::default()),
            status_span,
            Span::styled("    ", Style::default()),
            Span::styled("Path ", label),
            Span::styled(cwd_disp, dim_val),
        ]),
        Line::from(vec![
            Span::styled("  ⮩  ", label),
            Span::styled(
                format!("\"{}\"", first_disp),
                Style::default().fg(Color::Rgb(200, 200, 210)),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ⮨  ", label),
            Span::styled(
                format!("\"{}\"", last_disp),
                Style::default().fg(Color::Rgb(200, 200, 210)),
            ),
        ]),
        Line::from(vec![Span::styled(
            format!(
                "  {} · {} · {} user messages · {} total prompts",
                s.last_modified.format("%Y-%m-%d %H:%M:%S"),
                format_size(s.size_bytes),
                s.user_msg_count,
                s.all_prompts.len(),
            ),
            Style::default().fg(DIM),
        )]),
    ];

    let detail = Paragraph::new(lines);
    f.render_widget(detail, inner);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let filtered = &app.cached_filtered;

    let key = |k: &str| {
        Span::styled(
            format!(" {k} "),
            Style::default()
                .fg(SURFACE)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
    };
    let desc = |d: &str| Span::styled(format!(" {d} "), Style::default().fg(DIM).bg(SURFACE));

    let spans = match app.mode {
        Mode::Filter => {
            let cursor = if (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 500)
                % 2
                == 0
            {
                "█"
            } else {
                " "
            };
            vec![
                key("/"),
                Span::styled(
                    format!(" {}{} ", app.filter_text, cursor),
                    Style::default().fg(Color::White).bg(Color::Rgb(50, 48, 65)),
                ),
                desc("Enter confirm · Esc clear"),
                Span::styled(
                    format!("  {} matches", filtered.len()),
                    Style::default().fg(GOLD),
                ),
            ]
        }
        Mode::Preview => {
            vec![
                key("↑↓"),
                desc("scroll"),
                key("Enter"),
                desc("ghostty"),
                key("Esc"),
                desc("close"),
            ]
        }
        Mode::Usage => {
            vec![
                key("b"),
                desc("bars"),
                key("↑↓"),
                desc("scroll"),
                key("PgUp/PgDn"),
                desc("page"),
                key("Esc"),
                desc("close"),
            ]
        }
        Mode::Normal => {
            let mut s = vec![
                key("↑↓"),
                desc("nav"),
                key("⏎"),
                desc("ghostty"),
                key("r"),
                desc("here"),
                key("p"),
                desc("preview"),
                key("x"),
                desc("privacy"),
                key("u"),
                desc("usage"),
                key("/"),
                desc("search"),
                key("a"),
                desc(if app.show_alive_only { "all" } else { "alive" }),
                key("d"),
                desc(if app.show_dead_only { "all" } else { "dead" }),
                key("y"),
                desc("copy"),
                key("q"),
                desc("quit"),
            ];

            if app.copied_flash > 0 {
                s.push(Span::styled(
                    "  ✓ copied!",
                    Style::default()
                        .fg(ALIVE_COLOR)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            let total = app.sessions.len();
            let shown = filtered.len();
            let alive = app.sessions.iter().filter(|s| s.is_alive).count();
            let session_id_str = if let Some(session) = app
                .cached_filtered
                .get(app.selected)
                .and_then(|&i| app.sessions.get(i))
            {
                format!(" · {}", obfuscate(&session.id, app.privacy_mode))
            } else {
                String::new()
            };

            s.push(Span::styled(
                format!("  {shown}/{total} · {alive} alive{}", session_id_str),
                Style::default().fg(DIM),
            ));
            s
        }
    };

    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE));
    f.render_widget(bar, area);
}

fn draw_preview_overlay(f: &mut Frame, app: &App) {
    let Some(session) = app
        .cached_filtered
        .get(app.selected)
        .and_then(|&i| app.sessions.get(i))
    else {
        return;
    };

    let area = f.area();
    // Overlay: centered with 2-cell margin on each side
    let margin_h = 2u16;
    let margin_v = 2u16;
    let overlay = Rect {
        x: area.x + margin_h,
        y: area.y + margin_v,
        width: area.width.saturating_sub(margin_h * 2),
        height: area.height.saturating_sub(margin_v * 2),
    };

    f.render_widget(Clear, overlay);

    let status = if session.is_alive { "●" } else { "◌" };
    let project_name_disp = obfuscate(&session.project_name, app.privacy_mode);
    let id_disp = obfuscate(&session.id[..8.min(session.id.len())], app.privacy_mode);
    let title = format!(
        " {status} {} · {} · {} prompts ",
        project_name_disp,
        id_disp,
        session.all_prompts.len()
    );

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(PREVIEW_BG));

    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let query = app.filter_text.to_lowercase();

    // Build all preview lines
    let mut lines: Vec<Line> = Vec::new();

    for (i, prompt) in session.all_prompts.iter().enumerate() {
        // Prompt header
        let num_color = if i == 0 {
            ALIVE_COLOR
        } else if i == session.all_prompts.len() - 1 {
            GOLD
        } else {
            PROMPT_NUM
        };

        let tag = if i == 0 {
            "FIRST"
        } else if i == session.all_prompts.len() - 1 {
            "LAST"
        } else {
            ""
        };

        let mut header_spans = vec![Span::styled(
            format!("  #{:<3}", i + 1),
            Style::default().fg(num_color).add_modifier(Modifier::BOLD),
        )];
        if !tag.is_empty() {
            header_spans.push(Span::styled(
                format!(" {tag}"),
                Style::default().fg(num_color),
            ));
        }
        lines.push(Line::from(header_spans));

        // Prompt body — wrap to inner width, show full text
        let wrap_width = inner.width.saturating_sub(6) as usize;
        for text_line in prompt.lines() {
            // Word-wrap each line
            for chunk in wrap_text(text_line, wrap_width) {
                let chunk_disp = obfuscate(&chunk, app.privacy_mode);
                if !query.is_empty() && chunk.to_lowercase().contains(&query) && !app.privacy_mode {
                    let mut spans = vec![Span::styled("    ", Style::default().bg(PREVIEW_BG))];
                    spans.extend(highlight_match(
                        &chunk,
                        &query,
                        Color::Rgb(200, 200, 210),
                        PREVIEW_BG,
                    ));
                    lines.push(Line::from(spans));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("    {chunk_disp}"),
                        Style::default()
                            .fg(Color::Rgb(200, 200, 210))
                            .bg(PREVIEW_BG),
                    )));
                }
            }
        }

        // Separator
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(wrap_width.min(60))),
            Style::default().fg(Color::Rgb(40, 38, 55)).bg(PREVIEW_BG),
        )));
    }

    let total_lines = lines.len();
    let visible = inner.height as usize;

    let para = Paragraph::new(lines)
        .scroll((app.preview_scroll as u16, 0))
        .style(Style::default().bg(PREVIEW_BG));
    f.render_widget(para, inner);

    // Scrollbar
    if total_lines > visible {
        let mut sb_state = ScrollbarState::new(total_lines).position(app.preview_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(ACCENT))
                .track_style(Style::default().fg(Color::Rgb(40, 38, 55))),
            overlay,
            &mut sb_state,
        );
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn highlight_match(text: &str, query: &str, base_fg: Color, bg: Color) -> Vec<Span<'static>> {
    let lower = text.to_lowercase();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0;

    for (start, _) in lower.match_indices(query) {
        let end = start + query.len();
        if start > last_end {
            spans.push(Span::styled(
                text[last_end..start].to_string(),
                Style::default().fg(base_fg).bg(bg),
            ));
        }
        spans.push(Span::styled(
            text[start..end].to_string(),
            Style::default()
                .fg(MATCH_FG)
                .bg(MATCH_BG)
                .add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }
    if last_end < text.len() {
        spans.push(Span::styled(
            text[last_end..].to_string(),
            Style::default().fg(base_fg).bg(bg),
        ));
    }
    spans
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut remaining = text;
    while remaining.len() > max_width {
        // Try to break at a space
        let break_at = remaining[..max_width].rfind(' ').unwrap_or(max_width);
        lines.push(remaining[..break_at].to_string());
        remaining = remaining[break_at..].trim_start();
    }
    if !remaining.is_empty() {
        lines.push(remaining.to_string());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn trunc(text: &str, max: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes > 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{}KB", bytes / 1024)
    }
}

pub fn obfuscate(text: &str, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                // simple deterministic "pixelation" using ascii blocks
                match (c as u32) % 3 {
                    0 => '▓',
                    1 => '▒',
                    _ => '░',
                }
            } else {
                c
            }
        })
        .collect()
}
