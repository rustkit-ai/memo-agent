use anyhow::Result;
use chrono::{DateTime, Utc};
use crate::store::{Entry, Store};
use std::fs;
use std::path::Path;

pub struct InjectBlock {
    pub last_entries: Vec<Entry>,    // last 3 non-todo, non-recap entries
    pub open_todos: Vec<Entry>,      // ALL open todos
    pub recent_tags: Vec<String>,
    pub entry_count: usize,
    pub last_recap: Option<Entry>,   // most recent recap entry
    pub pinned_entries: Vec<Entry>,
}

impl InjectBlock {
    pub fn build(store: &Store) -> Result<Self> {
        Self::from_store(store, store.list(Some(20))?)
    }

    pub fn build_since(store: &Store, since: DateTime<Utc>) -> Result<Self> {
        Self::from_store(store, store.list_since(since, Some(20))?)
    }

    fn from_store(store: &Store, entries: Vec<Entry>) -> Result<Self> {
        let last_entries = entries
            .iter()
            .filter(|e| {
                let lower = e.content.to_ascii_lowercase();
                !lower.starts_with("todo:") && !lower.starts_with("recap:")
            })
            .take(3)
            .cloned()
            .collect();

        Ok(Self {
            last_entries,
            open_todos: store.list_open_todos()?,
            recent_tags: store.recent_tags(10)?,
            entry_count: store.count()?,
            last_recap: store.last_recap()?,
            pinned_entries: store.list_pinned()?,
        })
    }

    pub fn render_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::from("## memo context\n");

        // Pinned entries always shown first
        for e in &self.pinned_entries {
            writeln!(out, "pinned: \"{}\"", e.content).unwrap();
        }

        if let Some(recap) = &self.last_recap {
            let text = recap.content
                .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ':')
                .trim();
            writeln!(out, "recap ({}): \"{}\"", recap.timestamp.format("%Y-%m-%d"), text).unwrap();
        }

        if self.last_entries.is_empty() && self.last_recap.is_none() && self.pinned_entries.is_empty() {
            out.push_str("last: (no entries yet)\n");
        } else {
            for e in &self.last_entries {
                writeln!(out, "recent ({}): \"{}\"", e.timestamp.format("%Y-%m-%d"), e.content).unwrap();
            }
        }

        for todo in &self.open_todos {
            let text = todo.content
                .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ':')
                .trim();
            writeln!(out, "todo: {text}").unwrap();
        }

        if !self.recent_tags.is_empty() {
            writeln!(out, "recent tags: {}", self.recent_tags.join(" · ")).unwrap();
        }

        out
    }

    pub fn render_json(&self) -> Result<String> {
        let value = serde_json::json!({
            "last_recap": self.last_recap.as_ref().map(|e| serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
            })),
            "last_entries": self.last_entries.iter().map(|e| serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
                "tags": e.tags,
            })).collect::<Vec<_>>(),
            "open_todos": self.open_todos.iter().map(|e| serde_json::json!({
                "id": e.id,
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
            })).collect::<Vec<_>>(),
            "recent_tags": self.recent_tags,
            "entry_count": self.entry_count,
            "pinned_entries": self.pinned_entries.iter().map(|e| serde_json::json!({
                "id": e.id,
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
                "tags": e.tags,
            })).collect::<Vec<_>>(),
        });
        Ok(serde_json::to_string_pretty(&value)?)
    }

    fn empty() -> Self {
        Self {
            last_entries: vec![],
            open_todos: vec![],
            recent_tags: vec![],
            entry_count: 0,
            last_recap: None,
            pinned_entries: vec![],
        }
    }
}

pub struct SetupResult {
    pub claude_hook_installed: bool,
    pub cursor_rules_written: bool,
    pub windsurf_rules_written: bool,
    pub copilot_instructions_written: bool,
    pub start_hook_installed: bool,
    pub post_tool_hook_installed: bool,
}

pub fn setup(project_dir: &Path) -> Result<SetupResult> {
    // Claude Code
    write_instructions_to_claude_md(project_dir)?;
    write_to_claude_md(&InjectBlock::empty(), project_dir)?;
    let claude_hook_installed = install_stop_hook(project_dir)?;
    let start_hook_installed = install_start_hook(project_dir)?;
    let post_tool_hook_installed = install_post_tool_hook(project_dir)?;

    // Cursor
    let cursor_rules_written = write_cursor_rules(project_dir)?;
    if cursor_rules_written {
        write_to_cursor_rules(&InjectBlock::empty(), project_dir)?;
    }

    // Windsurf
    let windsurf_rules_written = write_windsurf_rules(project_dir)?;
    if windsurf_rules_written {
        write_to_windsurf_rules(&InjectBlock::empty(), project_dir)?;
    }

    // GitHub Copilot
    let copilot_instructions_written = write_copilot_instructions(project_dir)?;
    if copilot_instructions_written {
        write_to_copilot_instructions(&InjectBlock::empty(), project_dir)?;
    }

    Ok(SetupResult {
        claude_hook_installed,
        cursor_rules_written,
        windsurf_rules_written,
        copilot_instructions_written,
        start_hook_installed,
        post_tool_hook_installed,
    })
}

pub fn write_to_claude_md(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    patch_markdown_section(
        &project_dir.join("CLAUDE.md"),
        "<!-- memo:start -->",
        "<!-- memo:end -->",
        &block.render_text(),
    )
}

pub fn write_to_cursor_rules(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    patch_markdown_section(
        &project_dir.join(".cursor").join("rules").join("memo.mdc"),
        "<!-- memo:start -->",
        "<!-- memo:end -->",
        &block.render_text(),
    )
}

pub fn write_to_windsurf_rules(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    patch_markdown_section(
        &project_dir.join(".windsurfrules"),
        "<!-- memo:start -->",
        "<!-- memo:end -->",
        &block.render_text(),
    )
}

pub fn write_to_copilot_instructions(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    patch_markdown_section(
        &project_dir.join(".github").join("copilot-instructions.md"),
        "<!-- memo:start -->",
        "<!-- memo:end -->",
        &block.render_text(),
    )
}

pub fn write_to_vscode(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    write_to_copilot_instructions(block, project_dir)
}

/// Inject context into all configured agent files found in project_dir.
/// Each file is only updated if it already exists and contains a memo block marker.
/// Returns the list of files updated.
pub fn inject_all(block: &InjectBlock, project_dir: &Path) -> Result<Vec<String>> {
    let mut updated = Vec::new();

    let claude_md = project_dir.join("CLAUDE.md");
    if claude_md.exists() {
        let content = fs::read_to_string(&claude_md).unwrap_or_default();
        if content.contains("<!-- memo:start -->") {
            write_to_claude_md(block, project_dir)?;
            updated.push("CLAUDE.md".to_string());
        }
    }

    let cursor_rules = project_dir.join(".cursor").join("rules").join("memo.mdc");
    if cursor_rules.exists() {
        write_to_cursor_rules(block, project_dir)?;
        updated.push(".cursor/rules/memo.mdc".to_string());
    }

    let windsurf = project_dir.join(".windsurfrules");
    if windsurf.exists() {
        write_to_windsurf_rules(block, project_dir)?;
        updated.push(".windsurfrules".to_string());
    }

    let copilot = project_dir.join(".github").join("copilot-instructions.md");
    if copilot.exists() {
        let content = fs::read_to_string(&copilot).unwrap_or_default();
        if content.contains("<!-- memo:start -->") {
            write_to_copilot_instructions(block, project_dir)?;
            updated.push(".github/copilot-instructions.md".to_string());
        }
    }

    Ok(updated)
}

const CLAUDE_INSTRUCTIONS: &str = "\
- At session start: run `memo inject --claude` to load context from previous sessions\n\
- After modifying any file: run `memo log \"modified {filename}: {one-line reason}\"`\n\
- When you identify something to fix later: run `memo log \"todo: {description}\"`\n\
- At session end: run `memo recap \"{what was done} — next: {what comes next}\"` then `memo inject --claude`\n";

const CURSOR_INSTRUCTIONS: &str = "\
- At session start: run `memo inject --cursor` to load context from previous sessions\n\
- After modifying any file: run `memo log \"modified {filename}: {one-line reason}\"`\n\
- When you identify something to fix later: run `memo log \"todo: {description}\"`\n\
- At session end: run `memo recap \"{what was done} — next: {what comes next}\"` then `memo inject --cursor`\n";

const WINDSURF_INSTRUCTIONS: &str = "\
- At session start: run `memo inject --windsurf` to load context from previous sessions\n\
- After modifying any file: run `memo log \"modified {filename}: {one-line reason}\"`\n\
- When you identify something to fix later: run `memo log \"todo: {description}\"`\n\
- At session end: run `memo recap \"{what was done} — next: {what comes next}\"` then `memo inject --windsurf`\n";

const COPILOT_INSTRUCTIONS: &str = "\
- At session start: run `memo inject --copilot` to load context from previous sessions\n\
- After modifying any file: run `memo log \"modified {filename}: {one-line reason}\"`\n\
- When you identify something to fix later: run `memo log \"todo: {description}\"`\n\
- At session end: run `memo recap \"{what was done} — next: {what comes next}\"` then `memo inject --copilot`\n";

fn write_cursor_rules(project_dir: &Path) -> Result<bool> {
    let rules_dir = project_dir.join(".cursor").join("rules");
    let rules_path = rules_dir.join("memo.mdc");

    if rules_path.exists() {
        return Ok(false);
    }

    fs::create_dir_all(&rules_dir)?;
    fs::write(
        &rules_path,
        format!(
            "---\ndescription: memo persistent memory instructions\nalwaysApply: true\n---\n\n\
             ## memo — persistent agent memory\n{CURSOR_INSTRUCTIONS}"
        ),
    )?;
    Ok(true)
}

fn write_windsurf_rules(project_dir: &Path) -> Result<bool> {
    let path = project_dir.join(".windsurfrules");
    if path.exists() {
        return Ok(false);
    }
    fs::write(
        &path,
        format!("# memo — persistent agent memory\n{WINDSURF_INSTRUCTIONS}"),
    )?;
    Ok(true)
}

fn write_copilot_instructions(project_dir: &Path) -> Result<bool> {
    let github_dir = project_dir.join(".github");
    let path = github_dir.join("copilot-instructions.md");
    fs::create_dir_all(&github_dir)?;

    let header = "## memo — persistent agent memory";
    let block = format!("{header}\n{COPILOT_INSTRUCTIONS}");

    if path.exists() {
        let existing = fs::read_to_string(&path)?;
        if existing.contains(header) {
            return Ok(false);
        }
        fs::write(&path, format!("{}\n\n{}", existing.trim_end(), block))?;
    } else {
        fs::write(&path, block)?;
    }
    Ok(true)
}

fn write_instructions_to_claude_md(project_dir: &Path) -> Result<()> {
    patch_markdown_section(
        &project_dir.join("CLAUDE.md"),
        "<!-- memo:instructions:start -->",
        "<!-- memo:instructions:end -->",
        &format!("## memo — persistent agent memory\n{CLAUDE_INSTRUCTIONS}"),
    )
}

/// Replace or prepend a delimited section in a Markdown file.
/// The section is identified by `start` and `end` HTML comment markers.
/// If the file doesn't exist it is created. If the section doesn't exist it is prepended.
///
/// Slice arithmetic: `find(end).map(|i| i + end.len())` advances the end pointer past
/// the closing marker itself, so the replacement section fully replaces start..end inclusive.
fn patch_markdown_section(path: &Path, start: &str, end: &str, content: &str) -> Result<()> {
    let existing = if path.exists() { fs::read_to_string(path)? } else { String::new() };
    let section = format!("{start}\n{content}{end}\n");

    let new_content = if let Some(s) = existing.find(start) {
        // Advance past the end marker so we replace the whole block including the marker.
        let e = existing.find(end).map(|i| i + end.len()).unwrap_or(existing.len());
        format!("{}{}{}", &existing[..s], section, &existing[e..])
    } else {
        format!("{section}\n{existing}")
    };

    fs::write(path, new_content)?;
    Ok(())
}

/// Shared helper: read settings.json, check if already installed via `check_fn`,
/// apply `apply_fn` to insert the new hook value, then write back.
fn install_hook(
    project_dir: &Path,
    check_fn: impl Fn(&serde_json::Value) -> bool,
    apply_fn: impl Fn(&mut serde_json::Value),
) -> Result<bool> {
    let claude_dir = project_dir.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.json");

    let mut root: serde_json::Value = if settings_path.exists() {
        serde_json::from_str(&fs::read_to_string(&settings_path)?).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if check_fn(&root) {
        return Ok(false);
    }

    apply_fn(&mut root);

    fs::write(&settings_path, serde_json::to_string_pretty(&root)?)?;
    Ok(true)
}

fn install_stop_hook(project_dir: &Path) -> Result<bool> {
    install_hook(
        project_dir,
        |root| {
            root.get("hooks")
                .and_then(|h| h.get("Stop"))
                .and_then(|s| s.as_array())
                .is_some_and(|stop_hooks| {
                    stop_hooks.iter().any(|h| {
                        h.get("hooks")
                            .and_then(|hs| hs.as_array())
                            .is_some_and(|hs| {
                                hs.iter().any(|cmd| {
                                    cmd.get("command")
                                        .and_then(|c| c.as_str())
                                        .is_some_and(|s| s.contains("memo inject"))
                                })
                            })
                    })
                })
        },
        |root| {
            let memo_hook = serde_json::json!({
                "hooks": [{ "type": "command", "command": "memo inject --claude" }]
            });
            match root["hooks"]["Stop"].as_array_mut() {
                Some(arr) => arr.push(memo_hook),
                None => root["hooks"]["Stop"] = serde_json::json!([memo_hook]),
            }
        },
    )
}

fn install_post_tool_hook(project_dir: &Path) -> Result<bool> {
    install_hook(
        project_dir,
        |root| {
            root.get("hooks")
                .and_then(|h| h.get("PostToolUse"))
                .and_then(|s| s.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("hooks")
                            .and_then(|hs| hs.as_array())
                            .is_some_and(|hs| {
                                hs.iter().any(|cmd| {
                                    cmd.get("command")
                                        .and_then(|c| c.as_str())
                                        .is_some_and(|s| s.contains("memo capture"))
                                })
                            })
                    })
                })
        },
        |root| {
            // Only capture Write, Edit, MultiEdit tool calls
            let capture_hook = serde_json::json!({
                "matcher": "Write|Edit|MultiEdit",
                "hooks": [{ "type": "command", "command": "memo capture" }]
            });
            match root["hooks"]["PostToolUse"].as_array_mut() {
                Some(arr) => arr.push(capture_hook),
                None => root["hooks"]["PostToolUse"] = serde_json::json!([capture_hook]),
            }
        },
    )
}

fn install_start_hook(project_dir: &Path) -> Result<bool> {
    install_hook(
        project_dir,
        |root| {
            root.get("hooks")
                .and_then(|h| h.get("UserPromptSubmit"))
                .and_then(|s| s.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("hooks")
                            .and_then(|hs| hs.as_array())
                            .is_some_and(|hs| {
                                hs.iter().any(|cmd| {
                                    cmd.get("command")
                                        .and_then(|c| c.as_str())
                                        .is_some_and(|s| s.contains("memo inject") && s.contains("--once"))
                                })
                            })
                    })
                })
        },
        |root| {
            let memo_hook = serde_json::json!({
                "hooks": [{ "type": "command", "command": "memo inject --claude --once" }]
            });
            match root["hooks"]["UserPromptSubmit"].as_array_mut() {
                Some(arr) => arr.push(memo_hook),
                None => root["hooks"]["UserPromptSubmit"] = serde_json::json!([memo_hook]),
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_render_text_empty() {
        let block = InjectBlock::empty();
        let text = block.render_text();
        assert!(text.contains("## memo context"));
        assert!(text.contains("no entries yet"));
    }

    #[test]
    fn test_render_json() {
        let block = InjectBlock {
            last_entries: vec![],
            open_todos: vec![],
            recent_tags: vec!["bug".to_string()],
            entry_count: 5,
            last_recap: None,
            pinned_entries: vec![],
        };
        let val: serde_json::Value = serde_json::from_str(&block.render_json().unwrap()).unwrap();
        assert_eq!(val["entry_count"], 5);
        assert_eq!(val["recent_tags"][0], "bug");
    }

    #[test]
    fn test_patch_markdown_section_create_and_replace() {
        let dir = env::temp_dir().join(format!("memo_hooks_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("CLAUDE.md");

        patch_markdown_section(&path, "<!-- s -->", "<!-- e -->", "content\n").unwrap();
        let c = fs::read_to_string(&path).unwrap();
        assert!(c.contains("<!-- s -->") && c.contains("content"));

        // Idempotent
        patch_markdown_section(&path, "<!-- s -->", "<!-- e -->", "content\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().matches("<!-- s -->").count(), 1);
    }

    #[test]
    fn test_write_to_claude_md() {
        let dir = env::temp_dir().join(format!("memo_hooks_write_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let block = InjectBlock {
            last_entries: vec![],
            open_todos: vec![],
            recent_tags: vec!["refactor".to_string()],
            entry_count: 1,
            last_recap: None,
            pinned_entries: vec![],
        };

        write_to_claude_md(&block, &dir).unwrap();
        let content = fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert!(content.contains("<!-- memo:start -->"));
        assert!(content.contains("recent tags: refactor"));

        write_to_claude_md(&block, &dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("CLAUDE.md")).unwrap().matches("<!-- memo:start -->").count(),
            1
        );
    }
}
