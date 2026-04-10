# Séance

Browse, search, and resume Claude Code sessions from the terminal.

Séance reads the session journals Claude Code writes to `~/.claude/` and presents them in a searchable TUI. Select any session — alive or dead — and resume it in a new Ghostty tab or the current terminal.

## Install

**Homebrew**

```
brew install chilang/tap/seance
```

**Cargo**

```
cargo install --git https://github.com/chilang/seance
```

**From source**

```
git clone https://github.com/chilang/seance && cd seance
cargo build --release
cp target/release/seance /usr/local/bin/
```

## Usage

```
seance          # interactive TUI
seance list     # plain-text output
```

| Key | Action |
|-----|--------|
| `↑↓` `jk` | Navigate |
| `Enter` | Resume in new Ghostty tab |
| `r` | Resume in current terminal |
| `p` `Space` | Preview all user prompts |
| `/` | Search across project names and prompts |
| `y` | Copy resume command to clipboard |
| `a` `d` | Filter alive / dead |
| `q` | Quit |

## How It Works

1. Scans `~/.claude/projects/` for `.jsonl` session files
2. Parses user messages, strips system tags, extracts commands
3. Cross-references `~/.claude/sessions/` PIDs for alive/dead status
4. Loads recent files first; the rest parse in a background thread

Resume runs `claude -r <session-id>` from the session's original working directory.

## Requirements

- Claude Code CLI in PATH
- macOS for Ghostty tab integration (resume-in-terminal works on any platform)

## License

MIT
