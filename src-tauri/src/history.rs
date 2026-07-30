// Local transcription history persisted in SQLite alongside config.json
// ({config_dir}/gladiaflow/history.db). Survives app upgrades and WebView cache clears.
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionHistoryEntry {
    pub id: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionHistoryPage {
    pub items: Vec<TranscriptionHistoryEntry>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

struct Row {
    session_id: String,
    text: String,
    created_at: String,
}

const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;

static DB_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn db_mutex() -> &'static Mutex<()> {
    DB_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn get_history_db_path() -> PathBuf {
    crate::config::get_config_path()
        .parent()
        .map(|p| p.join("history.db"))
        .unwrap_or_else(|| PathBuf::from("history.db"))
}

fn with_connection<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&Connection) -> Result<T, String>,
{
    let _guard = db_mutex().lock().map_err(|e| e.to_string())?;
    let path = get_history_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    init_schema(&conn)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    f(&conn)
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS transcriptions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id   TEXT NOT NULL UNIQUE,
            text         TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_transcriptions_created_at
            ON transcriptions(created_at DESC);
        "#,
    )
    .map_err(|e| e.to_string())
}

pub fn save_entry(session_id: &str, text: &str) -> Result<(), String> {
    let session_id = session_id.trim();
    let trimmed = text.trim();
    if session_id.is_empty() || trimmed.is_empty() {
        return Ok(());
    }
    let created_at = chrono::Utc::now().to_rfc3339();
    with_connection(|conn| {
        conn.execute(
            "INSERT INTO transcriptions (session_id, text, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
                text = excluded.text,
                created_at = excluded.created_at",
            params![session_id, trimmed, created_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn list_entries(
    query: Option<&str>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<TranscriptionHistoryPage, String> {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let query = query.map(str::trim).filter(|q| !q.is_empty());

    with_connection(|conn| list_entries_conn(conn, query, page, page_size))
}

fn list_entries_conn(
    conn: &Connection,
    query: Option<&str>,
    page: u32,
    page_size: u32,
) -> Result<TranscriptionHistoryPage, String> {
    if let Some(q) = query {
        let rows = load_all_rows(conn)?;
        let filtered = fuzzy_filter(rows, q);
        let total = filtered.len() as u32;
        let offset = ((page - 1) * page_size) as usize;
        let items = filtered
            .into_iter()
            .skip(offset)
            .take(page_size as usize)
            .map(row_to_entry)
            .collect();
        return Ok(TranscriptionHistoryPage {
            items,
            total,
            page,
            page_size,
        });
    }

    let total: u32 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let offset = (page - 1) * page_size;
    let mut stmt = conn
        .prepare(
            "SELECT session_id, text, created_at FROM transcriptions
             ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(params![page_size, offset], |row| {
            Ok(Row {
                session_id: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(row_to_entry)
        .collect();

    Ok(TranscriptionHistoryPage {
        items,
        total,
        page,
        page_size,
    })
}

fn load_all_rows(conn: &Connection) -> Result<Vec<Row>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT session_id, text, created_at FROM transcriptions ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Row {
                session_id: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn fuzzy_filter(mut rows: Vec<Row>, query: &str) -> Vec<Row> {
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

    let mut scored: Vec<(u32, Row)> = rows
        .drain(..)
        .filter_map(|row| {
            let mut buf = Vec::new();
            let haystack = Utf32Str::new(&row.text, &mut buf);
            let score = pattern.score(haystack, &mut matcher)?;
            Some((score, row))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.created_at.cmp(&a.1.created_at))
    });
    scored.into_iter().map(|(_, row)| row).collect()
}

fn row_to_entry(row: Row) -> TranscriptionHistoryEntry {
    TranscriptionHistoryEntry {
        id: row.session_id,
        text: row.text,
        created_at: row.created_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn save_and_list_newest_first() {
        let conn = test_conn();
        save_entry_conn(&conn, "sess-1", "hello world").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        save_entry_conn(&conn, "sess-2", "second entry").unwrap();

        let page = list_entries_conn(&conn, None, 1, 20).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].id, "sess-2");
        assert_eq!(page.items[1].id, "sess-1");
    }

    #[test]
    fn pagination_returns_correct_slice() {
        let conn = test_conn();
        for i in 0..25 {
            save_entry_conn(
                &conn,
                &format!("sess-{i}"),
                &format!("transcription number {i}"),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let page1 = list_entries_conn(&conn, None, 1, 20).unwrap();
        assert_eq!(page1.total, 25);
        assert_eq!(page1.items.len(), 20);

        let page2 = list_entries_conn(&conn, None, 2, 20).unwrap();
        assert_eq!(page2.items.len(), 5);
    }

    #[test]
    fn fuzzy_search_matches_with_typo() {
        let conn = test_conn();
        save_entry_conn(&conn, "sess-a", "the quick brown fox").unwrap();
        save_entry_conn(&conn, "sess-b", "something else entirely").unwrap();

        let page = list_entries_conn(&conn, Some("quik fox"), 1, 20).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, "sess-a");
    }

    #[test]
    fn duplicate_session_id_is_idempotent() {
        let conn = test_conn();
        save_entry_conn(&conn, "sess-dup", "first version").unwrap();
        save_entry_conn(&conn, "sess-dup", "updated version").unwrap();

        let page = list_entries_conn(&conn, None, 1, 20).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].text, "updated version");
    }

    #[test]
    fn skips_empty_text() {
        let conn = test_conn();
        save_entry_conn(&conn, "sess-empty", "   ").unwrap();
        let page = list_entries_conn(&conn, None, 1, 20).unwrap();
        assert_eq!(page.total, 0);
    }

    fn save_entry_conn(conn: &Connection, session_id: &str, text: &str) -> Result<(), String> {
        let session_id = session_id.trim();
        let trimmed = text.trim();
        if session_id.is_empty() || trimmed.is_empty() {
            return Ok(());
        }
        let created_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO transcriptions (session_id, text, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
                text = excluded.text,
                created_at = excluded.created_at",
            params![session_id, trimmed, created_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
