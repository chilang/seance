use crate::usage::models::*;
use chrono::{DateTime, Utc};
use std::error::Error;

/// Default idle threshold for session detection (5 minutes)
const DEFAULT_IDLE_THRESHOLD_MINUTES: i64 = 5;

/// Detect sessions from a sequence of timestamps
/// A new session starts when the gap between consecutive timestamps exceeds the threshold
pub fn detect_sessions(
    timestamps: &[DateTime<Utc>],
    max_gap_minutes: i64,
) -> Vec<Vec<DateTime<Utc>>> {
    if timestamps.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<_> = timestamps.to_vec();
    sorted.sort();

    let mut sessions: Vec<Vec<DateTime<Utc>>> = Vec::new();
    let mut current_session = vec![sorted[0]];

    for window in sorted.windows(2) {
        let first = window[0];
        let second = window[1];

        let gap_minutes = (second - first).num_minutes();

        if gap_minutes <= max_gap_minutes {
            current_session.push(second);
        } else {
            sessions.push(current_session);
            current_session = vec![second];
        }
    }

    sessions.push(current_session);
    sessions
}

/// Calculate session statistics from timestamps
pub fn calculate_session_stats(timestamps: &[DateTime<Utc>], max_gap_minutes: i64) -> SessionStats {
    let sessions = detect_sessions(timestamps, max_gap_minutes);

    if sessions.is_empty() {
        return SessionStats::default();
    }

    let mut total_duration = 0.0;
    let mut session_windows = Vec::new();

    for session in &sessions {
        if session.is_empty() {
            continue;
        }

        let start = *session.first().unwrap();
        let end = *session.last().unwrap();
        let duration = (end - start).num_seconds() as f64 / 60.0;

        total_duration += duration;

        session_windows.push(SessionWindow {
            start,
            end,
            duration_minutes: duration,
            turn_count: session.len(),
        });
    }

    let count = sessions.len();
    let avg_duration = if count > 0 {
        total_duration / count as f64
    } else {
        0.0
    };

    SessionStats {
        session_count: count,
        total_session_time_minutes: total_duration,
        avg_session_time_minutes: avg_duration,
        sessions: session_windows,
    }
}

/// Aggregate token counts from usage entries with detailed cache breakdown
pub fn aggregate_tokens(entries: &[UsageEntry]) -> TokenAggregates {
    let mut aggregates = TokenAggregates::default();

    for entry in entries {
        let usage = &entry.message.usage;
        aggregates.input += usage.input_tokens;
        aggregates.output += usage.output_tokens;
        aggregates.cache_creation += usage.cache_creation_input_tokens.unwrap_or(0);
        aggregates.cache_read += usage.cache_read_input_tokens.unwrap_or(0);

        // Aggregate detailed cache creation breakdown
        if let Some(ref breakdown) = usage.cache_creation {
            aggregates.cache_creation_5m += breakdown.ephemeral_5m.unwrap_or(0);
            aggregates.cache_creation_1h += breakdown.ephemeral_1h.unwrap_or(0);
        } else {
            // If no breakdown, put all in 5m (cheaper rate)
            aggregates.cache_creation_5m += usage.cache_creation_input_tokens.unwrap_or(0);
        }
    }

    aggregates
}

/// Calculate cache efficiency metrics
pub fn calculate_cache_rates(aggregates: &TokenAggregates) -> (f64, f64) {
    let prompt = aggregates.prompt();

    if prompt == 0 {
        return (0.0, 0.0);
    }

    let hit_rate = (aggregates.cache_read as f64 / prompt as f64) * 100.0;
    let write_rate = (aggregates.cache_creation as f64 / prompt as f64) * 100.0;

    (hit_rate, write_rate)
}

/// Calculate cost for a single entry using model-specific rates
fn calculate_entry_cost_with_rates(entry: &UsageEntry, rates: &ModelRates) -> f64 {
    rates.calculate_entry_cost(&entry.message.usage)
}

/// Get the appropriate rates for a usage entry based on its model
fn get_entry_rates(entry: &UsageEntry) -> ModelRates {
    match &entry.message.model {
        Some(model) => ModelRates::for_model(&model.0),
        None => ModelRates::default(),
    }
}

/// Calculate cost for a single turn based on cost mode and entry data
fn calculate_turn_cost(entry: &UsageEntry, mode: CostMode) -> (f64, CostSource) {
    match mode {
        CostMode::Auto => {
            if let Some(cost) = entry.cost_usd {
                (cost, CostSource::Api)
            } else {
                let rates = get_entry_rates(entry);
                let cost = calculate_entry_cost_with_rates(entry, &rates);
                (cost, CostSource::Calculated)
            }
        }
        CostMode::Calculate => {
            let rates = get_entry_rates(entry);
            let cost = calculate_entry_cost_with_rates(entry, &rates);
            (cost, CostSource::Calculated)
        }
        CostMode::Display => {
            if let Some(cost) = entry.cost_usd {
                (cost, CostSource::Api)
            } else {
                (0.0, CostSource::Api) // Display mode shows 0 for missing costs
            }
        }
    }
}

/// Calculate total cost and determine cost source
pub fn calculate_session_cost(entries: &[UsageEntry], mode: CostMode) -> (f64, CostSource) {
    let mut total_cost = 0.0;
    let mut api_count = 0;
    let mut calculated_count = 0;

    for entry in entries {
        let (cost, source) = calculate_turn_cost(entry, mode);
        total_cost += cost;

        match source {
            CostSource::Api => api_count += 1,
            CostSource::Calculated => calculated_count += 1,
            _ => {}
        }
    }

    let source = if api_count == entries.len() {
        CostSource::Api
    } else if calculated_count == entries.len() {
        CostSource::Calculated
    } else {
        CostSource::Mixed
    };

    (total_cost, source)
}

/// Get unique models used in entries
pub fn get_models_used(entries: &[UsageEntry]) -> Vec<ModelName> {
    use std::collections::HashSet;

    let mut models: Vec<_> = entries
        .iter()
        .filter_map(|e| e.message.model.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    models.sort_by(|a, b| a.0.cmp(&b.0));
    models
}

/// Create turn summaries for timeline display with cache miss detection
pub fn create_turn_summaries(entries: &[UsageEntry], mode: CostMode) -> Vec<TurnSummary> {
    const CACHE_EXPIRY_MINUTES: i64 = 5;

    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let usage = &entry.message.usage;
            let (cost, cost_source) = calculate_turn_cost(entry, mode);
            let total = usage.input_tokens
                + usage.output_tokens
                + usage.cache_creation_input_tokens.unwrap_or(0)
                + usage.cache_read_input_tokens.unwrap_or(0);

            // Extract cache creation breakdown
            let (cache_5m, cache_1h) = if let Some(ref breakdown) = usage.cache_creation {
                (
                    breakdown.ephemeral_5m.unwrap_or(0),
                    breakdown.ephemeral_1h.unwrap_or(0),
                )
            } else {
                // If no breakdown, all goes to 5m
                (usage.cache_creation_input_tokens.unwrap_or(0), 0)
            };

            let cache_read = usage.cache_read_input_tokens.unwrap_or(0);

            // Detect cache miss reason
            let cache_miss_reason = if cache_read == 0 {
                // Check previous turn for expiry detection
                if idx > 0 {
                    let prev_entry = &entries[idx - 1];
                    let prev_cache_read = prev_entry
                        .message
                        .usage
                        .cache_read_input_tokens
                        .unwrap_or(0);
                    let time_gap = entry.timestamp.signed_duration_since(prev_entry.timestamp);

                    if prev_cache_read > 0 && time_gap.num_minutes() >= CACHE_EXPIRY_MINUTES {
                        CacheMissReason::Expired
                    } else {
                        CacheMissReason::Prefix
                    }
                } else {
                    // First turn with 0 cache - must be prefix miss
                    CacheMissReason::Prefix
                }
            } else {
                CacheMissReason::None
            };

            TurnSummary {
                id: entry
                    .message
                    .id
                    .as_ref()
                    .map(|id| id.0.clone())
                    .or_else(|| entry.request_id.as_ref().map(|id| id.0.clone()))
                    .unwrap_or_default(),
                timestamp: entry.timestamp,
                model: entry.message.model.clone(),
                input: usage.input_tokens,
                output: usage.output_tokens,
                cache_creation: usage.cache_creation_input_tokens.unwrap_or(0),
                cache_creation_5m: cache_5m,
                cache_creation_1h: cache_1h,
                cache_read,
                cost,
                cost_source,
                total_tokens: total,
                cache_miss_reason,
            }
        })
        .collect()
}

/// Calculate cost breakdown by token type for the session
pub fn calculate_cost_breakdown(entries: &[UsageEntry], mode: CostMode) -> CostBreakdown {
    let mut breakdown = CostBreakdown::default();

    for entry in entries {
        let usage = &entry.message.usage;
        let rates = get_entry_rates(entry);

        // Calculate per-type costs
        breakdown.input += (usage.input_tokens as f64 / 1_000_000.0) * rates.input;
        breakdown.output += (usage.output_tokens as f64 / 1_000_000.0) * rates.output;
        breakdown.cache_read +=
            (usage.cache_read_input_tokens.unwrap_or(0) as f64 / 1_000_000.0) * rates.cache_read;

        // Handle cache creation with breakdown
        if let Some(ref cache_creation) = usage.cache_creation {
            let tokens_5m = cache_creation.ephemeral_5m.unwrap_or(0);
            let tokens_1h = cache_creation.ephemeral_1h.unwrap_or(0);
            breakdown.cache_write_5m += (tokens_5m as f64 / 1_000_000.0) * rates.cache_write_5m;
            breakdown.cache_write_1h += (tokens_1h as f64 / 1_000_000.0) * rates.cache_write_1h;
        } else {
            // Fallback: use 5m rate for all
            let total_cache = usage.cache_creation_input_tokens.unwrap_or(0);
            breakdown.cache_write_5m += (total_cache as f64 / 1_000_000.0) * rates.cache_write_5m;
        }
    }

    breakdown
}

/// Analyze a complete session
pub fn analyze_session(
    entries: Vec<UsageEntry>,
    session_id: SessionId,
    cost_mode: CostMode,
) -> Result<SessionAnalysis, Box<dyn Error>> {
    if entries.is_empty() {
        return Err("No entries to analyze".into());
    }

    // Calculate aggregates
    let aggregates = aggregate_tokens(&entries);

    // Calculate costs
    let (total_cost, cost_source) = calculate_session_cost(&entries, cost_mode);
    let costs = calculate_cost_breakdown(&entries, cost_mode);

    // Calculate cache rates
    let (cache_hit_rate, cache_write_rate) = calculate_cache_rates(&aggregates);

    // Get timestamps for session detection
    let timestamps: Vec<_> = entries.iter().map(|e| e.timestamp).collect();
    let session_stats = calculate_session_stats(&timestamps, DEFAULT_IDLE_THRESHOLD_MINUTES);

    // Calculate time spent
    let time_spent_minutes =
        if let (Some(first), Some(last)) = (timestamps.first(), timestamps.last()) {
            (*last - *first).num_seconds() as f64 / 60.0
        } else {
            0.0
        };

    // Average cost per turn
    let avg_turn_cost = if !entries.is_empty() {
        total_cost / entries.len() as f64
    } else {
        0.0
    };

    // Get models used
    let models_used = get_models_used(&entries);

    // Create turn summaries
    let turns = create_turn_summaries(&entries, cost_mode);

    // Get project from entries (from session_id field or directory)
    let project = entries
        .first()
        .and_then(|e| e.session_id.as_ref())
        .map(|s| s.0.clone());

    Ok(SessionAnalysis {
        session_id,
        project,
        title: None,
        updated_at: timestamps.last().copied(),
        turn_count: entries.len(),
        aggregates,
        costs,
        total_cost,
        cost_source,
        cache_hit_rate,
        cache_write_rate,
        models_used,
        session_stats,
        time_spent_minutes,
        avg_turn_cost,
        turns,
    })
}

/// Format minutes as hours and minutes
pub fn format_duration(minutes: f64) -> String {
    if minutes < 60.0 {
        format!("{:.1}m", minutes)
    } else {
        let hours = (minutes / 60.0).floor();
        let mins = minutes % 60.0;
        format!("{}h {:.1}m", hours, mins)
    }
}

/// Format large numbers with thousands separators
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();

    if len <= 3 {
        return s;
    }

    let first_group_size = len % 3;
    let first_group_size = if first_group_size == 0 {
        3
    } else {
        first_group_size
    };

    let mut result = String::with_capacity(len + len / 3);
    result.push_str(&s[..first_group_size]);

    let mut i = first_group_size;
    while i < len {
        result.push(',');
        result.push_str(&s[i..i + 3]);
        i += 3;
    }

    result
}

/// Format currency with $ and 4 decimal places
pub fn format_currency(amount: f64) -> String {
    format!("${:.4}", amount)
}

/// Format currency with cost source indicator
pub fn format_currency_with_source(amount: f64, source: CostSource) -> String {
    match source {
        CostSource::Api => format!("${:.4}", amount),
        CostSource::Calculated => format!("${:.4}*", amount),
        CostSource::Mixed => format!("${:.4}~", amount),
    }
}

/// Format percentage with 1 decimal place
pub fn format_percentage(pct: f64) -> String {
    format!("{:.1}%", pct)
}
