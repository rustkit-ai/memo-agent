# aimemo × Claude Code

Claude Code reads `CLAUDE.md` automatically at the start of every session. `aimemo setup` installs **three hooks** and writes a context block into `CLAUDE.md` — the full memory loop runs with zero manual steps, ever.

---

## Setup

Run once in your project root:

```sh
aimemo setup --claude
```

Then bootstrap from your git history so the agent has context from day one:

```sh
aimemo bootstrap
```

---

## What gets installed

**Three hooks in `.claude/settings.json`:**

| Hook | Trigger | What it does |
|---|---|---|
| `PostToolUse` | After every Write / Edit / MultiEdit | Runs `aimemo capture` — auto-logs the file with a code description |
| `UserPromptSubmit` | At the start of each session | Runs `aimemo inject --claude --once` — injects fresh context |
| `Stop` | When you close Claude Code | Runs `aimemo inject --claude` — saves context for next session |

**`.claude/settings.json`** (excerpt):

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit|MultiEdit",
        "hooks": [{ "type": "command", "command": "aimemo capture" }]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [{ "type": "command", "command": "aimemo inject --claude --once" }]
      }
    ],
    "Stop": [
      {
        "hooks": [{ "type": "command", "command": "aimemo inject --claude" }]
      }
    ]
  }
}
```

**`CLAUDE.md`** (excerpt):

```markdown
<!-- aimemo:instructions:start -->
## aimemo — persistent memory
- At session start: run `aimemo inject --claude` to load context from previous sessions
- After modifying any file: run `aimemo log "modified {filename}: {one-line reason}"`
- When you identify something to fix later: run `aimemo log "todo: {description}"`
- At session end: run `aimemo recap "{what was done} — next: {what comes next}"` then `aimemo inject --claude`
<!-- aimemo:instructions:end -->

<!-- aimemo:start -->
## aimemo context
last: (no entries yet)
<!-- aimemo:end -->
```

---

## The session loop

```
Open Claude Code
      │
      ▼
UserPromptSubmit hook → aimemo inject --claude --once
      │  (injects context only if new entries exist)
      ▼
Claude reads CLAUDE.md ←── recap + recent entries + open todos
      │
      ▼
You work — Claude edits files
      │
      ▼
PostToolUse hook → aimemo capture
      │  (logs "wrote src/auth.rs: added fn handle_login"
      │   or  "edited src/db/pool.rs: added fn connect_pool"
      │   or  "edited src/auth.rs (3 changes)" if no pattern matched)
      ▼
Claude logs semantic context:
  aimemo log "modified src/auth.rs: extracted JWT validation"
  aimemo log "todo: add refresh token endpoint"
      │
      ▼
At session end:
  aimemo recap "implemented JWT auth — next: refresh token endpoint"
      │
      ▼
You close Claude Code
      │
      ▼
Stop hook → aimemo inject --claude
      │
      ▼
CLAUDE.md updated silently — ready for next session
```

---

## What the context block looks like

```
## aimemo context
recap (2026-03-15): "implemented JWT auth — next: refresh token endpoint"
recent (2026-03-15): "wrote src/auth/jwt.rs: added fn validate_token"
recent (2026-03-15): "edited src/auth/jwt.rs: added fn refresh_token"
recent (2026-03-15): "modified src/auth/jwt.rs: extracted JWT validation"
todo: add refresh token endpoint
recent tags: auth · jwt · auto
```

---

## Example session

```
You: where did we leave off?

Claude: Based on aimemo — last session you implemented JWT auth.
        The recap says: "next: refresh token endpoint".
        There's an open todo for that. Should I start there?
```

---

## Key commands

```sh
aimemo recap "<summary>"    # log end-of-session summary (shown prominently next session)
aimemo todo list            # see all open todos
aimemo todo done <id>       # mark a todo as done
aimemo bootstrap            # import recent git commits as memory entries
aimemo inject --claude      # manually update CLAUDE.md
aimemo doctor               # check hooks, DB, and all agent config files
```

---

## Verify setup

```sh
aimemo doctor
```

Example output on a healthy project:

```
Core
  ✓ binary: /usr/local/bin/aimemo
  ✓ database: ~/.local/share/aimemo/abc12345.db (42 entries)

Claude Code
  ✓ CLAUDE.md: aimemo context block present
  ✓ hook Stop: aimemo inject --claude
  ✓ hook UserPromptSubmit: aimemo inject --claude --once
  ✓ hook PostToolUse: aimemo capture (Write|Edit|MultiEdit)

Cursor
  ✓ .cursor/rules/aimemo.mdc: alwaysApply: true
  ✓ .cursor/rules/aimemo.mdc: aimemo context block present

All checks passed.
```

If anything is missing, run `aimemo setup` again — it is idempotent.
