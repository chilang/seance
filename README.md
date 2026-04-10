# Séance

A terminal UI for browsing, searching, and resuming Claude Code sessions.

Séance reads the session journal files Claude Code writes to `~/.claude/` and presents them in a navigable, searchable interface. Select any session — alive or dead — and resume it in a new Ghostty tab or the current terminal.

## Install

```
cargo install --path .
```

Or build and symlink:

```
cargo build --release
ln -sf $(pwd)/target/release/seance ~/.local/bin/seance
```

## Usage

```
seance              # interactive TUI
seance list         # plain-text session list
```

### Keys

| Key | Action |
|-----|--------|
| `↑↓` `jk` | Navigate |
| `Enter` | Resume in new Ghostty tab |
| `r` | Resume in current terminal |
| `p` `Space` | Preview all prompts |
| `/` | Search (filters across all prompts) |
| `y` | Copy resume command to clipboard |
| `a` | Toggle alive only |
| `d` | Toggle dead only |
| `g` `G` | Top / bottom |
| `q` | Quit |

### Preview

Press `p` to open a full-screen view of every user prompt in the selected session. Scroll with `↑↓`, close with `Esc`.

## How It Works

1. Scans `~/.claude/projects/` for session `.jsonl` files
2. Parses user messages, strips system tags, extracts slash commands
3. Cross-references `~/.claude/sessions/` PIDs to determine alive/dead status
4. Loads the 60 most recent files first; remaining files parse in a background thread

Resume runs `claude -r <session-id>` from the session's original working directory.

## Requirements

- macOS (Ghostty tab integration uses AppleScript; resume-in-terminal works anywhere)
- Rust 1.70+
- Claude Code CLI (`claude`) in PATH
