# aimemo × Windsurf

Windsurf reads `.windsurfrules` automatically in every session. `aimemo setup` writes instructions into that file telling Windsurf to maintain the memory loop: inject context at session start, log after file edits, recap at session end.

---

## Setup

Run once in your project root:

```sh
aimemo setup --windsurf
```

Then bootstrap from your git history so the agent has context from day one:

```sh
aimemo bootstrap
```

---

## What gets written

**`.windsurfrules`**:

```markdown
# aimemo — persistent memory
- At session start: run `aimemo inject --windsurf` to load context from previous sessions
- After modifying any file: run `aimemo log "modified {filename}: {one-line reason}"`
- When you identify something to fix later: run `aimemo log "todo: {description}"`
- At session end: run `aimemo recap "{what was done} — next: {what comes next}"` then `aimemo inject --windsurf`

<!-- aimemo:start -->
## aimemo context
last: (no entries yet)
<!-- aimemo:end -->
```

Windsurf loads `.windsurfrules` automatically — no additional configuration needed.

---

## The session loop

```
Open Windsurf
      │
      ▼
Windsurf reads .windsurfrules
      │
      ▼
Agent runs: aimemo inject --windsurf
      │  (updates the context block in .windsurfrules)
      ▼
Agent reads the context — knows where it left off
      │
      ▼
You work — agent logs after each file edit:
  aimemo log "modified src/db/migrate.rs: added pg16 migration"
  aimemo log "todo: update connection pool config for pg16 defaults"
      │
      ▼
At session end:
  aimemo recap "migrated DB to PostgreSQL 16 — next: update connection pool config"
  aimemo inject --windsurf
      │
      ▼
Next session starts with full context
```

---

## What the context block looks like

```
## aimemo context
recap (2026-03-15): "migrated DB to PostgreSQL 16 — next: update connection pool config"
recent (2026-03-15): "modified src/db/migrate.rs: added pg16 migration"
recent (2026-03-15): "modified src/db/pool.rs: extracted pool config"
todo: update connection pool config for pg16 defaults
recent tags: db · migration
```

---

## Example session

```
You: [opens Windsurf, starts a new session]

Windsurf: Based on aimemo — last session you migrated the database to PostgreSQL 16.
          Open todo: update the connection pool config for pg16 defaults.
          Want to tackle that now?
```

---

## Key commands

```sh
aimemo recap "<summary>"    # log end-of-session summary
aimemo todo list            # see all open todos
aimemo todo done <id>       # mark a todo as done
aimemo bootstrap            # import recent git commits as memory entries
aimemo inject --windsurf    # manually update .windsurfrules
aimemo inject --all         # update all configured agent files at once
```

---

## Verify setup

```sh
cat .windsurfrules
```

You should see the instructions and the `<!-- aimemo:start -->` block.
