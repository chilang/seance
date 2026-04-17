pub struct AppState {
    pub show_bars: bool,
    pub scroll_offset: usize,
    pub selected_turn: usize,
    pub privacy_mode: bool,
    pub detailed_view: bool,
}
use crate::usage::analyzers::{format_currency, format_duration, format_number, format_percentage};
use crate::usage::models::{ModelRates, SessionAnalysis};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, Tabs},
    Frame,
};

/// Color scheme matching the firestore explorer
const COLORS: Colors = Colors {
    cache_write: Color::Rgb(215, 170, 93), // Gold: #d7aa5d
    cache_read: Color::Rgb(46, 204, 113),  // Green: #2ecc71
    input: Color::Rgb(91, 141, 239),       // Blue: #5b8def
    output: Color::Rgb(230, 126, 34),      // Orange: #e67e22
    cost: Color::Rgb(231, 76, 60),         // Red: #e74c3c
    text: Color::White,
    muted: Color::Gray,
    accent: Color::Cyan,
};

struct Colors {
    cache_write: Color,
    cache_read: Color,
    input: Color,
    output: Color,
    cost: Color,
    text: Color,
    muted: Color,
    accent: Color,
}

/// Render the main session analysis UI
pub fn render_session_analysis(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    render_session_analysis_with_bars(f, analysis, area, false);
}

/// Render the main session analysis UI with optional bar display
pub fn render_session_analysis_with_bars(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    show_bars: bool,
) {
    render_session_analysis_with_state(
        f,
        analysis,
        area,
        &AppState {
            show_bars,
            scroll_offset: 0,
            selected_turn: 0,
            privacy_mode: false,
            detailed_view: false,
        },
    );
}

/// Render the main session analysis UI with full app state (navigation, scroll, etc)
pub fn render_session_analysis_with_state(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    app_state: &AppState,
) {
    // Layout: Title -> Stats Grid -> Timeline -> Help
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(14), // Stats grid (was 12)
            Constraint::Min(10),    // Timeline
            Constraint::Length(1),  // Help bar
        ])
        .split(area);

    render_title(f, analysis, main_layout[0], app_state);
    render_stats_grid(f, analysis, main_layout[1]);
    render_timeline_with_state(f, analysis, main_layout[2], app_state);
    render_help_bar_with_state(f, app_state, main_layout[3]);
}

/// Render the title section with session ID and metadata
fn render_title(f: &mut Frame, analysis: &SessionAnalysis, area: Rect, app_state: &AppState) {
    let session_id_disp = crate::ui::obfuscate(&analysis.session_id.0, app_state.privacy_mode);

    let time_range =
        if let (Some(first), Some(last)) = (analysis.turns.first(), analysis.turns.last()) {
            format!(
                "{} to {}",
                first.timestamp.format("%Y-%m-%d %H:%M:%S"),
                last.timestamp.format("%H:%M:%S")
            )
        } else {
            "Unknown time".to_string()
        };

    let title_text = format!(" Session: {} | {} ", session_id_disp, time_range);

    let project_info = analysis
        .project
        .as_ref()
        .map(|p| {
            format!(
                "Project: {}",
                crate::ui::obfuscate(p, app_state.privacy_mode)
            )
        })
        .unwrap_or_else(|| "Project: unknown".to_string());

    let block = Block::default()
        .title(title_text)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLORS.accent));

    // Format models with pricing info
    let models_text = if analysis.models_used.is_empty() {
        "unknown".to_string()
    } else {
        analysis
            .models_used
            .iter()
            .map(|m| {
                let rates = ModelRates::for_model(&m.0);
                format!("{} (${}/${})", m.0, rates.input, rates.output)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };

    let text = Text::from(vec![
        Line::from(project_info),
        Line::from(format!("Models: {}", models_text)),
    ]);

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Render the stats grid with 9 stat cards
fn render_stats_grid(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    let grid = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(4), // Token breakdown
        ])
        .split(area);

    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(grid[0]);

    let bottom_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(grid[1]);

    // Row 1 stats
    render_stat_card(
        f,
        "TOTAL TURNS",
        &format_number(analysis.turn_count as u64),
        "Turn count",
        top_row[0],
        COLORS.text,
    );

    render_stat_card(
        f,
        "TOTAL TOKENS",
        &format_number(analysis.aggregates.total()),
        "All categories combined",
        top_row[1],
        COLORS.text,
    );

    // TOTAL COST with cost source indicator
    let cost_description = match analysis.cost_source {
        crate::usage::models::CostSource::Api => "Conversation spend (API)",
        crate::usage::models::CostSource::Calculated => "Conversation spend (Calculated)",
        crate::usage::models::CostSource::Mixed => "Conversation spend (Mixed)",
    };

    render_stat_card(
        f,
        "TOTAL COST",
        &format_currency(analysis.total_cost),
        cost_description,
        top_row[2],
        COLORS.cost,
    );

    // Row 2 stats
    render_stat_card(
        f,
        "CACHE HIT RATE",
        &format_percentage(analysis.cache_hit_rate),
        "Prompt cache read ratio",
        bottom_row[0],
        COLORS.cache_read,
    );

    render_stat_card(
        f,
        "CACHE WRITE RATE",
        &format_percentage(analysis.cache_write_rate),
        "Prompts written to cache",
        bottom_row[1],
        COLORS.cache_write,
    );

    render_stat_card(
        f,
        "AVG TURN COST",
        &format_currency(analysis.avg_turn_cost),
        "Cost per turn average",
        bottom_row[2],
        COLORS.accent,
    );

    // Output % card - output as percentage of total tokens
    let total_tokens = analysis.aggregates.total();
    let output_percentage = if total_tokens > 0 {
        (analysis.aggregates.output as f64 / total_tokens as f64) * 100.0
    } else {
        0.0
    };

    render_stat_card(
        f,
        "OUTPUT %",
        &format!("{:.1}%", output_percentage),
        "Output as % of all tokens",
        bottom_row[3],
        COLORS.output,
    );

    render_token_breakdown_card(f, analysis, grid[2]);
}

fn render_token_breakdown_card(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    let block = Block::default()
        .title(" Token Breakdown ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let agg = &analysis.aggregates;
    let total = agg.total() as f64;

    if total == 0.0 {
        f.render_widget(Paragraph::new(" No tokens used ").block(block), area);
        return;
    }

    let make_span = |label: &str, count: u64, color: Color| {
        let pct = (count as f64 / total) * 100.0;
        vec![
            Span::styled(format!(" {} ", label), Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} ({:.1}%)", format_number(count), pct),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │", Style::default().fg(Color::DarkGray)),
        ]
    };

    let mut spans = Vec::new();
    spans.extend(make_span("Input", agg.input, COLORS.input));
    spans.extend(make_span(
        "Cache Write",
        agg.cache_creation,
        COLORS.cache_write,
    ));
    spans.extend(make_span("Cache Read", agg.cache_read, COLORS.cache_read));
    spans.extend(make_span("Output", agg.output, COLORS.output));

    // remove last separator
    if spans.last().map(|s| s.content.as_ref()) == Some(" │") {
        spans.pop();
    }

    let paragraph = Paragraph::new(Line::from(spans))
        .block(block)
        .alignment(Alignment::Center);

    f.render_widget(paragraph, area);
}

/// Render a single stat card
fn render_stat_card(
    f: &mut Frame,
    label: &str,
    value: &str,
    description: &str,
    area: Rect,
    value_color: Color,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let text = Text::from(vec![
        Line::from(Span::styled(label, Style::default().fg(Color::Gray))),
        Line::from(Span::styled(
            value,
            Style::default()
                .fg(value_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            description,
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

/// Render the timeline section
fn render_timeline(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    render_timeline_with_bars(f, analysis, area, false);
}

/// Render the timeline section with optional bar display
fn render_timeline_with_bars(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    show_bars: bool,
) {
    render_timeline_with_state(
        f,
        analysis,
        area,
        &AppState {
            show_bars,
            scroll_offset: 0,
            selected_turn: 0,
            privacy_mode: false,
            detailed_view: false,
        },
    );
}

/// Render the timeline section with full state support
fn render_timeline_with_state(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    app_state: &AppState,
) {
    let show_bars = app_state.show_bars;
    let title = if show_bars {
        "Timeline (Bars: Press 'b' to toggle, ↑↓ to scroll)"
    } else {
        "Timeline (Press 'b' for bars, ↑↓ to scroll)"
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLORS.accent));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if analysis.turns.is_empty() {
        let text = Paragraph::new("No data to display").alignment(Alignment::Center);
        f.render_widget(text, inner);
        return;
    }

    // Split timeline area
    let timeline_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Graphical Timeline
            Constraint::Min(5),     // Token bars
            Constraint::Length(2),  // Legend
        ])
        .split(inner);

    render_graphical_timeline(f, analysis, timeline_layout[0], app_state);

    if show_bars {
        render_turns_with_bars_and_navigation(f, analysis, timeline_layout[1], app_state);
    } else {
        render_turns_with_navigation(f, analysis, timeline_layout[1], app_state);
    }
    render_legend_with_bars(f, timeline_layout[2], show_bars);
}

/// Render session bands (green background for active sessions)
fn render_session_bands(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    if analysis.session_stats.sessions.is_empty() {
        return;
    }

    let block = Block::default().title("Sessions").borders(Borders::NONE);

    let inner = block.inner(area);

    // Calculate time range
    let all_turns = &analysis.turns;
    if all_turns.is_empty() {
        return;
    }

    let first_ts = all_turns.first().unwrap().timestamp;
    let last_ts = all_turns.last().unwrap().timestamp;
    let total_range = (last_ts - first_ts).num_seconds() as f64;

    if total_range <= 0.0 {
        return;
    }

    let width = inner.width as f64;

    // Create session info text
    let session_text: Vec<Span> = analysis
        .session_stats
        .sessions
        .iter()
        .enumerate()
        .flat_map(|(i, session)| {
            let start_pct = ((session.start - first_ts).num_seconds() as f64 / total_range * width)
                / width
                * 100.0;
            let end_pct = ((session.end - first_ts).num_seconds() as f64 / total_range * width)
                / width
                * 100.0;

            vec![
                Span::styled(
                    format!(" S{} ", i + 1),
                    Style::default()
                        .bg(COLORS.cache_read)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        " {} turns · {} ",
                        session.turn_count,
                        format_duration(session.duration_minutes)
                    ),
                    Style::default().fg(Color::Gray),
                ),
                Span::raw(" | "),
            ]
        })
        .collect();

    let text = Line::from(session_text);
    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

/// Render token bars as sparklines/sparklines-style visualization
fn render_token_bars(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    if analysis.turns.is_empty() {
        return;
    }

    let turns = &analysis.turns;
    let max_tokens = turns.iter().map(|t| t.total_tokens).max().unwrap_or(1) as f64;

    // Create data for sparkline (total tokens per turn)
    let data: Vec<u64> = turns.iter().map(|t| t.total_tokens).collect();

    // Scale to fit area
    let height = area.height as f64;
    let max_scaled = (height * 0.9) as u64;

    // Create colored sparklines for each token type
    let input_data: Vec<u64> = turns
        .iter()
        .map(|t| ((t.input as f64 / max_tokens) * max_scaled as f64) as u64)
        .collect();

    let output_data: Vec<u64> = turns
        .iter()
        .map(|t| ((t.output as f64 / max_tokens) * max_scaled as f64) as u64)
        .collect();

    let cache_read_data: Vec<u64> = turns
        .iter()
        .map(|t| ((t.cache_read as f64 / max_tokens) * max_scaled as f64) as u64)
        .collect();

    let cache_write_data: Vec<u64> = turns
        .iter()
        .map(|t| ((t.cache_creation as f64 / max_tokens) * max_scaled as f64) as u64)
        .collect();

    // Render stacked sparklines
    if !input_data.is_empty() {
        let input_spark = Sparkline::default()
            .data(&input_data)
            .style(Style::default().fg(COLORS.input));
        f.render_widget(input_spark, area);
    }

    // Use a bar chart style table as alternative
    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Model"),
        Cell::from("Input"),
        Cell::from("Output"),
        Cell::from("Cache Read"),
        Cell::from("Cache Write"),
        Cell::from("Cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = turns
        .iter()
        .take(20)
        .map(|turn| {
            let time_str = turn.timestamp.format("%H:%M").to_string();
            let model_str = turn
                .model
                .as_ref()
                .map(|m| {
                    // Show short model name
                    let name = &m.0;
                    if name.len() > 20 {
                        format!("{}...", &name[..17])
                    } else {
                        name.clone()
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());
            Row::new(vec![
                Cell::from(time_str),
                Cell::from(model_str),
                Cell::from(format_number(turn.input)),
                Cell::from(format_number(turn.output)),
                Cell::from(format_number(turn.cache_read)),
                Cell::from(format_number(turn.cache_creation)),
                Cell::from(format_currency(turn.cost)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(8),
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::NONE));

    f.render_widget(table, area);
}

/// Render the legend
fn render_legend(f: &mut Frame, area: Rect) {
    let items = vec![
        ("Cache Write", COLORS.cache_write),
        ("Cache Read", COLORS.cache_read),
        ("Input", COLORS.input),
        ("Output", COLORS.output),
        ("Cost", COLORS.cost),
    ];

    let spans: Vec<Span> = items
        .iter()
        .flat_map(|(label, color)| {
            vec![
                Span::styled("  ■ ", Style::default().fg(*color)),
                Span::styled(*label, Style::default().fg(Color::Gray)),
            ]
        })
        .collect();

    let text = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);

    f.render_widget(text, area);
}

/// Generate a mini bar graph from a numeric value
///
/// # Arguments
/// * `value` - The value to represent
/// * `max` - The maximum value in the dataset (for scaling)
/// * `width` - The maximum width of the bar in characters
/// * `fill_char` - The character to use for the bar
fn render_mini_bar(value: u64, max: u64, width: usize, fill_char: &str) -> String {
    if max == 0 {
        return format!("{:>width$}", "0", width = width);
    }

    let ratio = value as f64 / max as f64;
    let bar_width = (ratio * width as f64).ceil() as usize;
    let bar_width = bar_width.min(width);

    let bar = fill_char.repeat(bar_width);
    let padding = " ".repeat(width - bar_width);

    format!("{}{}", bar, padding)
}

/// Generate a mini bar graph for cost values
fn render_cost_bar(cost: f64, max: f64, width: usize) -> String {
    if max <= 0.0 {
        return format!("{:>width$}", "0", width = width);
    }

    let ratio = cost / max;
    let bar_width = (ratio * width as f64).ceil() as usize;
    let bar_width = bar_width.min(width);

    let bar = "█".repeat(bar_width);
    let padding = " ".repeat(width - bar_width);

    format!("{}{}", bar, padding)
}

/// Render a simple text table for non-interactive mode

/// Render token bars with mini bar graphs visualization
fn render_token_bars_with_bars(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    if analysis.turns.is_empty() {
        return;
    }

    let turns = &analysis.turns;

    // Calculate max values for scaling
    let max_input = turns.iter().map(|t| t.input).max().unwrap_or(0);
    let max_output = turns.iter().map(|t| t.output).max().unwrap_or(0);
    let max_cache_create = turns.iter().map(|t| t.cache_creation).max().unwrap_or(0);
    let max_cache_read = turns.iter().map(|t| t.cache_read).max().unwrap_or(0);
    let max_cost = turns.iter().map(|t| t.cost).fold(0.0, f64::max);

    // Bar width for TUI (smaller than CLI table)
    let bar_width = 6;

    let header = Row::new(vec![
        Cell::from("Time"),
        Cell::from("Model"),
        Cell::from("Input"),
        Cell::from("Output"),
        Cell::from("Cache"),
        Cell::from("Cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = turns
        .iter()
        .take(15) // Show fewer rows in TUI mode to fit bars
        .map(|turn| {
            let time_str = turn.timestamp.format("%H:%M").to_string();
            let model_str = turn
                .model
                .as_ref()
                .map(|m| {
                    let name = &m.0;
                    // Extract just the model family and version
                    if name.contains("opus") {
                        "Opus"
                    } else if name.contains("sonnet") {
                        "Sonnet"
                    } else if name.contains("haiku") {
                        "Haiku"
                    } else {
                        "?"
                    }
                })
                .unwrap_or("?");

            // Create bar visualizations with both number and bar
            let input_bar = format!(
                "{} {}",
                format_number(turn.input),
                render_mini_bar_colored(turn.input, max_input, bar_width, '█', COLORS.input)
            );

            let output_bar = format!(
                "{} {}",
                format_number(turn.output),
                render_mini_bar_colored(turn.output, max_output, bar_width, '▓', COLORS.output)
            );

            // Combined cache display
            let cache_total = turn.cache_creation + turn.cache_read;
            let cache_bar = format!(
                "{} {}",
                format_number(cache_total),
                render_mini_bar_colored_dual(
                    turn.cache_read,
                    turn.cache_creation,
                    max_cache_read.max(max_cache_create),
                    bar_width,
                    '▒',
                    '░',
                    COLORS.cache_read,
                    COLORS.cache_write
                )
            );

            let cost_str = format!(
                "{} {}",
                format_currency(turn.cost),
                render_cost_bar_colored(turn.cost, max_cost, bar_width, COLORS.cost)
            );

            Row::new(vec![
                Cell::from(time_str),
                Cell::from(model_str),
                Cell::from(input_bar),
                Cell::from(output_bar),
                Cell::from(cache_bar),
                Cell::from(cost_str),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(6),  // Time
            Constraint::Length(8),  // Model
            Constraint::Length(16), // Input with bar
            Constraint::Length(16), // Output with bar
            Constraint::Length(16), // Cache with bar
            Constraint::Length(16), // Cost with bar
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::NONE));

    f.render_widget(table, area);
}

/// Render a mini bar with embedded color codes for TUI
fn render_mini_bar_colored(
    value: u64,
    max: u64,
    width: usize,
    fill_char: char,
    color: Color,
) -> String {
    if max == 0 {
        return " ".repeat(width);
    }

    let ratio = value as f64 / max as f64;
    let bar_width = (ratio * width as f64).ceil() as usize;
    let bar_width = bar_width.min(width);

    let bar = fill_char.to_string().repeat(bar_width);
    let padding = " ".repeat(width - bar_width);

    format!("{}{}", bar, padding)
}

/// Render a dual-colored mini bar (for cache read + write combined)
fn render_mini_bar_colored_dual(
    value1: u64,
    value2: u64,
    max: u64,
    width: usize,
    char1: char,
    char2: char,
    _color1: Color,
    _color2: Color,
) -> String {
    if max == 0 {
        return " ".repeat(width);
    }

    let total = value1 + value2;
    let ratio = total as f64 / max as f64;
    let bar_width = (ratio * width as f64).ceil() as usize;
    let bar_width = bar_width.min(width);

    // Split bar between read and write
    let read_ratio = if total > 0 {
        value1 as f64 / total as f64
    } else {
        0.0
    };
    let read_width = (bar_width as f64 * read_ratio).round() as usize;
    let write_width = bar_width - read_width;

    let bar = char1.to_string().repeat(read_width) + &char2.to_string().repeat(write_width);
    let padding = " ".repeat(width - bar_width);

    format!("{}{}", bar, padding)
}

/// Render a mini bar for cost values with color
fn render_cost_bar_colored(cost: f64, max: f64, width: usize, _color: Color) -> String {
    if max <= 0.0 {
        return " ".repeat(width);
    }

    let ratio = cost / max;
    let bar_width = (ratio * width as f64).ceil() as usize;
    let bar_width = bar_width.min(width);

    let bar = '█'.to_string().repeat(bar_width);
    let padding = " ".repeat(width - bar_width);

    format!("{}{}", bar, padding)
}

/// Render the legend with optional bar mode indicator
fn render_legend_with_bars(f: &mut Frame, area: Rect, show_bars: bool) {
    let items = vec![
        ("Cache Write", COLORS.cache_write, "░"),
        ("Cache Read", COLORS.cache_read, "▒"),
        ("Input", COLORS.input, "█"),
        ("Output", COLORS.output, "▓"),
        ("Cost", COLORS.cost, "█"),
    ];

    let bar_indicator = if show_bars {
        " [BARS ON - press 'b' to toggle]"
    } else {
        ""
    };

    let spans: Vec<Span> = items
        .iter()
        .flat_map(|(label, color, _char)| {
            vec![
                Span::styled("  ■ ", Style::default().fg(*color)),
                Span::styled(*label, Style::default().fg(Color::Gray)),
            ]
        })
        .chain(vec![Span::styled(
            bar_indicator,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])
        .collect();

    let text = Paragraph::new(Line::from(spans)).alignment(Alignment::Center);

    f.render_widget(text, area);
}

/// Helper function to determine which session index a turn belongs to
fn get_turn_session_index(
    turn_idx: usize,
    session_stats: &crate::usage::models::SessionStats,
) -> Option<usize> {
    let mut turn_count = 0;
    for (session_idx, session) in session_stats.sessions.iter().enumerate() {
        if turn_idx < turn_count + session.turn_count {
            return Some(session_idx);
        }
        turn_count += session.turn_count;
    }
    None
}

/// Get session marker (letter) for a turn
fn get_session_marker(session_idx: usize) -> String {
    let markers = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J'];
    if session_idx < markers.len() {
        markers[session_idx].to_string()
    } else {
        format!("{}", session_idx + 1)
    }
}

fn format_time_delta(duration: chrono::Duration) -> (String, Color) {
    let secs = duration.num_seconds();
    if secs < 60 {
        (format!("+{}s", secs), Color::DarkGray)
    } else if secs < 3600 {
        let mins = secs / 60;
        let color = if mins >= 5 {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        (format!("+{}m", mins), color)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        (format!("+{}h{}m", hours, mins), Color::Red)
    }
}
fn get_session_color(session_idx: usize) -> Color {
    let colors = [
        Color::Rgb(46, 204, 113), // Green
        Color::Rgb(91, 141, 239), // Blue
        Color::Rgb(230, 126, 34), // Orange
        Color::Rgb(231, 76, 60),  // Red
        Color::Rgb(155, 89, 182), // Purple
        Color::Rgb(241, 196, 15), // Yellow
        Color::Rgb(26, 188, 156), // Teal
        Color::Rgb(52, 73, 94),   // Dark Blue
    ];
    colors[session_idx % colors.len()]
}

/// Render session overview bands
fn render_session_overview(f: &mut Frame, analysis: &SessionAnalysis, area: Rect) {
    if analysis.session_stats.sessions.is_empty() {
        return;
    }

    let block = Block::default().title("Sessions").borders(Borders::NONE);

    let session_info: Vec<Span> = analysis
        .session_stats
        .sessions
        .iter()
        .enumerate()
        .flat_map(|(i, session)| {
            let marker = get_session_marker(i);
            let color = get_session_color(i);
            let turns_str = if session.turn_count == 1 {
                "turn"
            } else {
                "turns"
            };
            vec![
                Span::styled(
                    format!(" [{}] ", marker),
                    Style::default()
                        .bg(color)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{} {} · {} min",
                        session.turn_count,
                        turns_str,
                        format_duration(session.duration_minutes)
                    ),
                    Style::default().fg(Color::Gray),
                ),
                Span::raw("  "),
            ]
        })
        .collect();

    let text = Paragraph::new(Line::from(session_info))
        .block(block)
        .alignment(Alignment::Left);

    f.render_widget(text, area);
}

/// Render turns table with navigation support (turn numbers, session markers, cache ratio)
fn render_turns_with_navigation(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    app_state: &AppState,
) {
    if analysis.turns.is_empty() {
        return;
    }

    let turns = &analysis.turns;
    let scroll = app_state.scroll_offset;
    let selected = app_state.selected_turn;
    let visible_count = (area.height as usize).saturating_sub(2).max(10);

    let header = Row::new(vec![
        Cell::from("#"),
        Cell::from("S"),
        Cell::from("Time"),
        Cell::from("Model"),
        Cell::from("Input"),
        Cell::from("Output"),
        Cell::from("Cache"),
        Cell::from("Cache%"),
        Cell::from("Cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = turns
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_count)
        .map(|(idx, turn)| {
            let is_selected = idx == selected;
            let session_idx = get_turn_session_index(idx, &analysis.session_stats);
            let session_marker = session_idx.map(get_session_marker).unwrap_or_default();
            let session_color = session_idx.map(get_session_color).unwrap_or(Color::Gray);

            let time_cell = if app_state.detailed_view && idx > 0 {
                let prev_turn = &analysis.turns[idx - 1];
                let delta = turn.timestamp.signed_duration_since(prev_turn.timestamp);
                let (delta_str, color) = format_time_delta(delta);
                Cell::from(Span::styled(delta_str, Style::default().fg(color)))
            } else {
                Cell::from(turn.timestamp.format("%H:%M").to_string())
            };

            let model_str = turn
                .model
                .as_ref()
                .map(|m| {
                    let name = &m.0;
                    if name.contains("opus") {
                        "Opus"
                    } else if name.contains("sonnet") {
                        "Sonnet"
                    } else if name.contains("haiku") {
                        "Haiku"
                    } else {
                        "?"
                    }
                })
                .unwrap_or("?");

            // Calculate cache ratio
            let prompt_tokens = turn.input + turn.cache_read + turn.cache_creation;
            let cache_ratio = if prompt_tokens > 0 {
                (turn.cache_read as f64 / prompt_tokens as f64) * 100.0
            } else {
                0.0
            };

            // Color cache ratio red if it's 0% (cache miss) and add expiry indicator
            let cache_ratio_cell = if cache_ratio == 0.0 {
                let expiry_indicator = match turn.cache_miss_reason {
                    crate::usage::models::CacheMissReason::Expired => " ⊘",
                    _ => "",
                };
                Cell::from(Span::styled(
                    format!("0%{}", expiry_indicator),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ))
            } else {
                Cell::from(format!("{:.0}%", cache_ratio))
            };

            let cost_str = match turn.cost_source {
                crate::usage::models::CostSource::Api => format_currency(turn.cost),
                _ => format!("{}*", format_currency(turn.cost)),
            };

            let cells = vec![
                Cell::from(format!("{}", idx + 1)),
                Cell::from(Span::styled(
                    session_marker,
                    Style::default()
                        .fg(session_color)
                        .add_modifier(Modifier::BOLD),
                )),
                time_cell,
                Cell::from(model_str),
                Cell::from(format_number(turn.input)),
                Cell::from(format_number(turn.output)),
                Cell::from(format_number(turn.cache_creation + turn.cache_read)),
                cache_ratio_cell,
                Cell::from(cost_str),
            ];

            let row = Row::new(cells);
            if is_selected {
                row.style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(4),  // Turn #
            Constraint::Length(2),  // Session marker
            Constraint::Length(6),  // Time
            Constraint::Length(8),  // Model
            Constraint::Length(10), // Input
            Constraint::Length(8),  // Output
            Constraint::Length(10), // Cache total
            Constraint::Length(7),  // Cache ratio
            Constraint::Length(12), // Cost
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::NONE));

    f.render_widget(table, area);
}

/// Render turns table with bars, navigation, turn numbers, session markers
fn render_turns_with_bars_and_navigation(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    app_state: &AppState,
) {
    if analysis.turns.is_empty() {
        return;
    }

    let turns = &analysis.turns;
    let scroll = app_state.scroll_offset;
    let selected = app_state.selected_turn;
    let visible_count = (area.height as usize).saturating_sub(2).max(8);

    // Calculate max values for scaling
    let max_input = turns.iter().map(|t| t.input).max().unwrap_or(0);
    let max_output = turns.iter().map(|t| t.output).max().unwrap_or(0);
    let max_cache = turns
        .iter()
        .map(|t| t.cache_creation + t.cache_read)
        .max()
        .unwrap_or(0);
    let max_cost = turns.iter().map(|t| t.cost).fold(0.0, f64::max);
    let bar_width = 5;

    let header = Row::new(vec![
        Cell::from("#"),
        Cell::from("S"),
        Cell::from("Time"),
        Cell::from("Mdl"),
        Cell::from("Input"),
        Cell::from("Output"),
        Cell::from("Cache"),
        Cell::from("C%"),
        Cell::from("Cost"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = turns
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_count)
        .map(|(idx, turn)| {
            let is_selected = idx == selected;
            let session_idx = get_turn_session_index(idx, &analysis.session_stats);
            let session_marker = session_idx.map(get_session_marker).unwrap_or_default();
            let session_color = session_idx.map(get_session_color).unwrap_or(Color::Gray);

            let time_cell = if app_state.detailed_view && idx > 0 {
                let prev_turn = &analysis.turns[idx - 1];
                let delta = turn.timestamp.signed_duration_since(prev_turn.timestamp);
                let (delta_str, color) = format_time_delta(delta);
                Cell::from(Span::styled(delta_str, Style::default().fg(color)))
            } else {
                Cell::from(turn.timestamp.format("%H:%M").to_string())
            };

            let model_str = turn
                .model
                .as_ref()
                .map(|m| {
                    let name = &m.0;
                    if name.contains("opus") {
                        "Op"
                    } else if name.contains("sonnet") {
                        "Sn"
                    } else if name.contains("haiku") {
                        "Ha"
                    } else {
                        "?"
                    }
                })
                .unwrap_or("?");

            // Calculate cache ratio
            let prompt_tokens = turn.input + turn.cache_read + turn.cache_creation;
            let cache_ratio = if prompt_tokens > 0 {
                (turn.cache_read as f64 / prompt_tokens as f64) * 100.0
            } else {
                0.0
            };

            // Create bars
            let input_bar = format!(
                "{}",
                render_mini_bar_colored(turn.input, max_input, bar_width, '█', COLORS.input)
            );
            let output_bar = format!(
                "{}",
                render_mini_bar_colored(turn.output, max_output, bar_width, '▓', COLORS.output)
            );
            let cache_bar = format!(
                "{}",
                render_mini_bar_colored_dual(
                    turn.cache_read,
                    turn.cache_creation,
                    max_cache,
                    bar_width,
                    '▒',
                    '░',
                    COLORS.cache_read,
                    COLORS.cache_write
                )
            );
            let cost_bar = format!(
                "{}",
                render_cost_bar_colored(turn.cost, max_cost, bar_width, COLORS.cost)
            );

            let cost_str = match turn.cost_source {
                crate::usage::models::CostSource::Api => format_currency(turn.cost),
                _ => format!("{}*", format_currency(turn.cost)),
            };

            // Create cache ratio cell with red highlight for 0% and expiry indicator
            let cache_ratio_cell = if cache_ratio == 0.0 {
                let expiry_indicator = match turn.cache_miss_reason {
                    crate::usage::models::CacheMissReason::Expired => "⊘ ",
                    _ => "",
                };
                Cell::from(Span::styled(
                    format!(
                        "0% {}{}",
                        expiry_indicator,
                        render_mini_bar_colored(0, 100, 4, '█', COLORS.cache_read)
                    ),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ))
            } else {
                Cell::from(format!(
                    "{:.0}% {}",
                    cache_ratio,
                    render_mini_bar_colored(cache_ratio as u64, 100, 4, '█', COLORS.cache_read)
                ))
            };

            let cells = vec![
                Cell::from(format!("{}", idx + 1)),
                Cell::from(Span::styled(
                    session_marker,
                    Style::default()
                        .fg(session_color)
                        .add_modifier(Modifier::BOLD),
                )),
                time_cell,
                Cell::from(model_str),
                Cell::from(format!("{} {}", format_number(turn.input), input_bar)),
                Cell::from(format!("{} {}", format_number(turn.output), output_bar)),
                Cell::from(format!(
                    "{} {}",
                    format_number(turn.cache_creation + turn.cache_read),
                    cache_bar
                )),
                cache_ratio_cell,
                Cell::from(format!("{} {}", cost_str, cost_bar)),
            ];

            let row = Row::new(cells);
            if is_selected {
                row.style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect();

    let table = Table::new(
        rows,
        vec![
            Constraint::Length(4),  // Turn #
            Constraint::Length(2),  // Session marker
            Constraint::Length(6),  // Time
            Constraint::Length(4),  // Model
            Constraint::Length(14), // Input with bar
            Constraint::Length(12), // Output with bar
            Constraint::Length(14), // Cache with bar
            Constraint::Length(8),  // Cache ratio with bar
            Constraint::Length(14), // Cost with bar
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::NONE));

    f.render_widget(table, area);
}

/// Render the help bar with state info
fn render_help_bar_with_state(f: &mut Frame, app_state: &AppState, area: Rect) {
    let bars_status = if app_state.show_bars { "ON" } else { "OFF" };
    let turn_info = format!(
        " | Turn: {} | Scroll: {}",
        app_state.selected_turn + 1,
        app_state.scroll_offset
    );

    let help_text = format!(
        "[q]uit | [v]iew detailed: {} | [b]ars: {} | [↑↓] nav | [PgUp/PgDn] page | [n/N] next/prev session{}",
        if app_state.detailed_view { "ON" } else { "OFF" }, bars_status, turn_info
    );

    let text = Paragraph::new(help_text)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);

    f.render_widget(text, area);
}

fn build_stacked_bars(
    turns: &[crate::usage::models::TurnSummary],
    max_tokens: u64,
    area: Rect,
) -> Paragraph<'static> {
    let height = area.height.saturating_sub(1) as usize; // Account for bottom border
    let width = area.width as usize;
    if height == 0 || width == 0 {
        return Paragraph::new("");
    }

    let mut grid: Vec<Vec<Span>> = vec![vec![Span::raw(" "); width]; height];

    for (x, t) in turns.iter().enumerate().take(width) {
        let total = t.total_tokens as f64;
        if total == 0.0 {
            continue;
        }

        let h_total = ((total / max_tokens as f64) * height as f64).ceil() as usize;
        let h_total = h_total.min(height).max(1);

        let cr = t.cache_read as f64;
        let cw = t.cache_creation as f64;
        let input = t.input as f64;
        let output = t.output as f64;

        let mut h_cr = (cr / total * h_total as f64).round() as usize;
        let mut h_cw = (cw / total * h_total as f64).round() as usize;
        let mut h_in = (input / total * h_total as f64).round() as usize;
        let mut h_out = (output / total * h_total as f64).round() as usize;

        let mut sum = h_cr + h_cw + h_in + h_out;
        while sum > h_total {
            if h_cr > 0 {
                h_cr -= 1;
            } else if h_in > 0 {
                h_in -= 1;
            } else if h_cw > 0 {
                h_cw -= 1;
            } else if h_out > 0 {
                h_out -= 1;
            }
            sum -= 1;
        }
        while sum < h_total && sum > 0 {
            if cr > 0.0 {
                h_cr += 1;
            } else if input > 0.0 {
                h_in += 1;
            } else if cw > 0.0 {
                h_cw += 1;
            } else if output > 0.0 {
                h_out += 1;
            }
            sum += 1;
        }

        let mut current_h = 0;
        let mut fill = |count: usize, color: Color| {
            for _ in 0..count {
                if current_h < height {
                    let y = height - 1 - current_h;
                    grid[y][x] = Span::styled("█", Style::default().fg(color));
                    current_h += 1;
                }
            }
        };

        fill(h_cr, COLORS.cache_read);
        fill(h_cw, COLORS.cache_write);
        fill(h_in, COLORS.input);
        fill(h_out, COLORS.output);
    }

    let lines: Vec<Line> = grid.into_iter().map(Line::from).collect();
    Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
}

fn render_graphical_timeline(
    f: &mut Frame,
    analysis: &SessionAnalysis,
    area: Rect,
    app_state: &AppState,
) {
    if analysis.turns.is_empty() {
        return;
    }

    let scroll = app_state.scroll_offset;
    let selected = app_state.selected_turn;

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(8)])
        .split(area);

    let graph_area = layout[0];
    let label_area = layout[1];

    let visible_count = graph_area.width as usize;
    let start_idx = scroll;
    let end_idx = (scroll + visible_count).min(analysis.turns.len());

    if start_idx >= analysis.turns.len() {
        return;
    }
    let visible_turns = &analysis.turns[start_idx..end_idx];

    let max_tokens = analysis
        .turns
        .iter()
        .map(|t| t.total_tokens)
        .max()
        .unwrap_or(1);
    let max_cost = analysis
        .turns
        .iter()
        .map(|t| t.cost)
        .fold(0.0, f64::max)
        .max(0.001);

    let token_data: Vec<u64> = visible_turns.iter().map(|t| t.total_tokens).collect();
    let cost_data: Vec<u64> = visible_turns
        .iter()
        .map(|t| (t.cost * 10000.0) as u64)
        .collect();

    let ribbon_spans: Vec<Span> = visible_turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let real_idx = scroll + i;
            let prompt_tokens = t.input + t.cache_read + t.cache_creation;
            let cache_ratio = if prompt_tokens > 0 {
                t.cache_read as f64 / prompt_tokens as f64
            } else {
                0.0
            };

            let color = if cache_ratio > 0.8 {
                COLORS.cache_read
            } else if cache_ratio > 0.5 {
                Color::Yellow
            } else {
                Color::Rgb(80, 80, 80)
            };

            let mut style = Style::default().fg(color);
            if real_idx == selected {
                style = style.fg(Color::White);
            }

            Span::styled("█", style)
        })
        .collect();

    let v_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Tokens sparkline + border
            Constraint::Length(3), // Cost sparkline + border
            Constraint::Length(1), // Cache ribbon
            Constraint::Length(1), // Bottom Spacer
        ])
        .split(graph_area);

    let border_style = Style::default().fg(Color::DarkGray);

    let tokens_sparkline = Sparkline::default()
        .data(&token_data)
        .max(max_tokens)
        .style(Style::default().fg(COLORS.input))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(border_style),
        );

    let cost_sparkline = Sparkline::default()
        .data(&cost_data)
        .max((max_cost * 10000.0) as u64)
        .style(Style::default().fg(COLORS.cost))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(border_style),
        );

    let ribbon_para = Paragraph::new(Line::from(ribbon_spans));

    if app_state.detailed_view {
        let stacked_bars = build_stacked_bars(visible_turns, max_tokens, v_layout[0]);
        f.render_widget(stacked_bars, v_layout[0]);
    } else {
        f.render_widget(tokens_sparkline, v_layout[0]);
    }

    f.render_widget(cost_sparkline, v_layout[1]);
    f.render_widget(ribbon_para, v_layout[2]);

    let label_v_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Tokens + border
            Constraint::Length(3), // Cost + border
            Constraint::Length(1), // Cache ribbon
            Constraint::Length(1), // Bottom Spacer
        ])
        .split(label_area);

    let token_label = Paragraph::new(format!(" T:{}", format_number(max_tokens)))
        .style(Style::default().fg(if app_state.detailed_view {
            Color::White
        } else {
            Color::Gray
        }))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(border_style),
        );

    let cost_label = Paragraph::new(format!(" ${:.2}", max_cost))
        .style(Style::default().fg(Color::Gray))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(border_style),
        );

    let cache_label = Paragraph::new(" Cache").style(Style::default().fg(Color::Gray));

    f.render_widget(token_label, label_v_layout[0]);
    f.render_widget(cost_label, label_v_layout[1]);
    f.render_widget(cache_label, label_v_layout[2]);
}
