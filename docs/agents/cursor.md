# aimemo × Cursor

Cursor reads rules from `.cursor/rules/` automatically. `aimemo setup` writes a rule file with `alwaysApply: true` that instructs Cursor to maintain the memory loop: inject context at session start, log after file edits, recap at session end.

---

## Setup

Run once in your project root:

```sh
aimemo setup --cursor
```

Then bootstrap from your git history so the agent has context from day one:

```sh
aimemo bootstrap
```

---

## What gets written

**`.cursor/rules/aimemo.mdc`**:

```markdown
---
description: aimemo persistent memory instructions
alwaysApply: true
---

## aimemo — persistent memory
- At session start: run `aimemo inject --cursor` to load context from previous sessions
- After modifying any file: run `aimemo log "modified {filename}: {one-line reason}"`
- When you identify something to fix later: run `aimemo log "todo: {description}"`
- At session end: run `aimemo recap "{what was done} — next: {what comes next}"` then `aimemo inject --cursor`

<!-- aimemo:start -->
## aimemo context
last: (no entries yet)
<!-- aimemo:end -->
```

Because `alwaysApply: true` is set, Cursor loads this rule file in every chat and Composer session automatically.

---

## The session loop

```
Open Cursor
      │
      ▼
Cursor loads .cursor/rules/aimemo.mdc  (alwaysApply: true)
      │
      ▼
Agent runs: aimemo inject --cursor
      │  (updates the context block in aimemo.mdc)
      ▼
Agent reads the context — knows where it left off
      │
      ▼
You work — agent logs after each file edit:
  aimemo log "modified src/payments/service.rs: added idempotency keys"
  aimemo log "todo: write integration test for duplicate charge case"
      │
      ▼
At session end:
  aimemo recap "added idempotency to payment service — next: integration tests"
  aimemo inject --cursor
      │
      ▼
Next session starts with full context
```

---

## What the context block looks like

```
## aimemo context
recap (2026-03-15): "added idempotency to payment service — next: integration tests"
recent (2026-03-15): "modified src/payments/service.rs: added idempotency keys"
todo: write integration test for duplicate charge case
recent tags: payments · idempotency
```

---

## Example session

```
You: [opens Cursor, starts a new chat]

Cursor: Based on aimemo — last session you added idempotency keys to the payment
        service. Open todo: write an integration test for the duplicate charge
        case. Should I start there?
```

---

## Key commands

```sh
aimemo recap "<summary>"    # log end-of-session summary
aimemo todo list            # see all open todos
aimemo todo done <id>       # mark a todo as done
aimemo bootstrap            # import recent git commits as memory entries
aimemo inject --cursor      # manually update .cursor/rules/aimemo.mdc
aimemo inject --all         # update all configured agent files at once
```

---

## Verify setup

```sh
cat .cursor/rules/aimemo.mdc
```

You should see the `alwaysApply: true` frontmatter and the `<!-- aimemo:start -->` block.
