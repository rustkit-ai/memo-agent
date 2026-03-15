# memo

Persistent memory for AI coding agents. Single Rust binary, zero runtime dependencies.

Works with Claude Code, Cursor, Aider, and any agent that can run shell commands.

## The problem

Every new AI session starts from zero. The agent re-explores project structure, re-discovers conventions, re-learns what was done last time. `memo` fixes this by injecting a compact context block at session start — written by the agent, read next time.

## Workflow

```
# Start of session
memo inject

# During session — log important decisions
memo log "switched to WAL mode for SQLite, fixes concurrent write issue"
memo log "refactored auth middleware" --tag refactor

# End of session
memo log "todo: fix token refresh in utils.rs:42"
```

**inject output (~80 tokens):**
```
## memo context
last: 2026-03-14 — "refactored auth, broke token refresh"
todo: fix utils.rs:42 — token refresh logic
recent tags: bug · refactor · auth
```

## Install

**cargo:**
```sh
cargo install memo
```

**curl (Linux/macOS):**
```sh
curl -fsSL https://github.com/rustkit-ai/memo/releases/latest/download/install.sh | sh
```

**brew:**
```sh
brew install rustkit-ai/tap/memo
```

## Commands

```
memo init                   # initialize project memory
memo log <message>          # save a memory entry
memo log <message> --tag X  # with optional tag
memo inject                 # print compact context block (stdout)
memo inject --claude        # write block directly into CLAUDE.md
memo inject --format json   # JSON output for programmatic use
memo list                   # show last 10 entries
memo list --all             # show all entries
memo clear                  # clear all memory for current project
memo stats                  # entry count, estimated tokens saved
```

## Storage

SQLite at `~/.local/share/memo/<project-hash>.db`. Project identified by git remote URL (fallback: absolute path). No config files, no daemons.

## Add to CLAUDE.md

```markdown
## Agent instructions
- Run `memo inject` at the start of every session
- Run `memo log "<what you did>"` when finishing a task
- Run `memo log "todo: <next step>"` before ending the session
```

Or let memo write it: `memo inject --claude`

## License

MIT
