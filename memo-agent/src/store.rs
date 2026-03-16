use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
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
    pub status: String,  // "active" or "done"
    pub pinned: bool,
}

pub struct Store {
    conn: Connection,
    pub project_id: String,
    pub(crate) session_id: String,
}

/// Returns the database path for the given project directory.
pub fn db_path_for(project_dir: &Path) -> Result<PathBuf> {
    let project_id = detect_project_id(project_dir)?;
    db_path(&project_id)
}

/// Returns the path to the last-inject marker file for this project.
pub fn inject_marker_path(project_dir: &Path) -> Result<PathBuf> {
    let project_id = detect_project_id(project_dir)?;
    Ok(db_path(&project_id)?.with_extension("last_inject"))
}

/// Shared SELECT column list used in all entry queries.
const SELECT_COLS: &str =
    "id, timestamp, content, tags, project_id, session_id, status, pinned";

impl Store {
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
        run_migrations(&conn)?;

        let session_id = new_session_id();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, started_at, project_id) VALUES (?1, ?2, ?3)",
            params![session_id, Utc::now().to_rfc3339(), project_id],
        )?;

        Ok(Self { conn, project_id, session_id })
    }

    /// Save an entry and return its row ID.
    pub fn save(&self, content: &str, tags: &[String]) -> Result<i64> {
        let tags_json = serde_json::to_string(tags)?;
        self.conn.execute(
            "INSERT INTO entries (timestamp, content, tags, project_id, session_id) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Utc::now().to_rfc3339(),
                content,
                tags_json,
                self.project_id,
                self.session_id,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list(&self, limit: Option<usize>) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 ORDER BY timestamp DESC LIMIT ?2"),
            params![self.project_id, limit_val(limit)],
        )
    }

    pub fn list_by_tag(&self, tag: &str, limit: Option<usize>) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 AND instr(tags, json_quote(?2)) > 0 ORDER BY timestamp DESC LIMIT ?3"),
            params![self.project_id, tag, limit_val(limit)],
        )
    }

    pub fn list_since(&self, since: DateTime<Utc>, limit: Option<usize>) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 AND timestamp >= ?2 ORDER BY timestamp DESC LIMIT ?3"),
            params![self.project_id, since.to_rfc3339(), limit_val(limit)],
        )
    }

    pub fn search(&self, query: &str) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 AND content LIKE ?2 ORDER BY timestamp DESC"),
            params![self.project_id, format!("%{query}%")],
        )
    }

    pub fn search_since(&self, query: &str, since: DateTime<Utc>) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 AND content LIKE ?2 AND timestamp >= ?3 ORDER BY timestamp DESC"),
            params![self.project_id, format!("%{query}%"), since.to_rfc3339()],
        )
    }

    pub fn save_at(&self, content: &str, tags: &[String], timestamp: DateTime<Utc>) -> Result<i64> {
        let tags_json = serde_json::to_string(tags)?;
        self.conn.execute(
            "INSERT INTO entries (timestamp, content, tags, project_id, session_id) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                timestamp.to_rfc3339(),
                content,
                tags_json,
                self.project_id,
                self.session_id,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn prune(&self, before: DateTime<Utc>) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM entries WHERE project_id = ?1 AND timestamp < ?2",
            params![self.project_id, before.to_rfc3339()],
        )?)
    }

    /// Export all entries in ascending timestamp order (distinct from `list` which is DESC + limited).
    pub fn export_all(&self) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 ORDER BY timestamp ASC"),
            params![self.project_id],
        )
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>> {
        self.fetch_one(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE id = ?1 AND project_id = ?2"),
            params![id, self.project_id],
        )
    }

    pub fn update(&self, id: i64, content: &str, tags: &[String]) -> Result<bool> {
        let tags_json = serde_json::to_string(tags)?;
        let n = self.conn.execute(
            "UPDATE entries SET content = ?1, tags = ?2 WHERE id = ?3 AND project_id = ?4",
            params![content, tags_json, id, self.project_id],
        )?;
        Ok(n > 0)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM entries WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        Ok(n > 0)
    }

    pub fn clear(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM entries WHERE project_id = ?1",
            params![self.project_id],
        )?)
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
        // Similar iteration pattern to `all_tags` but deduplicates and preserves recency order.
        let mut stmt = self.conn.prepare(
            "SELECT tags FROM entries WHERE project_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![self.project_id, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;

        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        for row in rows {
            let ts: Vec<String> = serde_json::from_str(&row?).unwrap_or_default();
            for t in ts {
                if seen.insert(t.clone()) {
                    tags.push(t);
                }
            }
        }
        Ok(tags)
    }

    pub fn all_tags(&self) -> Result<Vec<(String, usize)>> {
        // Similar iteration pattern to `recent_tags` but counts occurrences and sorts by frequency.
        let mut stmt = self.conn.prepare(
            "SELECT tags FROM entries WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map(params![self.project_id], |row| {
            row.get::<_, String>(0)
        })?;

        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for row in rows {
            let ts: Vec<String> = serde_json::from_str(&row?).unwrap_or_default();
            for t in ts {
                *counts.entry(t).or_insert(0) += 1;
            }
        }

        let mut result: Vec<(String, usize)> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(result)
    }

    /// Return all open (non-done) todos across all time.
    pub fn list_open_todos(&self) -> Result<Vec<Entry>> {
        self.fetch(
            &format!(
                "SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 \
                 AND LOWER(content) LIKE 'todo:%' AND status = 'active' \
                 ORDER BY timestamp ASC"
            ),
            params![self.project_id],
        )
    }

    /// Mark a todo (or any entry) as done.
    pub fn complete_todo(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE entries SET status = 'done' WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        Ok(n > 0)
    }

    /// Pin an entry so it always appears in context.
    pub fn pin(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE entries SET pinned = 1 WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        Ok(n > 0)
    }

    /// Remove pin from an entry.
    pub fn unpin(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE entries SET pinned = 0 WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        Ok(n > 0)
    }

    /// Return all pinned entries in chronological order.
    pub fn list_pinned(&self) -> Result<Vec<Entry>> {
        self.fetch(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 AND pinned = 1 ORDER BY timestamp ASC"),
            params![self.project_id],
        )
    }

    /// Return the most recent recap entry.
    pub fn last_recap(&self) -> Result<Option<Entry>> {
        self.fetch_one(
            &format!(
                "SELECT {SELECT_COLS} FROM entries WHERE project_id = ?1 \
                 AND LOWER(content) LIKE 'recap:%' ORDER BY timestamp DESC LIMIT 1"
            ),
            params![self.project_id],
        )
    }

    /// Returns true if an entry with this exact content and timestamp already exists.
    pub fn has_entry_by_signature(&self, content: &str, timestamp: DateTime<Utc>) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE project_id = ?1 AND content = ?2 AND timestamp = ?3",
            params![self.project_id, content, timestamp.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Returns true if there's already an entry with this exact content logged in the last `seconds`.
    /// Used to avoid duplicate auto-capture entries.
    pub fn has_recent_entry(&self, content: &str, seconds: i64) -> Result<bool> {
        let since = Utc::now() - chrono::Duration::seconds(seconds);
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE project_id = ?1 AND content = ?2 AND timestamp > ?3",
            params![self.project_id, content, since.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Check if there are entries newer than `since` (used for --once inject guard).
    pub fn has_entries_since(&self, since: DateTime<Utc>) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE project_id = ?1 AND timestamp > ?2",
            params![self.project_id, since.to_rfc3339()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Core fetch helper: runs a prepared statement and maps rows to entries.
    fn fetch(&self, sql: &str, params: impl rusqlite::Params) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(sql)?;
        stmt.query_map(params, map_row)?
            .map(|r| r.map_err(anyhow::Error::from))
            .collect()
    }

    /// Fetch at most one entry row. Shared by `get` and `last_recap`.
    fn fetch_one(&self, sql: &str, params: impl rusqlite::Params) -> Result<Option<Entry>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query_map(params, map_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }
}

/// Returns (timestamp, message) pairs from the last `limit` git commits in the given dir.
pub fn git_log(project_dir: &Path, limit: usize) -> Vec<(DateTime<Utc>, String)> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &project_dir.to_string_lossy(),
            "log",
            &format!("-{limit}"),
            "--format=%aI|%s",
        ])
        .output();

    let Ok(output) = output else { return vec![] };
    if !output.status.success() {
        return vec![];
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (ts_str, msg) = line.split_once('|')?;
            let ts = chrono::DateTime::parse_from_rfc3339(ts_str.trim())
                .ok()
                .map(|dt| dt.with_timezone(&Utc))?;
            Some((ts, msg.trim().to_string()))
        })
        .collect()
}

/// Map a SQLite row (columns: id, timestamp, content, tags, project_id, session_id, status, pinned) to an Entry.
fn map_row(row: &Row<'_>) -> rusqlite::Result<Entry> {
    let ts_str: String = row.get(1)?;
    let tags_json: String = row.get(3)?;
    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Entry {
        id: row.get(0)?,
        timestamp,
        content: row.get(2)?,
        tags,
        project_id: row.get(4)?,
        session_id: row.get(5)?,
        status: row.get(6).unwrap_or_else(|_| "active".to_string()),
        pinned: row.get::<_, i64>(7).unwrap_or(0) != 0,
    })
}

/// Convert an optional limit to a SQLite LIMIT value. -1 means no limit.
#[inline]
fn limit_val(limit: Option<usize>) -> i64 {
    limit.map(|n| n as i64).unwrap_or(-1)
}

/// Detect a stable project identifier from the git remote URL, or fall back to a
/// hash of the canonical directory path.
// Falls back to canonical path hash if not in a git repo or no remote set.
fn detect_project_id(dir: &Path) -> Result<String> {
    if let Ok(output) = std::process::Command::new("git")
        .args(["-C", &dir.to_string_lossy(), "remote", "get-url", "origin"])
        .output()
        && output.status.success()
    {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !url.is_empty() {
            return Ok(hash_str(&url));
        }
    }
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    Ok(hash_str(&canonical.to_string_lossy()))
}

fn hash_str(s: &str) -> String {
    let bytes = Sha256::digest(s.as_bytes());
    bytes.iter().take(8).fold(String::with_capacity(16), |mut acc, b| {
        acc.push_str(&format!("{b:02x}"));
        acc
    })
}

fn db_path(project_id: &str) -> Result<PathBuf> {
    Ok(dirs_base()?.join(format!("{project_id}.db")))
}

fn dirs_base() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("MEMO_DB_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find data dir"))?
        .join("memo"))
}

/// Generate a session ID using nanosecond timestamp combined with the process ID
/// to reduce collision probability on fast machines.
fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("{nanos:x}{pid:x}")
}

const MIGRATIONS: &[&str] = &[
    // v2: add status column for todo lifecycle
    "ALTER TABLE entries ADD COLUMN status TEXT NOT NULL DEFAULT 'active';",
    // v3: add pinned column
    "ALTER TABLE entries ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;",
];

/// Apply any pending schema migrations.
///
/// The `schema_version` table stores the highest applied version number.
/// If the version is 0 (fresh DB that has never stored a version row), we
/// record version 1 and set `applied = 1`.  Each migration in `MIGRATIONS` is
/// numbered starting at 2, so a fresh DB with `applied = 1` will correctly
/// run all migrations on the first open.
fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
    )?;
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if version == 0 {
        conn.execute("INSERT INTO schema_version (version) VALUES (?1)", params![1])?;
    }

    // If version was 0 we just inserted 1, so set applied = 1 to skip no migrations
    // that were already handled by the base schema (MIGRATIONS starts at v2).
    let applied = if version == 0 { 1i64 } else { version };
    for (i, migration) in MIGRATIONS.iter().enumerate() {
        let migration_version = i as i64 + 2;
        if applied < migration_version {
            conn.execute_batch(migration)?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![migration_version],
            )?;
        }
    }

    Ok(())
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    started_at  TEXT NOT NULL,
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
impl Store {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        run_migrations(&conn)?;
        let project_id = "test_project".to_string();
        let session_id = new_session_id();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, started_at, project_id) VALUES (?1, ?2, ?3)",
            params![session_id, Utc::now().to_rfc3339(), project_id],
        )?;
        Ok(Self { conn, project_id, session_id })
    }
}

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
    fn test_save_returns_rowid() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store.save("first", &[]).unwrap();
        let id2 = store.save("second", &[]).unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
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
        assert_eq!(store.clear().unwrap(), 1);
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

    #[test]
    fn test_list_by_tag() {
        let store = Store::open_in_memory().unwrap();
        store.save("fix auth", &["bug".to_string(), "auth".to_string()]).unwrap();
        store.save("refactor db", &["refactor".to_string()]).unwrap();
        store.save("fix typo", &["bug".to_string()]).unwrap();
        assert_eq!(store.list_by_tag("bug", None).unwrap().len(), 2);
        assert_eq!(store.list_by_tag("bug", Some(1)).unwrap().len(), 1);
    }

    #[test]
    fn test_delete() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save("to delete", &[]).unwrap();
        assert!(store.delete(id).unwrap());
        assert_eq!(store.count().unwrap(), 0);
        assert!(!store.delete(id).unwrap());
    }

    #[test]
    fn test_all_tags() {
        let store = Store::open_in_memory().unwrap();
        store.save("a", &["bug".to_string(), "auth".to_string()]).unwrap();
        store.save("b", &["bug".to_string()]).unwrap();
        store.save("c", &["refactor".to_string()]).unwrap();
        let tags = store.all_tags().unwrap();
        assert_eq!(tags[0], ("bug".to_string(), 2));
    }

    #[test]
    fn test_list_since() {
        let store = Store::open_in_memory().unwrap();
        store.save("old entry", &[]).unwrap();
        let now = Utc::now();
        store.save("new entry", &[]).unwrap();
        let entries = store.list_since(now, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "new entry");
    }

    #[test]
    fn test_todo_lifecycle() {
        let store = Store::open_in_memory().unwrap();
        let id = store.save("todo: implement feature", &[]).unwrap();
        store.save("todo: fix bug", &[]).unwrap();
        let todos = store.list_open_todos().unwrap();
        assert_eq!(todos.len(), 2);

        let done = store.complete_todo(id).unwrap();
        assert!(done);

        let todos_after = store.list_open_todos().unwrap();
        assert_eq!(todos_after.len(), 1);
        assert_eq!(todos_after[0].content, "todo: fix bug");
    }

    #[test]
    fn test_last_recap() {
        let store = Store::open_in_memory().unwrap();
        store.save("recap: did some work", &[]).unwrap();
        store.save("recap: finished the feature", &[]).unwrap();
        let recap = store.last_recap().unwrap();
        assert!(recap.is_some());
        assert_eq!(recap.unwrap().content, "recap: finished the feature");
    }

    #[test]
    fn test_has_entries_since() {
        let store = Store::open_in_memory().unwrap();
        store.save("old entry", &[]).unwrap();
        let now = Utc::now();
        assert!(!store.has_entries_since(now).unwrap());
        store.save("new entry", &[]).unwrap();
        assert!(store.has_entries_since(now).unwrap());
    }
}
