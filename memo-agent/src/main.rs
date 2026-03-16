mod store;
mod hooks;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use store::{Entry, Store, db_path_for, inject_marker_path};
use hooks::{inject_all, setup, write_to_claude_md, write_to_copilot_instructions, write_to_cursor_rules, write_to_vscode, write_to_windsurf_rules, InjectBlock};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "memo", version, about = "Persistent memory for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Override project directory (default: current directory)
    #[arg(long, value_name = "DIR", global = true)]
    project: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize project memory
    Init,

    /// Save a memory entry
    Log {
        /// Message to log. Use "-" to read from stdin.
        message: String,
        #[arg(long, action = clap::ArgAction::Append)]
        tag: Vec<String>,
    },

    /// Search memory entries
    Search {
        query: String,
        /// Limit to entries newer than this duration (e.g. 1d, 7d, 24h, 1w)
        #[arg(long, value_name = "DURATION")]
        since: Option<String>,
    },

    /// Print context block for injection at session start
    Inject {
        /// Write block into CLAUDE.md instead of stdout
        #[arg(long)]
        claude: bool,

        /// Write block into .cursor/rules/memo.mdc instead of stdout
        #[arg(long)]
        cursor: bool,

        /// Write block into .windsurfrules instead of stdout
        #[arg(long)]
        windsurf: bool,

        /// Write block into .github/copilot-instructions.md instead of stdout
        #[arg(long)]
        copilot: bool,

        /// Write block into .github/copilot-instructions.md (VS Code) instead of stdout
        #[arg(long)]
        vscode: bool,

        /// Inject into all configured agent files
        #[arg(long)]
        all: bool,

        /// Only inject if context has changed since last inject (for use in hooks)
        #[arg(long)]
        once: bool,

        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,

        /// Limit to entries newer than this duration (e.g. 1d, 7d, 24h, 1w)
        #[arg(long, value_name = "DURATION")]
        since: Option<String>,
    },

    /// List recent memory entries
    List {
        /// Show all entries (default: last 10)
        #[arg(long)]
        all: bool,

        /// Filter entries by tag
        #[arg(long)]
        tag: Option<String>,
    },

    /// Delete a memory entry by id
    Delete { id: i64 },

    /// List all tags with usage counts
    Tags,

    /// Clear all memory for current project
    Clear {
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Show memory statistics
    Stats,

    /// Set up memo for Claude Code (writes CLAUDE.md + installs Stop hook)
    Setup,

    /// Edit a memory entry in $EDITOR
    Edit { id: i64 },

    /// Check memo health for this project
    Doctor,

    /// Export memory entries to JSON
    Export {
        /// Output file (default: stdout)
        #[arg(long, short = 'o', value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Import memory entries from a JSON export file
    Import {
        /// JSON file to import (created by `memo export`)
        file: PathBuf,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Delete entries older than a duration
    Prune {
        /// Delete entries older than this duration (e.g. 30d, 12w)
        #[arg(long, value_name = "DURATION")]
        older_than: String,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Log a session summary shown prominently at next session start
    Recap {
        /// Summary of what was done this session and what comes next
        summary: String,
    },

    /// Manage todos
    Todo {
        #[command(subcommand)]
        action: TodoAction,
    },

    /// Bootstrap memory from recent git commit history
    Bootstrap {
        /// Number of commits to import (default: 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Sync memory with the team shared file (.memo/memory.json)
    Sync {
        /// Only export to .memo/memory.json (don't import)
        #[arg(long)]
        export_only: bool,
        /// Only import from .memo/memory.json (don't export)
        #[arg(long)]
        import_only: bool,
    },

    /// Auto-capture from PostToolUse hook (reads JSON from stdin, called by Claude Code hook)
    #[command(hide = true)]
    Capture,

    /// Pin an entry so it always appears in context
    Pin { id: i64 },

    /// Remove pin from an entry
    Unpin { id: i64 },

    /// Preview the context block that gets injected at session start
    Context,
}

#[derive(Subcommand)]
enum TodoAction {
    /// Mark a todo as done
    Done { id: i64 },
    /// List all open todos
    List,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

fn resolve_dir(cli_project: &Option<PathBuf>) -> Result<PathBuf> {
    match cli_project {
        Some(p) => Ok(p.clone()),
        None => std::env::current_dir().context("cannot determine current directory"),
    }
}

/// Parse a duration string like "7d" into ("7", "d") and return a chrono::Duration.
fn parse_duration(s: &str) -> Result<chrono::Duration> {
    // split "7d" into ("7", "d") — saturating_sub(1) handles empty string safely
    let (num_str, unit) = s.split_at(s.len().saturating_sub(1));
    let n: i64 = num_str
        .parse()
        .with_context(|| format!("invalid duration: {s}"))?;
    match unit {
        "d" => Ok(chrono::Duration::days(n)),
        "h" => Ok(chrono::Duration::hours(n)),
        "w" => Ok(chrono::Duration::weeks(n)),
        _ => anyhow::bail!("invalid duration format '{s}': use e.g. 1d, 7d, 24h, 1w"),
    }
}

/// Returns the formatted (id, timestamp, tags) parts of an entry for display.
fn format_entry_prefix(e: &Entry) -> (String, String, String) {
    let id = format!("#{}", e.id).cyan().bold().to_string();
    let ts = e.timestamp.format("%Y-%m-%d %H:%M").to_string().dimmed().to_string();
    let tags = if e.tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", e.tags.join(", ")).yellow().to_string()
    };
    (id, ts, tags)
}

fn print_entry(e: &Entry) {
    let (id, ts, tags) = format_entry_prefix(e);
    let pin = if e.pinned { " 📌".to_string() } else { String::new() };
    println!("{} {} — {}{}{}", id, ts, e.content, tags, pin);
}

fn print_entry_highlight(e: &Entry, query: &str) {
    let (id, ts, tags) = format_entry_prefix(e);
    let content = if let Some(pos) = e.content.to_lowercase().find(&query.to_lowercase()) {
        let before = &e.content[..pos];
        let matched = &e.content[pos..pos + query.len()];
        let after = &e.content[pos + query.len()..];
        format!("{}{}{}", before, matched.red().bold(), after)
    } else {
        e.content.clone()
    };
    println!("{id} {ts} — {content}{tags}");
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = resolve_dir(&cli.project)?;

    match cli.command {
        Command::Init => {
            let store = Store::open(&dir)?;
            println!("memo initialized for project {}", &store.project_id[..8]);
            println!("db: ~/.local/share/memo/{}.db", store.project_id);
            println!();
            println!("Add the following to your project's CLAUDE.md:");
            println!();
            println!("```");
            println!("<!-- memo:start -->");
            println!("<!-- memo:end -->");
            println!("```");
            println!();
            println!("Or run `memo inject --claude` to write it automatically.");
        }

        Command::Log { message, tag } => {
            let message = if message == "-" {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("failed to read from stdin")?;
                let trimmed = buf.trim().to_string();
                anyhow::ensure!(!trimmed.is_empty(), "stdin message was empty");
                trimmed
            } else {
                message
            };
            let store = Store::open(&dir)?;
            store.save(&message, &tag)?;
            println!("logged: {message}");
        }

        Command::Inject { claude, cursor, windsurf, copilot, vscode, all, once, format, since } => {
            // --once guard: skip if nothing changed since last inject
            if once {
                let marker = inject_marker_path(&dir)?;
                if marker.exists()
                    && let Ok(meta) = std::fs::metadata(&marker)
                    && let Ok(mtime) = meta.modified()
                {
                    let mtime_dt = chrono::DateTime::<chrono::Utc>::from(mtime);
                    let age = chrono::Utc::now() - mtime_dt;
                    if age < chrono::Duration::minutes(5) {
                        let store = Store::open(&dir)?;
                        if !store.has_entries_since(mtime_dt)? {
                            return Ok(());
                        }
                    }
                }
            }

            let store = Store::open(&dir)?;
            let block = match since {
                Some(s) => InjectBlock::build_since(&store, chrono::Utc::now() - parse_duration(&s)?)?,
                None => InjectBlock::build(&store)?,
            };

            if all {
                let updated = inject_all(&block, &dir)?;
                for file in &updated {
                    println!("memo context written to {file}");
                }
                if updated.is_empty() {
                    println!("no configured agent files found — run `memo setup` first");
                }
            } else if claude {
                write_to_claude_md(&block, &dir)?;
                println!("memo context written to CLAUDE.md");
            } else if cursor {
                write_to_cursor_rules(&block, &dir)?;
                println!("memo context written to .cursor/rules/memo.mdc");
            } else if windsurf {
                write_to_windsurf_rules(&block, &dir)?;
                println!("memo context written to .windsurfrules");
            } else if copilot {
                write_to_copilot_instructions(&block, &dir)?;
                println!("memo context written to .github/copilot-instructions.md");
            } else if vscode {
                write_to_vscode(&block, &dir)?;
                println!("memo context written to .github/copilot-instructions.md");
            } else {
                match format {
                    OutputFormat::Json => println!("{}", block.render_json()?),
                    OutputFormat::Text => print!("{}", block.render_text()),
                }
            }

            // Update the inject marker if --once was used
            if once {
                let marker = inject_marker_path(&dir)?;
                // hook must never fail — ignore write errors for the marker file
                let _ = std::fs::write(marker, "");
            }
        }

        Command::List { all, tag } => {
            let store = Store::open(&dir)?;
            let limit = if all { None } else { Some(10) };
            let entries = match tag {
                Some(t) => store.list_by_tag(&t, limit)?,
                None => store.list(limit)?,
            };

            if entries.is_empty() {
                println!("no entries yet.");
                println!("  run `memo bootstrap` to import git history, or");
                println!("  run `memo log \"<message>\"` to save your first entry.");
                return Ok(());
            }
            entries.iter().for_each(print_entry);
        }

        Command::Delete { id } => {
            let store = Store::open(&dir)?;
            if store.delete(id)? {
                println!("deleted entry #{id}");
            } else {
                println!("entry #{id} not found");
            }
        }

        Command::Tags => {
            let store = Store::open(&dir)?;
            let tags = store.all_tags()?;
            if tags.is_empty() {
                println!("no tags yet");
                return Ok(());
            }
            for (tag, count) in &tags {
                println!("{tag:<20} {count}");
            }
        }

        Command::Search { query, since } => {
            let store = Store::open(&dir)?;
            let entries = match since {
                Some(s) => store.search_since(&query, chrono::Utc::now() - parse_duration(&s)?)?,
                None => store.search(&query)?,
            };
            if entries.is_empty() {
                println!("no entries found for query: {query}");
                return Ok(());
            }
            entries.iter().for_each(|e| print_entry_highlight(e, &query));
        }

        Command::Clear { yes } => {
            if !yes {
                eprint!("clear all memory for this project? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("aborted");
                    return Ok(());
                }
            }
            let store = Store::open(&dir)?;
            println!("cleared {} entries", store.clear()?);
        }

        Command::Setup => {
            let result = setup(&dir)?;
            println!("✓ CLAUDE.md updated with memo instructions and context block");
            if result.claude_hook_installed {
                println!("✓ Stop hook installed in .claude/settings.json");
                println!("  → memo inject --claude will run automatically at end of each Claude Code session");
            } else {
                println!("  Claude Code Stop hook already present, skipped");
            }
            if result.start_hook_installed {
                println!("✓ Start hook installed (UserPromptSubmit) for fresh context at session start");
            } else {
                println!("  Start hook already present, skipped");
            }
            if result.post_tool_hook_installed {
                println!("✓ PostToolUse hook installed — file edits captured automatically in Claude Code");
            } else {
                println!("  PostToolUse hook already present, skipped");
            }
            if result.cursor_rules_written {
                println!("✓ Cursor rules written to .cursor/rules/memo.mdc");
                println!("  → memo inject --cursor will update context at session start");
            } else {
                println!("  Cursor rules already present, skipped");
            }
            if result.windsurf_rules_written {
                println!("✓ Windsurf rules written to .windsurfrules");
                println!("  → memo inject --windsurf will update context at session start");
            } else {
                println!("  Windsurf rules already present, skipped");
            }
            if result.copilot_instructions_written {
                println!("✓ Copilot instructions written to .github/copilot-instructions.md");
                println!("  → memo inject --copilot will update context at session start");
            } else {
                println!("  Copilot instructions already present, skipped");
            }
            println!();
            println!("Run `memo log \"<message>\"` to start logging.");
        }

        Command::Stats => {
            let store = Store::open(&dir)?;
            let block = InjectBlock::build(&store)?;
            println!("project:      {}", &store.project_id[..8]);
            println!("entries:      {}", block.entry_count);
            println!("tokens saved: ~{}", block.render_text().len() / 4);
            if !block.recent_tags.is_empty() {
                println!("top tags:     {}", block.recent_tags.join(", "));
            }
        }

        Command::Edit { id } => {
            let store = Store::open(&dir)?;
            let entry = store.get(id)?.ok_or_else(|| anyhow::anyhow!("entry #{id} not found"))?;

            let tmp_path = std::env::temp_dir().join(format!("memo_edit_{id}.txt"));
            std::fs::write(&tmp_path, &entry.content)?;

            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| {
                    if cfg!(windows) { "notepad".to_string() } else { "vi".to_string() }
                });

            let status = std::process::Command::new(&editor)
                .arg(&tmp_path)
                .status()
                .with_context(|| format!("failed to launch editor '{editor}'"))?;

            if !status.success() {
                anyhow::bail!("editor '{editor}' exited with error");
            }

            let new_content = std::fs::read_to_string(&tmp_path)?;
            let _ = std::fs::remove_file(&tmp_path);
            let new_content = new_content.trim().to_string();

            if new_content.is_empty() {
                anyhow::bail!("content is empty, edit aborted");
            }

            if new_content == entry.content {
                println!("no changes");
                return Ok(());
            }

            store.update(id, &new_content, &entry.tags)?;
            println!("updated entry #{id}");
        }

        Command::Doctor => {
            let ok   = "✓".green().bold().to_string();
            let fail = "✗".red().bold().to_string();
            let warn = "!".yellow().bold().to_string();

            let mut issues = 0usize;

            macro_rules! check {
                ($cond:expr, $ok_msg:expr, $fail_msg:expr) => {
                    if $cond {
                        println!("  {ok} {}", $ok_msg);
                    } else {
                        println!("  {fail} {}", $fail_msg);
                        issues += 1;
                    }
                };
            }
            macro_rules! notice {
                ($msg:expr) => { println!("  {warn} {}", $msg); };
            }

            // ── Core ──────────────────────────────────────────────────────────
            println!("{}", "Core".bold());

            // 1. binary
            let path_env = std::env::var("PATH").unwrap_or_default();
            let sep = if cfg!(windows) { ';' } else { ':' };
            let bin_name = if cfg!(windows) { "memo.exe" } else { "memo" };
            let found_memo = path_env.split(sep).find_map(|p| {
                let c = std::path::Path::new(p).join(bin_name);
                if c.exists() { Some(c) } else { None }
            });
            match &found_memo {
                Some(p) => println!("  {ok} binary: {}", p.display()),
                None    => { println!("  {warn} memo not found in PATH — add it to PATH"); issues += 1; }
            }

            // 2. database
            match Store::open(&dir) {
                Ok(store) => {
                    let db_path = db_path_for(&dir)?;
                    let count = store.count()?;
                    println!("  {ok} database: {} ({count} entries)", db_path.display());
                }
                Err(e) => { println!("  {fail} database error: {e}"); issues += 1; }
            }

            println!();

            // ── Claude Code ───────────────────────────────────────────────────
            println!("{}", "Claude Code".bold());

            // 3. CLAUDE.md
            let claude_md = dir.join("CLAUDE.md");
            if claude_md.exists() {
                let md = std::fs::read_to_string(&claude_md).unwrap_or_default();
                check!(
                    md.contains("<!-- memo:start -->"),
                    "CLAUDE.md: memo context block present",
                    "CLAUDE.md: no memo block — run `memo setup`"
                );
            } else {
                println!("  {fail} CLAUDE.md not found — run `memo setup`");
                issues += 1;
            }

            // 4. .claude/settings.json hooks
            let settings_path = dir.join(".claude").join("settings.json");
            if settings_path.exists() {
                let raw = std::fs::read_to_string(&settings_path).unwrap_or_default();
                let root: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();

                let hook_commands = |section: &str| -> Vec<String> {
                    root.get("hooks")
                        .and_then(|h| h.get(section))
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .flat_map(|entry| {
                                entry.get("hooks")
                                    .and_then(|hs| hs.as_array())
                                    .map(|hs| hs.iter()
                                        .filter_map(|h| h.get("command").and_then(|c| c.as_str()).map(str::to_owned))
                                        .collect::<Vec<_>>())
                                    .unwrap_or_default()
                            })
                            .collect())
                        .unwrap_or_default()
                };

                let hook_matchers = |section: &str| -> Vec<String> {
                    root.get("hooks")
                        .and_then(|h| h.get(section))
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|e| e.get("matcher").and_then(|m| m.as_str()).map(str::to_owned))
                            .collect())
                        .unwrap_or_default()
                };

                // Stop hook
                let stop_cmds = hook_commands("Stop");
                check!(
                    stop_cmds.iter().any(|c| c.contains("memo inject")),
                    "hook Stop: memo inject --claude",
                    "hook Stop: missing — run `memo setup`"
                );

                // UserPromptSubmit hook
                let start_cmds = hook_commands("UserPromptSubmit");
                check!(
                    start_cmds.iter().any(|c| c.contains("memo inject")),
                    "hook UserPromptSubmit: memo inject --claude --once",
                    "hook UserPromptSubmit: missing — run `memo setup`"
                );

                // PostToolUse hook
                let post_cmds = hook_commands("PostToolUse");
                let post_matchers = hook_matchers("PostToolUse");
                let has_capture = post_cmds.iter().any(|c| c.contains("memo capture"));
                let has_matcher = post_matchers.iter().any(|m| m.contains("Write"));
                check!(
                    has_capture && has_matcher,
                    "hook PostToolUse: memo capture (Write|Edit|MultiEdit)",
                    "hook PostToolUse: missing or incomplete — run `memo setup`"
                );
            } else {
                println!("  {fail} .claude/settings.json not found — run `memo setup`");
                issues += 1;
            }

            // ── Cursor ────────────────────────────────────────────────────────
            let cursor_rules = dir.join(".cursor").join("rules").join("memo.mdc");
            if cursor_rules.exists() {
                println!();
                println!("{}", "Cursor".bold());
                let contents = std::fs::read_to_string(&cursor_rules).unwrap_or_default();
                check!(
                    contents.contains("alwaysApply: true"),
                    ".cursor/rules/memo.mdc: alwaysApply: true",
                    ".cursor/rules/memo.mdc: missing alwaysApply: true — run `memo setup`"
                );
                check!(
                    contents.contains("<!-- memo:start -->"),
                    ".cursor/rules/memo.mdc: memo context block present",
                    ".cursor/rules/memo.mdc: no memo block — run `memo inject --cursor`"
                );
            } else if dir.join(".cursor").exists() {
                println!();
                println!("{}", "Cursor".bold());
                notice!(".cursor/ found but memo.mdc missing — run `memo setup` to add it");
            }

            // ── Windsurf ──────────────────────────────────────────────────────
            let windsurf_rules = dir.join(".windsurfrules");
            if windsurf_rules.exists() {
                println!();
                println!("{}", "Windsurf".bold());
                let contents = std::fs::read_to_string(&windsurf_rules).unwrap_or_default();
                check!(
                    contents.contains("memo inject"),
                    ".windsurfrules: memo instructions present",
                    ".windsurfrules: no memo instructions — run `memo setup`"
                );
                check!(
                    contents.contains("<!-- memo:start -->"),
                    ".windsurfrules: memo context block present",
                    ".windsurfrules: no memo block — run `memo inject --windsurf`"
                );
            }

            // ── Copilot / VS Code ─────────────────────────────────────────────
            let copilot = dir.join(".github").join("copilot-instructions.md");
            if copilot.exists() {
                println!();
                println!("{}", "Copilot / VS Code".bold());
                let contents = std::fs::read_to_string(&copilot).unwrap_or_default();
                check!(
                    contents.contains("memo inject"),
                    ".github/copilot-instructions.md: memo instructions present",
                    ".github/copilot-instructions.md: no memo instructions — run `memo setup`"
                );
                check!(
                    contents.contains("<!-- memo:start -->"),
                    ".github/copilot-instructions.md: memo context block present",
                    ".github/copilot-instructions.md: no memo block — run `memo inject --copilot`"
                );
            }

            println!();
            if issues == 0 {
                println!("{}", "All checks passed.".green().bold());
            } else {
                println!("{}", format!("{issues} issue(s) found — run `memo setup` to fix.").yellow());
            }
        }

        Command::Export { output } => {
            let store = Store::open(&dir)?;
            let entries = store.export_all()?;
            let data = serde_json::json!({
                "version": 1,
                "project_id": store.project_id,
                "entries": entries.iter().map(|e| serde_json::json!({
                    "timestamp": e.timestamp.to_rfc3339(),
                    "content": e.content,
                    "tags": e.tags,
                })).collect::<Vec<_>>(),
            });
            let json = serde_json::to_string_pretty(&data)?;
            match output {
                Some(path) => {
                    std::fs::write(&path, &json)?;
                    println!("exported {} entries to {}", entries.len(), path.display());
                }
                None => println!("{json}"),
            }
        }

        Command::Import { file, yes } => {
            let content = std::fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let data: serde_json::Value = serde_json::from_str(&content)
                .context("invalid JSON — expected output from `memo export`")?;
            let entries = data["entries"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("missing 'entries' array in JSON"))?;

            if !yes {
                eprint!("import {} entries into this project? [y/N] ", entries.len());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("aborted");
                    return Ok(());
                }
            }

            let store = Store::open(&dir)?;
            let mut imported = 0usize;
            for entry in entries {
                let content_str = entry["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("entry missing 'content'"))?;
                let tags: Vec<String> = entry["tags"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string()))
                    .collect();
                let timestamp = entry["timestamp"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);
                store.save_at(content_str, &tags, timestamp)?;
                imported += 1;
            }
            println!("imported {imported} entries");
        }

        Command::Prune { older_than, yes } => {
            let cutoff = chrono::Utc::now() - parse_duration(&older_than)?;
            if !yes {
                eprint!("delete all entries older than {older_than}? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("aborted");
                    return Ok(());
                }
            }
            let store = Store::open(&dir)?;
            let deleted = store.prune(cutoff)?;
            println!("pruned {deleted} entries");
        }

        Command::Recap { summary } => {
            let store = Store::open(&dir)?;
            store.save(&format!("recap: {summary}"), &["recap".to_string()])?;
            println!("recap logged");
        }

        Command::Todo { action } => {
            let store = Store::open(&dir)?;
            match action {
                TodoAction::Done { id } => {
                    if store.complete_todo(id)? {
                        println!("todo #{id} marked as done");
                    } else {
                        println!("entry #{id} not found");
                    }
                }
                TodoAction::List => {
                    let todos = store.list_open_todos()?;
                    if todos.is_empty() {
                        println!("no open todos");
                        return Ok(());
                    }
                    todos.iter().for_each(print_entry);
                }
            }
        }

        Command::Bootstrap { limit, yes } => {
            let commits = store::git_log(&dir, limit);
            if commits.is_empty() {
                println!("no git history found in this directory");
                return Ok(());
            }

            if !yes {
                eprintln!("import {} commits from git history? [y/N] ", commits.len());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("aborted");
                    return Ok(());
                }
            }

            let store = Store::open(&dir)?;
            let mut imported = 0usize;
            for (timestamp, message) in &commits {
                let content = format!("git: {message}");
                if store.has_entry_by_signature(&content, *timestamp)? {
                    continue;
                }
                store.save_at(&content, &["git".to_string()], *timestamp)?;
                imported += 1;
            }
            println!("bootstrapped {imported} commits from git history");
            if imported < commits.len() {
                println!("({} already present, skipped)", commits.len() - imported);
            }
        }

        Command::Sync { export_only, import_only } => {
            let store = Store::open(&dir)?;
            let sync_path = dir.join(".memo").join("memory.json");

            let mut imported = 0usize;

            // Import from shared file
            if !export_only && sync_path.exists() {
                let content = std::fs::read_to_string(&sync_path)?;
                let data: serde_json::Value = serde_json::from_str(&content)
                    .context("invalid .memo/memory.json — delete it and re-run memo sync")?;
                if let Some(entries) = data["entries"].as_array() {
                    for entry in entries {
                        let content_str = entry["content"].as_str().unwrap_or("");
                        let tags: Vec<String> = entry["tags"]
                            .as_array()
                            .unwrap_or(&vec![])
                            .iter()
                            .filter_map(|t| t.as_str().map(|s| s.to_string()))
                            .collect();
                        let timestamp = entry["timestamp"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .unwrap_or_else(chrono::Utc::now);
                        if !store.has_entry_by_signature(content_str, timestamp)? {
                            store.save_at(content_str, &tags, timestamp)?;
                            imported += 1;
                        }
                    }
                }
            }

            // Export to shared file
            if !import_only {
                let entries = store.export_all()?;
                let data = serde_json::json!({
                    "version": 1,
                    "entries": entries.iter().map(|e| serde_json::json!({
                        "timestamp": e.timestamp.to_rfc3339(),
                        "content": e.content,
                        "tags": e.tags,
                    })).collect::<Vec<_>>(),
                });
                let memo_dir = dir.join(".memo");
                std::fs::create_dir_all(&memo_dir)?;
                std::fs::write(&sync_path, serde_json::to_string_pretty(&data)?)?;
                let exported = entries.len();
                if import_only {
                    println!("imported {imported} new entries");
                } else {
                    println!("synced: imported {imported} new entries, exported {exported} entries to .memo/memory.json");
                    println!("commit .memo/memory.json to share with your team");
                }
            } else {
                println!("imported {imported} new entries");
            }
        }

        Command::Capture => {
            // Called by Claude Code PostToolUse hook — hook must never fail
            let _ = run_capture(&dir);
        }

        Command::Pin { id } => {
            let store = Store::open(&dir)?;
            if store.pin(id)? {
                println!("pinned entry #{id} — it will always appear in context");
            } else {
                println!("entry #{id} not found");
            }
        }

        Command::Unpin { id } => {
            let store = Store::open(&dir)?;
            if store.unpin(id)? {
                println!("unpinned entry #{id}");
            } else {
                println!("entry #{id} not found");
            }
        }

        Command::Context => {
            let store = Store::open(&dir)?;
            let block = InjectBlock::build(&store)?;
            let project_short = &store.project_id[..8];

            println!("{}", format!("memo context — project {project_short} ({} entries)", block.entry_count).dimmed());
            println!();

            if !block.pinned_entries.is_empty() {
                println!("{}", "📌 pinned".cyan().bold());
                for e in &block.pinned_entries {
                    let ts = e.timestamp.format("%Y-%m-%d").to_string().dimmed();
                    println!("  {} {} — {}", format!("#{}", e.id).cyan(), ts, e.content);
                }
                println!();
            }

            if let Some(recap) = &block.last_recap {
                println!("{}", "📋 recap".yellow().bold());
                let text = recap.content
                    .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ':')
                    .trim();
                let ts = recap.timestamp.format("%Y-%m-%d").to_string().dimmed();
                println!("  {} — {}", ts, text);
                println!();
            }

            if !block.last_entries.is_empty() {
                println!("{}", "🕐 recent".bold());
                for e in &block.last_entries {
                    let ts = e.timestamp.format("%Y-%m-%d").to_string().dimmed();
                    let tags = if e.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", e.tags.join(", ")).yellow().to_string()
                    };
                    println!("  {} {} — {}{}", format!("#{}", e.id).cyan(), ts, e.content, tags);
                }
                println!();
            }

            if !block.open_todos.is_empty() {
                println!("{}", "✅ open todos".bold());
                for todo in &block.open_todos {
                    let text = todo.content
                        .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ':')
                        .trim();
                    println!("  {} — {}", format!("#{}", todo.id).cyan(), text);
                }
                println!();
            }

            if !block.recent_tags.is_empty() {
                println!("{}", format!("🏷  tags: {}", block.recent_tags.join(" · ")).dimmed());
                println!();
            }

            if block.pinned_entries.is_empty()
                && block.last_recap.is_none()
                && block.last_entries.is_empty()
                && block.open_todos.is_empty()
            {
                println!("{}", "no context yet.".dimmed());
                println!("{}", "  run `memo bootstrap` to import git history, or".dimmed());
                println!("{}", "  run `memo log \"<message>\"` to save your first entry.".dimmed());
                println!();
            }

            let raw = block.render_text();
            let token_estimate = raw.len() / 4;
            println!("{}", format!("~{token_estimate} tokens").dimmed());
        }
    }

    Ok(())
}

/// Read a PostToolUse JSON payload from stdin and auto-log modified files.
/// Called silently by the Claude Code PostToolUse hook — errors are intentionally swallowed.
fn run_capture(dir: &Path) -> anyhow::Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("failed to read stdin")?;

    let data: serde_json::Value = serde_json::from_str(&input)?;
    let tool_name = data["tool_name"].as_str().unwrap_or("");

    let file_path = data["tool_input"]["file_path"].as_str().unwrap_or("");
    if file_path.is_empty() {
        return Ok(());
    }
    let path = std::path::Path::new(file_path);
    // Canonicalize both paths to resolve symlinks (e.g. /var → /private/var on macOS)
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let rel = canonical_path.strip_prefix(&canonical_dir).unwrap_or(&canonical_path);
    let file = rel.display().to_string().replace('\\', "/");

    let log_msg = match tool_name {
        "Write" => {
            let content = data["tool_input"]["content"].as_str().unwrap_or("");
            match describe_content(content) {
                Some(desc) => format!("wrote {file}: {desc}"),
                None => format!("wrote {file}"),
            }
        }
        "Edit" => {
            let old = data["tool_input"]["old_string"].as_str().unwrap_or("");
            let new = data["tool_input"]["new_string"].as_str().unwrap_or("");
            match describe_diff(old, new) {
                Some(desc) => format!("edited {file}: {desc}"),
                None => format!("edited {file}"),
            }
        }
        "MultiEdit" => {
            let edits = data["tool_input"]["edits"].as_array();
            let n = edits.map(|a| a.len()).unwrap_or(1);
            let old: String = edits
                .map(|arr| arr.iter().filter_map(|e| e["old_string"].as_str()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            let new: String = data["tool_input"]["edits"].as_array()
                .map(|arr| arr.iter().filter_map(|e| e["new_string"].as_str()).collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            match describe_diff(&old, &new) {
                Some(desc) => format!("edited {file}: {desc}"),
                None => format!("edited {file} ({n} changes)"),
            }
        }
        _ => return Ok(()),
    };

    let store = Store::open(dir)?;

    // Deduplicate: skip if same entry was logged in the last 60 seconds
    if store.has_recent_entry(&log_msg, 60)? {
        return Ok(());
    }

    store.save(&log_msg, &["auto".to_string()])?;
    Ok(())
}

/// Describe the primary construct in a freshly-written file.
fn describe_content(text: &str) -> Option<String> {
    let mut prev = "";
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            prev = line;
            continue;
        }
        if let Some(desc) = classify_line(line, prev) {
            return Some(desc);
        }
        prev = line;
    }
    None
}

/// Describe what was changed between `old` and `new` text (e.g. Edit payload).
/// Scans lines that appear in `new` but not `old` for meaningful constructs.
fn describe_diff(old: &str, new: &str) -> Option<String> {
    let old_lines: std::collections::HashSet<&str> =
        old.lines().map(str::trim).filter(|l| !l.is_empty()).collect();

    let mut prev = "";
    for raw in new.lines() {
        let line = raw.trim();
        if line.is_empty() {
            prev = line;
            continue;
        }
        if !old_lines.contains(line) && let Some(desc) = classify_line(line, prev) {
            return Some(desc);
        }
        prev = line;
    }
    None
}

/// Map a single source line to a short human description, considering the previous line
/// (for e.g. test attribute detection). Returns None if no meaningful pattern matches.
fn classify_line(line: &str, prev: &str) -> Option<String> {
    // Skip pure comments (unless they contain TODO/FIXME)
    let is_comment = line.starts_with("//") || line.starts_with("/*")
        || (line.starts_with('#') && !line.starts_with("#[") && !line.starts_with("#!"));
    if is_comment {
        if line.contains("TODO") { return Some("added TODO".to_string()); }
        if line.contains("FIXME") { return Some("added FIXME".to_string()); }
        return None;
    }

    // Test detection — Rust: #[test] / #[tokio::test] on previous line
    let is_test_attr = prev == "#[test]"
        || prev.starts_with("#[tokio::test")
        || prev.starts_with("#[async_std::test");
    if is_test_attr {
        let fn_line = line.trim_start_matches("pub ").trim_start_matches("async ");
        if fn_line.starts_with("fn ") && let Some(name) = word_after(fn_line, "fn ") {
            return Some(format!("added test {name}"));
        }
    }

    // Rust / Go / Swift function definitions
    for prefix in &["pub async fn ", "pub fn ", "async fn ", "fn "] {
        if line.starts_with(prefix) {
            let rest = line.trim_start_matches("pub ").trim_start_matches("async ").trim_start_matches("fn ");
            if let Some(name) = word_before_paren(rest) {
                return Some(format!("added fn {name}"));
            }
        }
    }

    // Python / Lua
    for prefix in &["async def ", "def "] {
        if line.starts_with(prefix) && let Some(name) = word_after(line, prefix) {
            return Some(format!("added fn {name}"));
        }
    }

    // TypeScript / JavaScript named functions
    for prefix in &["export async function ", "export function ", "async function ", "function "] {
        if line.starts_with(prefix) {
            let rest = line
                .trim_start_matches("export ")
                .trim_start_matches("async ")
                .trim_start_matches("function ");
            if let Some(name) = word_before_paren(rest) {
                return Some(format!("added fn {name}"));
            }
        }
    }

    // Arrow functions: (export) const name = (...) => or = async (...)
    for prefix in &["export const ", "const "] {
        if line.starts_with(prefix) {
            let rest = line.trim_start_matches("export ").trim_start_matches("const ");
            if (rest.contains("= (") || rest.contains("= async (") || rest.contains("=>"))
                && let Some(name) = word_before_assign(rest)
            {
                return Some(format!("added fn {name}"));
            }
        }
    }

    // Struct / enum / trait / class / interface / type
    for (prefix, kind) in &[
        ("pub struct ", "struct"),
        ("struct ", "struct"),
        ("pub enum ", "enum"),
        ("enum ", "enum"),
        ("pub trait ", "trait"),
        ("trait ", "trait"),
        ("interface ", "interface"),
        ("export interface ", "interface"),
        ("export type ", "type"),
        ("class ", "class"),
        ("abstract class ", "class"),
        ("export class ", "class"),
        ("export abstract class ", "class"),
    ] {
        if line.starts_with(prefix) && let Some(name) = word_after(line, prefix) {
            return Some(format!("added {kind} {name}"));
        }
    }

    // Rust impl blocks: "impl Trait for Type" or "impl Type"
    if line.starts_with("impl ") && !line.starts_with("impl<") {
        let rest = &line[5..];
        let name = if let Some(idx) = rest.find(" for ") {
            rest[idx + 5..].split(|c: char| !c.is_alphanumeric() && c != '_').next()
        } else {
            rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '<').next()
        };
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            return Some(format!("added impl {n}"));
        }
    }

    // HTTP route handlers (Express / Axum / Hono / etc.)
    for method in &["get", "post", "put", "patch", "delete", "head", "options"] {
        for pattern in &[
            format!("app.{method}("),
            format!("router.{method}("),
            format!("route.{method}("),
        ] {
            if line.starts_with(pattern.as_str()) {
                let path = extract_route_path(line).unwrap_or_else(|| "/".to_string());
                return Some(format!("added {method} {path}"));
            }
        }
    }

    None
}

/// Extract name of function from "fn name(..." → "name"
fn word_before_paren(s: &str) -> Option<&str> {
    let name = s.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Extract first identifier after `prefix` in `line`.
fn word_after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?;
    let name = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Extract identifier before " =" in a const/let assignment.
fn word_before_assign(s: &str) -> Option<&str> {
    let name = s.split(|c: char| !c.is_alphanumeric() && c != '_').next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Extract the route path string from a line like `app.get("/foo", ...)`.
fn extract_route_path(line: &str) -> Option<String> {
    let after_paren = line.split_once('(')?.1.trim_start();
    let q = after_paren.chars().next()?;
    if q == '"' || q == '\'' || q == '`' {
        let rest = &after_paren[1..];
        let end = rest.find(q)?;
        Some(rest[..end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    // ── classify_line ─────────────────────────────────────────────────────────

    #[test]
    fn rust_pub_fn() {
        assert_eq!(classify_line("pub fn handle_login(req: Request) -> Response {", ""), Some("added fn handle_login".into()));
    }

    #[test]
    fn rust_pub_async_fn() {
        assert_eq!(classify_line("pub async fn fetch_user(id: Uuid) -> Result<User> {", ""), Some("added fn fetch_user".into()));
    }

    #[test]
    fn rust_private_fn() {
        assert_eq!(classify_line("fn validate_token(token: &str) -> bool {", ""), Some("added fn validate_token".into()));
    }

    #[test]
    fn rust_struct() {
        assert_eq!(classify_line("pub struct AuthToken {", ""), Some("added struct AuthToken".into()));
    }

    #[test]
    fn rust_enum() {
        assert_eq!(classify_line("pub enum TokenKind {", ""), Some("added enum TokenKind".into()));
    }

    #[test]
    fn rust_trait() {
        assert_eq!(classify_line("pub trait Authenticator {", ""), Some("added trait Authenticator".into()));
    }

    #[test]
    fn rust_impl() {
        assert_eq!(classify_line("impl AuthToken {", ""), Some("added impl AuthToken".into()));
    }

    #[test]
    fn rust_impl_for() {
        assert_eq!(classify_line("impl Authenticator for JwtAuthenticator {", ""), Some("added impl JwtAuthenticator".into()));
    }

    #[test]
    fn rust_test_with_attr() {
        assert_eq!(classify_line("fn test_login_success() {", "#[test]"), Some("added test test_login_success".into()));
    }

    #[test]
    fn rust_tokio_test() {
        assert_eq!(classify_line("async fn test_fetch_user() {", "#[tokio::test]"), Some("added test test_fetch_user".into()));
    }

    #[test]
    fn python_def() {
        assert_eq!(classify_line("def process_payment(amount, currency):", ""), Some("added fn process_payment".into()));
    }

    #[test]
    fn python_async_def() {
        assert_eq!(classify_line("async def handle_webhook(request):", ""), Some("added fn handle_webhook".into()));
    }

    #[test]
    fn typescript_export_function() {
        assert_eq!(classify_line("export function createUser(data: UserData): Promise<User> {", ""), Some("added fn createUser".into()));
    }

    #[test]
    fn typescript_arrow_fn() {
        assert_eq!(classify_line("export const deleteAccount = async (id: string) => {", ""), Some("added fn deleteAccount".into()));
    }

    #[test]
    fn typescript_interface() {
        assert_eq!(classify_line("interface UserRepository {", ""), Some("added interface UserRepository".into()));
    }

    #[test]
    fn express_get_route() {
        assert_eq!(classify_line("app.get('/api/users', authenticate, listUsers)", ""), Some("added get /api/users".into()));
    }

    #[test]
    fn express_post_route() {
        assert_eq!(classify_line("router.post(\"/auth/login\", loginHandler)", ""), Some("added post /auth/login".into()));
    }

    #[test]
    fn comment_skipped() {
        assert_eq!(classify_line("// fn not_a_real_function() {", ""), None);
    }

    #[test]
    fn todo_comment() {
        assert_eq!(classify_line("// TODO: handle edge case", ""), Some("added TODO".into()));
    }

    #[test]
    fn plain_assignment_skipped() {
        assert_eq!(classify_line("let x = 42;", ""), None);
    }

    // ── describe_diff ─────────────────────────────────────────────────────────

    #[test]
    fn diff_detects_new_fn() {
        let old = "fn foo() {}\n";
        let new = "fn foo() {}\nfn bar() {}\n";
        assert_eq!(describe_diff(old, new), Some("added fn bar".into()));
    }

    #[test]
    fn diff_ignores_unchanged_lines() {
        let old = "pub fn existing() {}\n";
        let new = "pub fn existing() {}\n";
        assert_eq!(describe_diff(old, new), None);
    }

    #[test]
    fn diff_detects_struct_in_edit() {
        let old = "use std::collections::HashMap;\n";
        let new = "use std::collections::HashMap;\npub struct Config {\n    pub port: u16,\n}\n";
        assert_eq!(describe_diff(old, new), Some("added struct Config".into()));
    }

    // ── describe_content ──────────────────────────────────────────────────────

    #[test]
    fn content_finds_first_fn() {
        let src = "use anyhow::Result;\n\npub async fn run(config: Config) -> Result<()> {\n    Ok(())\n}\n";
        assert_eq!(describe_content(src), Some("added fn run".into()));
    }

    #[test]
    fn content_empty_returns_none() {
        assert_eq!(describe_content(""), None);
    }

    #[test]
    fn content_only_comments() {
        let src = "// This file is intentionally empty\n";
        assert_eq!(describe_content(src), None);
    }
}
