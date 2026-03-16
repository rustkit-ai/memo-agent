# aimemo × GitHub Copilot / VS Code

GitHub Copilot reads `.github/copilot-instructions.md` automatically in VS Code and JetBrains IDEs. `aimemo setup` writes instructions into that file telling Copilot to maintain the memory loop: inject context at session start, log after file edits, recap at session end.

> **VS Code users:** use `aimemo inject --vscode` — it writes to the same file.

---

## Setup

Run once in your project root:

```sh
aimemo setup --copilot
```

Then bootstrap from your git history so the agent has context from day one:

```sh
aimemo bootstrap
```

---

## What gets written

**`.github/copilot-instructions.md`** (appended section):

```markdown
## aimemo — persistent memory
- At session start: run `aimemo inject --copilot` to load context from previous sessions
- After modifying any file: run `aimemo log "modified {filename}: {one-line reason}"`
- When you identify something to fix later: run `aimemo log "todo: {description}"`
- At session end: run `aimemo recap "{what was done} — next: {what comes next}"` then `aimemo inject --copilot`

<!-- aimemo:start -->
## aimemo context
last: (no entries yet)
<!-- aimemo:end -->
```

> If a `copilot-instructions.md` already exists, `aimemo setup` appends the block instead of overwriting the file.

---

## The session loop

```
Open VS Code / JetBrains with Copilot
      │
      ▼
Copilot reads .github/copilot-instructions.md
      │
      ▼
Agent runs: aimemo inject --copilot
      │  (updates the context block in copilot-instructions.md)
      ▼
Agent reads the context — knows where it left off
      │
      ▼
You work — agent logs after each file edit:
  aimemo log "modified src/components/Button.tsx: extracted shared component"
  aimemo log "todo: update Storybook stories for Button"
      │
      ▼
At session end:
  aimemo recap "extracted Button component, replaced 12 usages — next: Storybook"
  aimemo inject --copilot
      │
      ▼
Next session starts with full context
```

---

## What the context block looks like

```
## aimemo context
recap (2026-03-15): "extracted Button component, replaced 12 usages — next: Storybook"
recent (2026-03-15): "modified src/components/Button.tsx: extracted shared component"
todo: update Storybook stories for Button
recent tags: components · refactor
```

---

## Example session

```
You: [opens Copilot Chat]

Copilot: Based on aimemo — last session you extracted a shared Button component
         and replaced 12 inline usages. Open todo: update the Storybook stories
         for Button. Should I help with that?
```

---

## Key commands

```sh
aimemo recap "<summary>"    # log end-of-session summary
aimemo todo list            # see all open todos
aimemo todo done <id>       # mark a todo as done
aimemo bootstrap            # import recent git commits as memory entries
aimemo inject --copilot     # manually update .github/copilot-instructions.md
aimemo inject --vscode      # same — alias for VS Code users
aimemo inject --all         # update all configured agent files at once
```

---

## Enable Copilot instructions in VS Code

Make sure this setting is enabled:

```json
{
  "github.copilot.chat.codeGeneration.useInstructionFiles": true
}
```

Or via the UI: **Settings → GitHub Copilot → Chat: Use Instruction Files**.

---

## Verify setup

```sh
cat .github/copilot-instructions.md
```

You should see the instructions section and the `<!-- aimemo:start -->` block.
