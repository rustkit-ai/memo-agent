use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use memo_core::Store;
use memo_hooks::{write_to_claude_md, InjectBlock};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "memo", version, about = "Persistent memory for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize project memory
    Init,

    /// Save a memory entry
    Log {
        message: String,
        #[arg(long, action = clap::ArgAction::Append)]
        tag: Vec<String>,
    },

    /// Search memory entries
    Search {
        /// Query string to search for in entry content
        query: String,
    },

    /// Print context block for injection at session start
    Inject {
        /// Write block into CLAUDE.md instead of stdout
        #[arg(long)]
        claude: bool,

        /// Output format
        #[arg(long, value_name = "FORMAT", default_value = "text")]
        format: String,
    },

    /// List recent memory entries
    List {
        /// Show all entries (default: last 10)
        #[arg(long)]
        all: bool,
    },

    /// Clear all memory for current project
    Clear {
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Show memory statistics
    Stats,
}

fn project_dir() -> Result<PathBuf> {
    std::env::current_dir().context("cannot determine current directory")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = project_dir()?;

    match cli.command {
        Command::Init => {
            let store = Store::open(&dir)?;
            println!("memo initialized for project {}", &store.project_id[..8]);
            println!("db: ~/.local/share/memo/{}.db", store.project_id);
            println!();
            println!("Add the following to your project's CLAUDE.md to auto-inject context:");
            println!();
            println!("```");
            println!("<!-- memo:start -->");
            println!("<!-- memo:end -->");
            println!("```");
            println!();
            println!("Or run `memo inject --claude` to write it automatically.");
        }

        Command::Log { message, tag } => {
            let store = Store::open(&dir)?;
            store.save(&message, &tag)?;
            println!("logged: {}", message);
        }

        Command::Inject { claude, format } => {
            let store = Store::open(&dir)?;
            let block = InjectBlock::build(&store)?;

            if claude {
                write_to_claude_md(&block, &dir)?;
                println!("memo context written to CLAUDE.md");
            } else {
                match format.as_str() {
                    "json" => println!("{}", block.render_json()?),
                    _ => print!("{}", block.render_text()),
                }
            }
        }

        Command::List { all } => {
            let store = Store::open(&dir)?;
            let limit = if all { None } else { Some(10) };
            let entries = store.list(limit)?;

            if entries.is_empty() {
                println!("no entries yet. run `memo log \"<message>\"` to save one.");
                return Ok(());
            }

            for entry in &entries {
                let date = entry.timestamp.format("%Y-%m-%d %H:%M");
                let tags = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", entry.tags.join(", "))
                };
                println!("{} — {}{}", date, entry.content, tags);
            }
        }

        Command::Search { query } => {
            let store = Store::open(&dir)?;
            let entries = store.search(&query)?;

            if entries.is_empty() {
                println!("no entries found for query: {}", query);
                return Ok(());
            }

            for entry in &entries {
                let date = entry.timestamp.format("%Y-%m-%d %H:%M");
                let tags = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", entry.tags.join(", "))
                };
                println!("{} — {}{}", date, entry.content, tags);
            }
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
            let n = store.clear()?;
            println!("cleared {} entries", n);
        }

        Command::Stats => {
            let store = Store::open(&dir)?;
            let count = store.count()?;
            let tags = store.recent_tags(20)?;
            let block = InjectBlock::build(&store)?;
            // Rough token estimate: chars in inject block / 4
            let tokens_saved = block.render_text().len() / 4;
            println!("project:      {}", &store.project_id[..8]);
            println!("entries:      {}", count);
            println!("tokens saved: ~{}", tokens_saved);
            if !tags.is_empty() {
                println!("top tags:     {}", tags.join(", "));
            }
        }
    }

    Ok(())
}
