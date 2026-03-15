use anyhow::Result;
use memo_core::{Entry, Store};
use std::path::Path;

pub struct InjectBlock {
    pub last_entry: Option<Entry>,
    pub todos: Vec<Entry>,
    pub recent_tags: Vec<String>,
    pub entry_count: usize,
}

impl InjectBlock {
    pub fn build(store: &Store) -> Result<Self> {
        let entries = store.list(Some(20))?;
        let last_entry = entries.first().cloned();
        let todos = entries
            .iter()
            .filter(|e| e.content.starts_with("todo:") || e.content.starts_with("TODO:"))
            .cloned()
            .collect();
        let recent_tags = store.recent_tags(10)?;
        let entry_count = store.count()?;
        Ok(Self {
            last_entry,
            todos,
            recent_tags,
            entry_count,
        })
    }

    pub fn render_text(&self) -> String {
        let mut out = String::from("## memo context\n");

        if let Some(e) = &self.last_entry {
            let date = e.timestamp.format("%Y-%m-%d");
            out.push_str(&format!("last: {} — \"{}\"\n", date, e.content));
        } else {
            out.push_str("last: (no entries yet)\n");
        }

        for todo in &self.todos {
            out.push_str(&format!("todo: {}\n", todo.content.trim_start_matches("todo:").trim_start_matches("TODO:").trim()));
        }

        if !self.recent_tags.is_empty() {
            out.push_str(&format!("recent tags: {}\n", self.recent_tags.join(" · ")));
        }

        out
    }

    pub fn render_json(&self) -> Result<String> {
        let value = serde_json::json!({
            "last_entry": self.last_entry.as_ref().map(|e| serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
                "tags": e.tags,
            })),
            "todos": self.todos.iter().map(|e| serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "content": e.content,
            })).collect::<Vec<_>>(),
            "recent_tags": self.recent_tags,
            "entry_count": self.entry_count,
        });
        Ok(serde_json::to_string_pretty(&value)?)
    }
}

pub fn write_to_claude_md(block: &InjectBlock, project_dir: &Path) -> Result<()> {
    let claude_md = project_dir.join("CLAUDE.md");
    let section_start = "<!-- memo:start -->";
    let section_end = "<!-- memo:end -->";

    let memo_section = format!(
        "{}\n{}{}\n",
        section_start,
        block.render_text(),
        section_end
    );

    let existing = if claude_md.exists() {
        std::fs::read_to_string(&claude_md)?
    } else {
        String::new()
    };

    let new_content = if existing.contains(section_start) {
        // Replace existing section
        let start = existing.find(section_start).unwrap();
        let end = existing
            .find(section_end)
            .map(|i| i + section_end.len())
            .unwrap_or(start);
        format!("{}{}{}", &existing[..start], memo_section, &existing[end..])
    } else {
        // Prepend
        format!("{}\n{}", memo_section, existing)
    };

    std::fs::write(&claude_md, new_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_render_text_empty() {
        let block = InjectBlock {
            last_entry: None,
            todos: vec![],
            recent_tags: vec![],
            entry_count: 0,
        };
        let text = block.render_text();
        assert!(text.contains("## memo context"));
        assert!(text.contains("no entries yet"));
    }

    #[test]
    fn test_render_json() {
        let block = InjectBlock {
            last_entry: None,
            todos: vec![],
            recent_tags: vec!["bug".to_string()],
            entry_count: 5,
        };
        let json = block.render_json().unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["entry_count"], 5);
        assert_eq!(val["recent_tags"][0], "bug");
    }

    #[test]
    fn test_write_to_claude_md() {
        let dir = env::temp_dir().join(format!("memo_hooks_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let block = InjectBlock {
            last_entry: None,
            todos: vec![],
            recent_tags: vec!["refactor".to_string()],
            entry_count: 1,
        };

        write_to_claude_md(&block, &dir).unwrap();

        let content = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert!(content.contains("<!-- memo:start -->"));
        assert!(content.contains("recent tags: refactor"));

        // Idempotent: write again, should replace not append
        write_to_claude_md(&block, &dir).unwrap();
        let content2 = std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert_eq!(content2.matches("<!-- memo:start -->").count(), 1);
    }
}
