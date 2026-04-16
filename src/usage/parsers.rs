use crate::usage::models::*;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct RawUsageEntry {
    timestamp: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    version: Option<String>,
    message: Option<RawMessage>,
    #[serde(rename = "costUSD")]
    cost_usd: Option<f64>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "isApiErrorMessage")]
    is_api_error: Option<bool>,
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(rename = "uuid")]
    uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    usage: RawTokenUsage,
    model: Option<String>,
    id: Option<String>,
    content: Option<Vec<RawMessageContent>>,
    role: Option<String>,
    #[serde(rename = "type")]
    message_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCacheCreation {
    #[serde(rename = "ephemeral_5m_input_tokens")]
    ephemeral_5m_input_tokens: Option<u64>,
    #[serde(rename = "ephemeral_1h_input_tokens")]
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawTokenUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    #[serde(rename = "cache_creation")]
    cache_creation: Option<RawCacheCreation>,
    speed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMessageContent {
    text: Option<String>,
}

fn parse_line(line: &str) -> Result<Option<UsageEntry>, Box<dyn std::error::Error>> {
    let raw: RawUsageEntry = match serde_json::from_str(line) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };

    let entry_type = raw.entry_type.as_deref().unwrap_or("");
    if entry_type != "assistant" {
        return Ok(None);
    }

    let raw_message = match raw.message {
        Some(m) => m,
        None => return Ok(None),
    };

    let timestamp = match DateTime::parse_from_rfc3339(&raw.timestamp) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return Ok(None),
    };

    let cache_creation_breakdown =
        raw_message
            .usage
            .cache_creation
            .map(|raw| CacheCreationBreakdown {
                ephemeral_5m: raw.ephemeral_5m_input_tokens,
                ephemeral_1h: raw.ephemeral_1h_input_tokens,
            });

    let entry = UsageEntry {
        timestamp,
        session_id: raw.session_id.map(SessionId),
        version: raw.version.map(Version),
        message: Message {
            usage: TokenUsage {
                input_tokens: raw_message.usage.input_tokens,
                output_tokens: raw_message.usage.output_tokens,
                cache_creation_input_tokens: raw_message.usage.cache_creation_input_tokens,
                cache_read_input_tokens: raw_message.usage.cache_read_input_tokens,
                cache_creation: cache_creation_breakdown,
                speed: raw_message.usage.speed,
            },
            model: raw_message.model.map(ModelName),
            id: raw_message.id.map(MessageId),
            content: raw_message.content.map(|contents| {
                contents
                    .into_iter()
                    .map(|c| MessageContent { text: c.text })
                    .collect()
            }),
        },
        cost_usd: raw.cost_usd,
        request_id: raw.request_id.map(RequestId),
        is_api_error: raw.is_api_error,
    };

    Ok(Some(entry))
}

pub fn parse_jsonl_file(
    path: &Path,
    deduplicate: bool,
) -> Result<Vec<UsageEntry>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match parse_line(&line) {
            Ok(Some(entry)) => {
                if deduplicate {
                    let key = entry.dedup_key();
                    if !key.is_empty() && seen_keys.contains(&key) {
                        continue;
                    }
                    seen_keys.insert(key);
                }
                entries.push(entry);
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Warning: Failed to parse line: {}", e);
            }
        }
    }

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(entries)
}
