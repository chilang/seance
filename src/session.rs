use chrono::{DateTime, Local};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub cwd: String,
    pub project_name: String,
    pub last_modified: DateTime<Local>,
    pub size_bytes: u64,
    pub user_msg_count: usize,
    pub first_prompt: Option<String>,
    pub last_prompt: Option<String>,
    pub all_prompts: Vec<String>,
    pub tool_keywords: Vec<String>,
    pub is_alive: bool,
}

#[derive(Deserialize)]
struct ActiveSession {
    pid: u64,
    #[serde(rename = "sessionId")]
    session_id: String,
}

#[derive(Deserialize)]
struct JournalEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<MessageContent>,
    cwd: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Structured { content: ContentValue },
    Other(()),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContentValue {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    text: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

fn is_pid_alive(pid: u64) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn get_alive_session_ids(claude_dir: &Path) -> HashSet<String> {
    let sessions_dir = claude_dir.join("sessions");
    let mut alive = HashSet::new();

    if let Ok(entries) = fs::read_dir(sessions_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().map_or(false, |e| e == "json") {
                if let Ok(data) = fs::read_to_string(entry.path()) {
                    if let Ok(s) = serde_json::from_str::<ActiveSession>(&data) {
                        if is_pid_alive(s.pid) {
                            alive.insert(s.session_id);
                        }
                    }
                }
            }
        }
    }
    alive
}

fn extract_user_text(entry: &JournalEntry) -> Option<String> {
    let msg = entry.message.as_ref()?;
    let raw = match msg {
        MessageContent::Structured { content } => match content {
            ContentValue::Text(s) => s.clone(),
            ContentValue::Blocks(blocks) => {
                blocks
                    .iter()
                    .find_map(|b| {
                        if b.block_type.as_deref() == Some("text") {
                            b.text.clone().filter(|t| !t.trim().is_empty())
                        } else {
                            None
                        }
                    })?
            }
        },
        MessageContent::Other(_) => return None,
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Skip auto-summaries
    if trimmed.starts_with("Write a 3-6 word summary") {
        return None;
    }

    // Skip local-command-caveat (system generated)
    if trimmed.starts_with("<local-command-caveat>") {
        return None;
    }

    // Strip system-reminder wrappers to get the real content
    let mut text = trimmed.to_string();
    while text.starts_with("<system-reminder>") {
        if let Some(pos) = text.find("</system-reminder>") {
            text = text[pos + "</system-reminder>".len()..].trim().to_string();
        } else {
            return None;
        }
    }
    if text.is_empty() {
        return None;
    }

    // Extract command name from XML-tagged commands
    if text.starts_with("<command-name>") || text.starts_with("<command-message>") {
        // Look for the slash-command name
        let mut cmd_name = None;
        for segment in text.split('<') {
            let segment = segment.trim();
            if let Some(rest) = segment.strip_prefix("command-name>") {
                let name = rest.trim().trim_start_matches('/');
                let name = name.split('<').next().unwrap_or(name).trim();
                if !name.is_empty() {
                    cmd_name = Some(format!("/{name}"));
                }
            }
        }
        if let Some(name) = cmd_name {
            // Skip /clear commands
            if name == "/clear" {
                return None;
            }
            return Some(name);
        }
        // Fallback: couldn't parse command, skip it
        return None;
    }

    // Skip standalone /clear
    if text.trim() == "/clear" || text.trim() == "clear" {
        return None;
    }

    Some(text)
}

fn extract_assistant_keywords(entry: &JournalEntry) -> Vec<String> {
    let msg = match entry.message.as_ref() {
        Some(msg) => msg,
        None => return Vec::new(),
    };

    let blocks = match msg {
        MessageContent::Structured { content } => match content {
            ContentValue::Blocks(blocks) => blocks,
            ContentValue::Text(_) => return Vec::new(),
        },
        MessageContent::Other(_) => return Vec::new(),
    };

    let mut keywords = Vec::new();
    let searchable_fields = ["command", "file_path", "path", "pattern", "prompt", "description"];

    for block in blocks {
        match block.block_type.as_deref() {
            Some("tool_use") => {
                let tool_name = block.name.as_deref().unwrap_or("");
                let mut parts = vec![tool_name.to_string()];

                if let Some(ref input) = block.input {
                    for key in &searchable_fields {
                        if let Some(val) = input.get(*key).and_then(|v| v.as_str()) {
                            let truncated: String = val.chars().take(500).collect();
                            parts.push(truncated);
                        }
                    }
                }

                let combined = parts.join(" ").to_lowercase();
                if !combined.trim().is_empty() {
                    keywords.push(combined);
                }
            }
            Some("text") => {
                if let Some(ref text) = block.text {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let snippet: String = trimmed.chars().take(200).collect();
                        keywords.push(snippet.to_lowercase());
                    }
                }
            }
            _ => {}
        }
    }

    keywords
}

fn project_name_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.to_string())
}

fn parse_session_file(path: &Path, alive_ids: &HashSet<String>) -> Option<Session> {
    let metadata = fs::metadata(path).ok()?;
    let last_modified: DateTime<Local> = metadata.modified().ok()?.into();
    let size_bytes = metadata.len();

    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut all_prompts: Vec<String> = Vec::new();
    let mut tool_keywords: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: JournalEntry = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Grab session metadata from first entry that has it
        if session_id.is_none() {
            if let Some(ref sid) = entry.session_id {
                session_id = Some(sid.clone());
            }
        }
        if cwd.is_none() {
            if let Some(ref c) = entry.cwd {
                cwd = Some(c.clone());
            }
        }

        // Extract user prompts
        if entry.entry_type.as_deref() == Some("user") {
            if let Some(text) = extract_user_text(&entry) {
                all_prompts.push(text);
            }
        }

        // Extract assistant tool keywords
        if entry.entry_type.as_deref() == Some("assistant") {
            tool_keywords.extend(extract_assistant_keywords(&entry));
        }
    }

    let sid = session_id.unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let cwd = cwd.unwrap_or_default();
    let project_name = project_name_from_cwd(&cwd);
    let is_alive = alive_ids.contains(&sid);

    let user_msg_count = all_prompts.len();
    let first_prompt = all_prompts.first().cloned();
    let last_prompt = all_prompts.last().cloned();

    Some(Session {
        id: sid,
        cwd,
        project_name,
        last_modified,
        size_bytes,
        user_msg_count,
        first_prompt,
        last_prompt,
        all_prompts,
        tool_keywords,
        is_alive,
    })
}

/// Collect all jsonl file paths sorted by mtime descending (newest first).
/// This is fast — only stat, no parsing.
fn collect_session_files() -> (Vec<(PathBuf, std::time::SystemTime)>, HashSet<String>) {
    let home = dirs::home_dir().expect("no home directory");
    let claude_dir = home.join(".claude");
    let projects_dir = claude_dir.join("projects");

    let alive_ids = get_alive_session_ids(&claude_dir);
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

    if let Ok(project_entries) = fs::read_dir(&projects_dir) {
        for project_entry in project_entries.flatten() {
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }
            if let Ok(dir_files) = fs::read_dir(&project_path) {
                for file in dir_files.flatten() {
                    let fpath = file.path();
                    if fpath.extension().map_or(false, |e| e == "jsonl")
                        && !fpath
                            .parent()
                            .map_or(false, |p| p.ends_with("subagents"))
                    {
                        if let Ok(meta) = fs::metadata(&fpath) {
                            if let Ok(mtime) = meta.modified() {
                                files.push((fpath, mtime));
                            }
                        }
                    }
                }
            }
        }
    }

    // Newest first
    files.sort_by(|a, b| b.1.cmp(&a.1));
    (files, alive_ids)
}

/// Parse a batch of session files. Returns parsed sessions (skipping empties).
fn parse_batch(files: &[(PathBuf, std::time::SystemTime)], alive_ids: &HashSet<String>) -> Vec<Session> {
    files
        .iter()
        .filter_map(|(path, _)| {
            let s = parse_session_file(path, alive_ids)?;
            if s.user_msg_count == 0 { None } else { Some(s) }
        })
        .collect()
}

/// Load recent sessions first (fast initial render), return remaining file list for background loading.
pub fn discover_sessions_incremental(batch_size: usize) -> (Vec<Session>, Vec<(PathBuf, std::time::SystemTime)>, HashSet<String>) {
    let (files, alive_ids) = collect_session_files();
    let first_batch_end = batch_size.min(files.len());
    let first_batch = parse_batch(&files[..first_batch_end], &alive_ids);
    let remaining = files[first_batch_end..].to_vec();
    (first_batch, remaining, alive_ids)
}

/// Parse the remaining files (called from background after initial render).
pub fn load_remaining(files: &[(PathBuf, std::time::SystemTime)], alive_ids: &HashSet<String>) -> Vec<Session> {
    parse_batch(files, alive_ids)
}

/// Convenience: load everything at once (for CLI list mode).
pub fn discover_sessions() -> Vec<Session> {
    let (files, alive_ids) = collect_session_files();
    let mut sessions = parse_batch(&files, &alive_ids);
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    sessions
}
