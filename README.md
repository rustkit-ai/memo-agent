# memo — Persistent memory for AI coding agents

**Stop starting every AI session from zero.**

`memo` gives AI agents like Claude Code, Cursor, and Aider a persistent memory across sessions. One binary, zero dependencies, works in any project.

```
$ memo inject

## memo context
last: 2026-03-15 — "refactored auth middleware, JWT now stateless"
todo: fix token refresh in utils.rs:42
recent tags: refactor · auth · bug
```

---

## The problem

Every new AI session starts from scratch. The agent re-reads files it already read, re-discovers conventions it already learned, asks questions it already asked. On a large codebase this costs hundreds of tokens and minutes of context-building — every single time.

`memo` fixes this with a compact context block (~80 tokens) injected at session start. Written by the agent, read next time.

---

## Install

**cargo** (recommended):
```sh
cargo install memo-agent
```

**curl** (Linux / macOS):
```sh
curl -fsSL https://github.com/rustkit-ai/memo/releases/latest/download/install.sh | sh
```

**brew**:
```sh
brew install rustkit-ai/tap/memo
```

---

## How a session works with Claude Code

### 1. One-time setup

Run this once in your project:

```sh
memo setup
```

This does two things:
- Writes agent instructions into `CLAUDE.md` so Claude knows to use memo
- Installs a **Stop hook** in `.claude/settings.json` that runs `memo inject --claude` automatically when Claude finishes a session

You never have to think about it again.

---

### 2. Work with Claude normally

Open Claude Code and work as usual — ask questions, implement features, fix bugs. Claude will log what it does using `memo log` as it goes, following the instructions in `CLAUDE.md`.

```
You: implement the password reset flow

Claude: [works on the feature]
        memo log "implemented password reset: email token, 1h expiry, bcrypt hash"
        memo log "todo: add rate limiting on /reset endpoint"
```

---

### 3. Session ends — CLAUDE.md updates automatically

When you close Claude Code, the Stop hook fires and runs `memo inject --claude`. Your `CLAUDE.md` is updated with a fresh context block:

```markdown
<!-- memo:start -->
## memo context
last: 2026-03-15 — "implemented password reset: email token, 1h expiry, bcrypt hash"
todo: add rate limiting on /reset endpoint
recent tags: auth · security · todo
<!-- memo:end -->
```

No manual steps. No copy-pasting. It just happens.

---

### 4. Next session — Claude knows where it left off

You open Claude Code the next day. Claude reads `CLAUDE.md` automatically at startup and immediately knows:

- What was done last session
- What's next on the todo list
- What areas of the codebase were touched

```
You: what did we do last time?

Claude: Based on memo — you implemented password reset with an email token,
        1h expiry, and bcrypt hash. Still need to add rate limiting on /reset.
```

No re-exploration. No repeated questions. Claude picks up exactly where it left off.

---

## The full loop

```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│   memo setup          ← run once                       │
│        │                                               │
│        ▼                                               │
│   Open Claude Code                                     │
│        │                                               │
│        ▼                                               │
│   Claude reads CLAUDE.md  ←── context from last time   │
│        │                                               │
│        ▼                                               │
│   Work: tasks, fixes, features                         │
│        │                                               │
│        ▼                                               │
│   Claude logs: memo log "..."                          │
│        │                                               │
│        ▼                                               │
│   Session ends → hook fires → CLAUDE.md updated ───┐  │
│                                                     │  │
│   Next session ◄────────────────────────────────────┘  │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## With any other agent

Add this to your `CLAUDE.md` / `AGENTS.md` / system prompt:

```markdown
## Memory
- Run `memo inject` at the start of every session
- Run `memo log "<what you did>"` after each significant task
- Run `memo log "todo: <next step>"` before ending the session
```

---

## Commands

| Command | Description |
|---|---|
| `memo setup` | One-time Claude Code integration (CLAUDE.md + Stop hook) |
| `memo init` | Initialize project memory |
| `memo log "<message>"` | Save a memory entry |
| `memo log "<message>" --tag refactor` | Save with one or more tags |
| `memo log -` | Read message from stdin |
| `memo inject` | Print context block to stdout |
| `memo inject --claude` | Write context block into CLAUDE.md |
| `memo inject --since 7d` | Limit context to last 7 days |
| `memo inject --format json` | JSON output for programmatic use |
| `memo list` | Show last 10 entries |
| `memo list --all` | Show all entries |
| `memo list --tag bug` | Filter by tag |
| `memo search <query>` | Full-text search entries |
| `memo delete <id>` | Delete a specific entry |
| `memo tags` | List all tags with usage counts |
| `memo stats` | Entry count + token savings estimate |
| `memo clear` | Clear all memory for current project |

---

## Why not just use CLAUDE.md?

You can write to `CLAUDE.md` manually — but that means **you** do the work. `memo` lets the agent maintain its own memory, automatically, without any human intervention between sessions.

---

## License

MIT — [rustkit-ai/memo](https://github.com/rustkit-ai/memo)
