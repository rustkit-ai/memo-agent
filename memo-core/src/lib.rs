use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub content: String,
    pub tags: Vec<String>,
    pub project_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub project_id: String,
}

pub struct Store {
    conn: Connection,
    pub project_id: String,
    pub session_id: String,
}

impl Store {
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let project_id = "test_project".to_string();
        let session_id = new_session_id();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, started_at, project_id) VALUES (?1, ?2, ?3)",
            params![session_id, Utc::now().to_rfc3339(), project_id],
        )?;
        Ok(Self { conn, project_id, session_id })
    }

    pub fn open(project_dir: &Path) -> Result<Self> {
        let project_id = detect_project_id(project_dir)?;
        let db_path = db_path(&project_id)?;

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create db dir {}", parent.display()))?;
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("open db {}", db_path.display()))?;

        conn.execute_batch(SCHEMA)?;

        let session_id = new_session_id();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, started_at, project_id) VALUES (?1, ?2, ?3)",
            params![session_id, Utc::now().to_rfc3339(), project_id],
        )?;

        Ok(Self {
            conn,
            project_id,
            session_id,
        })
    }

    pub fn save(&self, content: &str, tags: &[String]) -> Result<i64> {
        let tags_json = serde_json::to_string(tags)?;
        let id = self.conn.execute(
            "INSERT INTO entries (timestamp, content, tags, project_id, session_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Utc::now().to_rfc3339(),
                content,
                tags_json,
                self.project_id,
                self.session_id,
            ],
        )?;
        Ok(id as i64)
    }

    pub fn list(&self, limit: Option<usize>) -> Result<Vec<Entry>> {
        let sql = match limit {
            Some(n) => format!(
                "SELECT id, timestamp, content, tags, project_id, session_id \
                 FROM entries WHERE project_id = '{}' ORDER BY timestamp DESC LIMIT {}",
                self.project_id, n
            ),
            None => format!(
                "SELECT id, timestamp, content, tags, project_id, session_id \
                 FROM entries WHERE project_id = '{}' ORDER BY timestamp DESC",
                self.project_id
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let entries = stmt
            .query_map([], |row| {
                let tags_json: String = row.get(3)?;
                let ts_str: String = row.get(1)?;
                Ok((row.get(0)?, ts_str, row.get(2)?, tags_json, row.get(4)?, row.get(5)?))
            })?
            .map(|r| {
                let (id, ts_str, content, tags_json, project_id, session_id): (
                    i64, String, String, String, String, String,
                ) = r?;
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Ok(Entry {
                    id,
                    timestamp,
                    content,
                    tags,
                    project_id,
                    session_id,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(entries)
    }

    pub fn clear(&self) -> Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM entries WHERE project_id = ?1",
            params![self.project_id],
        )?;
        Ok(n)
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE project_id = ?1",
            params![self.project_id],
            |row| row.get(0),
        )?;
        Ok(n as usize)
    }

    pub fn recent_tags(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT tags FROM entries WHERE project_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![self.project_id, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;

        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        for row in rows {
            let json = row?;
            let ts: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            for t in ts {
                if seen.insert(t.clone()) {
                    tags.push(t);
                }
            }
        }
        Ok(tags)
    }
}

fn detect_project_id(dir: &Path) -> Result<String> {
    // Try to find git remote URL
    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "remote", "get-url", "origin"])
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !url.is_empty() {
                return Ok(hash_str(&url));
            }
        }
    }

    // Fallback: hash the absolute path
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    Ok(hash_str(&canonical.to_string_lossy()))
}

fn hash_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)[..16].to_string()
}

fn db_path(project_id: &str) -> Result<PathBuf> {
    let base = dirs_base()?;
    Ok(base.join(format!("{}.db", project_id)))
}

fn dirs_base() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".local").join("share").join("memo"))
}

fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", ts)
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    project_id  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,
    content     TEXT NOT NULL,
    tags        TEXT NOT NULL DEFAULT '[]',
    project_id  TEXT NOT NULL,
    session_id  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entries_project ON entries(project_id, timestamp);
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_list() {
        let store = Store::open_in_memory().unwrap();
        store.save("hello world", &[]).unwrap();
        store.save("second entry", &["bug".to_string()]).unwrap();
        let entries = store.list(None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].content, "second entry");
    }

    #[test]
    fn test_count() {
        let store = Store::open_in_memory().unwrap();
        store.save("a", &[]).unwrap();
        store.save("b", &[]).unwrap();
        assert_eq!(store.count().unwrap(), 2);
    }

    #[test]
    fn test_clear() {
        let store = Store::open_in_memory().unwrap();
        store.save("a", &[]).unwrap();
        let n = store.clear().unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_recent_tags() {
        let store = Store::open_in_memory().unwrap();
        store.save("fix", &["bug".to_string(), "auth".to_string()]).unwrap();
        store.save("refactor", &["refactor".to_string()]).unwrap();
        let tags = store.recent_tags(10).unwrap();
        assert!(tags.contains(&"refactor".to_string()));
        assert!(tags.contains(&"bug".to_string()));
    }
}
