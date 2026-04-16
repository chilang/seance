use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A unique identifier for a Claude Code session
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// A unique identifier for a message/request
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

/// A unique identifier for a request
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(pub String);

/// Model name (e.g., "claude-sonnet-4-6")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelName(pub String);

impl ModelName {
    /// Get display-friendly model name
    pub fn display(&self) -> &str {
        &self.0
    }
}

/// Project path where the session occurred
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectPath(pub String);

/// Claude Code version
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version(pub String);

/// Cost calculation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CostMode {
    /// Use pre-calculated costs when available, otherwise calculate
    #[default]
    Auto,
    /// Always calculate costs from token counts
    Calculate,
    /// Always use pre-calculated costs (show 0 if missing)
    Display,
}

impl std::str::FromStr for CostMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(CostMode::Auto),
            "calculate" => Ok(CostMode::Calculate),
            "display" => Ok(CostMode::Display),
            _ => Err(format!("Invalid cost mode: {}", s)),
        }
    }
}

/// Source of cost calculation for a turn or session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CostSource {
    /// Cost came from API (costUSD field in JSONL)
    Api,
    /// Cost was calculated locally using model pricing
    Calculated,
    /// Mix of API and calculated costs
    Mixed,
}

impl std::fmt::Display for CostSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CostSource::Api => write!(f, "API"),
            CostSource::Calculated => write!(f, "Calculated"),
            CostSource::Mixed => write!(f, "Mixed"),
        }
    }
}

/// Individual token usage entry from a Claude API call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    pub timestamp: DateTime<Utc>,
    pub session_id: Option<SessionId>,
    pub version: Option<Version>,
    pub message: Message,
    pub cost_usd: Option<f64>,
    pub request_id: Option<RequestId>,
    pub is_api_error: Option<bool>,
}

impl UsageEntry {
    /// Create a unique hash for deduplication based on message ID + request ID
    pub fn dedup_key(&self) -> String {
        format!(
            "{}:{}",
            self.message
                .id
                .as_ref()
                .map(|id| id.0.clone())
                .unwrap_or_default(),
            self.request_id
                .as_ref()
                .map(|id| id.0.clone())
                .unwrap_or_default()
        )
    }

    /// Get total tokens for this entry
    pub fn total_tokens(&self) -> u64 {
        let usage = &self.message.usage;
        usage.input_tokens
            + usage.output_tokens
            + usage.cache_creation_input_tokens.unwrap_or(0)
            + usage.cache_read_input_tokens.unwrap_or(0)
    }

    /// Get prompt tokens (input + cache related)
    pub fn prompt_tokens(&self) -> u64 {
        let usage = &self.message.usage;
        usage.input_tokens
            + usage.cache_creation_input_tokens.unwrap_or(0)
            + usage.cache_read_input_tokens.unwrap_or(0)
    }
}

/// Message structure within a usage entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub usage: TokenUsage,
    pub model: Option<ModelName>,
    pub id: Option<MessageId>,
    pub content: Option<Vec<MessageContent>>,
}

/// Detailed cache creation breakdown by duration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheCreationBreakdown {
    #[serde(rename = "ephemeral_5m_input_tokens")]
    pub ephemeral_5m: Option<u64>,
    #[serde(rename = "ephemeral_1h_input_tokens")]
    pub ephemeral_1h: Option<u64>,
}

/// Token usage details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(rename = "cache_creation_input_tokens")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(rename = "cache_read_input_tokens")]
    pub cache_read_input_tokens: Option<u64>,
    /// Detailed breakdown of cache creation by duration
    pub cache_creation: Option<CacheCreationBreakdown>,
    pub speed: Option<String>, // "standard" or "fast"
}

/// Message content (if available)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: Option<String>,
}

/// Aggregated token counts across multiple entries
#[derive(Debug, Clone, Default, Serialize)]
pub struct TokenAggregates {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_creation_5m: u64,
    pub cache_creation_1h: u64,
    pub cache_read: u64,
}

impl TokenAggregates {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_creation + self.cache_read
    }

    pub fn prompt(&self) -> u64 {
        self.input + self.cache_creation + self.cache_read
    }
}

/// Cost breakdown by token type
#[derive(Debug, Clone, Default, Serialize)]
pub struct CostBreakdown {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

impl CostBreakdown {
    pub fn total(&self) -> f64 {
        self.input + self.output + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }

    /// Combined cache write cost for display
    pub fn cache_write_total(&self) -> f64 {
        self.cache_write_5m + self.cache_write_1h
    }
}

/// A session window (period of continuous activity)
#[derive(Debug, Clone, Serialize)]
pub struct SessionWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub duration_minutes: f64,
    pub turn_count: usize,
}

/// Session statistics
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionStats {
    pub session_count: usize,
    pub total_session_time_minutes: f64,
    pub avg_session_time_minutes: f64,
    pub sessions: Vec<SessionWindow>,
}

/// Model pricing rates (per 1M tokens)
#[derive(Debug, Clone)]
pub struct ModelRates {
    pub input: f64,          // $ per 1M tokens
    pub output: f64,         // $ per 1M tokens
    pub cache_read: f64,     // $ per 1M tokens
    pub cache_write_5m: f64, // $ per 1M tokens
    pub cache_write_1h: f64, // $ per 1M tokens
}

impl Default for ModelRates {
    fn default() -> Self {
        // Default to Claude Sonnet 4.6 rates
        Self {
            input: 3.0,
            output: 15.0,
            cache_read: 0.30,
            cache_write_5m: 3.75,
            cache_write_1h: 6.0,
        }
    }
}

impl ModelRates {
    /// Get rates for a specific model based on model name
    /// Uses fuzzy matching to handle versioned model names
    pub fn for_model(model_name: &str) -> Self {
        let name_lower = model_name.to_lowercase();

        // Opus 4.6 / 4.5: $5/25 MTok, CW 5m: $6.25, CW 1h: $10, CR: $0.50
        if name_lower.contains("opus-4-6") || name_lower.contains("opus-4-5") {
            return Self {
                input: 5.0,
                output: 25.0,
                cache_read: 0.50,
                cache_write_5m: 6.25,
                cache_write_1h: 10.0,
            };
        }

        // Opus 4.1 / 4.0 / 3: $15/75 MTok, CW 5m: $18.75, CW 1h: $30, CR: $1.50
        if name_lower.contains("opus-4-1")
            || name_lower.contains("opus-4-0")
            || name_lower.contains("opus-3")
        {
            return Self {
                input: 15.0,
                output: 75.0,
                cache_read: 1.50,
                cache_write_5m: 18.75,
                cache_write_1h: 30.0,
            };
        }

        // Haiku 4.5: $1/5 MTok, CW 5m: $1.25, CW 1h: $2, CR: $0.10
        if name_lower.contains("haiku-4-5") {
            return Self {
                input: 1.0,
                output: 5.0,
                cache_read: 0.10,
                cache_write_5m: 1.25,
                cache_write_1h: 2.0,
            };
        }

        // Haiku 3.5: $0.80/4 MTok, CW 5m: $1, CW 1h: $1.60, CR: $0.08
        if name_lower.contains("haiku-3-5") {
            return Self {
                input: 0.80,
                output: 4.0,
                cache_read: 0.08,
                cache_write_5m: 1.0,
                cache_write_1h: 1.60,
            };
        }

        // Haiku 3: $0.25/1.25 MTok, CW 5m: $0.30, CW 1h: $0.50, CR: $0.03
        if name_lower.contains("haiku-3") {
            return Self {
                input: 0.25,
                output: 1.25,
                cache_read: 0.03,
                cache_write_5m: 0.30,
                cache_write_1h: 0.50,
            };
        }

        // Sonnet 4.6 / 4.5 / 4 / 3.7: $3/15 MTok, CW 5m: $3.75, CW 1h: $6, CR: $0.30
        // Default fallback for any sonnet
        if name_lower.contains("sonnet") {
            return Self {
                input: 3.0,
                output: 15.0,
                cache_read: 0.30,
                cache_write_5m: 3.75,
                cache_write_1h: 6.0,
            };
        }

        // Opus fallback (for unknown opus models)
        if name_lower.contains("opus") {
            return Self {
                input: 5.0,
                output: 25.0,
                cache_read: 0.50,
                cache_write_5m: 6.25,
                cache_write_1h: 10.0,
            };
        }

        // Haiku fallback (for unknown haiku models)
        if name_lower.contains("haiku") {
            return Self {
                input: 0.80,
                output: 4.0,
                cache_read: 0.08,
                cache_write_5m: 1.0,
                cache_write_1h: 1.60,
            };
        }

        // Default to Sonnet 4.6
        Self::default()
    }

    /// Calculate costs from token aggregates using detailed cache breakdown
    pub fn calculate_costs(&self, aggregates: &TokenAggregates) -> CostBreakdown {
        CostBreakdown {
            input: (aggregates.input as f64 / 1_000_000.0) * self.input,
            output: (aggregates.output as f64 / 1_000_000.0) * self.output,
            cache_read: (aggregates.cache_read as f64 / 1_000_000.0) * self.cache_read,
            cache_write_5m: (aggregates.cache_creation_5m as f64 / 1_000_000.0)
                * self.cache_write_5m,
            cache_write_1h: (aggregates.cache_creation_1h as f64 / 1_000_000.0)
                * self.cache_write_1h,
        }
    }

    /// Calculate cost for a single entry with detailed cache breakdown
    pub fn calculate_entry_cost(&self, usage: &TokenUsage) -> f64 {
        let input_cost = (usage.input_tokens as f64 / 1_000_000.0) * self.input;
        let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * self.output;
        let cache_read_cost =
            (usage.cache_read_input_tokens.unwrap_or(0) as f64 / 1_000_000.0) * self.cache_read;

        // Handle detailed cache creation breakdown
        let (cache_write_5m, cache_write_1h) = if let Some(ref breakdown) = usage.cache_creation {
            let tokens_5m = breakdown.ephemeral_5m.unwrap_or(0);
            let tokens_1h = breakdown.ephemeral_1h.unwrap_or(0);

            // If we have breakdown, use it
            let cost_5m = (tokens_5m as f64 / 1_000_000.0) * self.cache_write_5m;
            let cost_1h = (tokens_1h as f64 / 1_000_000.0) * self.cache_write_1h;
            (cost_5m, cost_1h)
        } else {
            // Fallback: use the total cache_creation_input_tokens
            // Assume all 5m (cheaper) when we don't have breakdown
            let total_cache = usage.cache_creation_input_tokens.unwrap_or(0);
            let cost = (total_cache as f64 / 1_000_000.0) * self.cache_write_5m;
            (cost, 0.0)
        };

        input_cost + output_cost + cache_read_cost + cache_write_5m + cache_write_1h
    }
}

/// Complete session analysis result
#[derive(Debug, Clone, Serialize)]
pub struct SessionAnalysis {
    pub session_id: SessionId,
    pub project: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub turn_count: usize,
    pub aggregates: TokenAggregates,
    pub costs: CostBreakdown,
    pub total_cost: f64,
    pub cost_source: CostSource,
    pub cache_hit_rate: f64,
    pub cache_write_rate: f64,
    pub models_used: Vec<ModelName>,
    pub session_stats: SessionStats,
    pub time_spent_minutes: f64,
    pub avg_turn_cost: f64,
    pub turns: Vec<TurnSummary>,
}

/// Cache miss reason
#[derive(Debug, Clone, Serialize)]
pub enum CacheMissReason {
    /// Cache expired (5+ minute gap from previous turn with cache hits)
    Expired,
    /// Prefix miss (first time seeing this prompt, no matching prefix)
    Prefix,
    /// Not a miss (has cache hits)
    None,
}

impl std::fmt::Display for CacheMissReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheMissReason::Expired => write!(f, "⊘"),
            CacheMissReason::Prefix => write!(f, ""),
            CacheMissReason::None => write!(f, ""),
        }
    }
}

/// Individual turn summary for timeline display
#[derive(Debug, Clone, Serialize)]
pub struct TurnSummary {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub model: Option<ModelName>,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_creation_5m: u64,
    pub cache_creation_1h: u64,
    pub cache_read: u64,
    pub cost: f64,
    pub cost_source: CostSource,
    pub total_tokens: u64,
    pub cache_miss_reason: CacheMissReason,
}

/// Cache efficiency metrics
#[derive(Debug, Clone, Serialize)]
pub struct CacheEfficiency {
    pub hit_rate: f64,   // percentage
    pub write_rate: f64, // percentage
    pub savings: f64,    // dollars saved vs no cache
}

/// Model breakdown within a session
#[derive(Debug, Clone, Serialize)]
pub struct ModelBreakdown {
    pub model: ModelName,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost: f64,
}
