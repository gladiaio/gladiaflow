// Local transcription history persisted in SQLite alongside config.json
// ({config_dir}/gladiaflow/history.db). Survives app upgrades and WebView cache clears.
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionHistoryEntry {
    pub id: String,
    pub text: String,
    pub created_at: String,
    pub kind: String,
    pub utterances: Vec<MeetingHistoryUtterance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingHistoryUtterance {
    pub text: String,
    pub channel: u32,
    pub owner: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<u32>,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub end: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionHistoryPage {
    pub items: Vec<TranscriptionHistoryEntry>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRecord {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub summary: String,
    pub folder: String,
    pub duration: f64,
    pub speaker_names: BTreeMap<String, String>,
    pub text: String,
    pub utterances: Vec<MeetingHistoryUtterance>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingUpdate {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub summary: Option<String>,
    pub folder: Option<String>,
    pub duration: Option<f64>,
    pub speaker_names: Option<BTreeMap<String, String>>,
}

struct Row {
    session_id: String,
    text: String,
    created_at: String,
    kind: String,
    utterances_json: Option<String>,
}

struct MeetingRow {
    session_id: String,
    text: String,
    created_at: String,
    updated_at: String,
    utterances_json: Option<String>,
    title: String,
    notes: String,
    summary: String,
    folder: String,
    duration: f64,
    speaker_names_json: String,
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
    .map_err(|e| e.to_string())?;
    let columns = conn
        .prepare("PRAGMA table_info(transcriptions)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|column| column == "kind") {
        conn.execute(
            "ALTER TABLE transcriptions ADD COLUMN kind TEXT NOT NULL DEFAULT 'dictation'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    if !columns.iter().any(|column| column == "utterances_json") {
        conn.execute(
            "ALTER TABLE transcriptions ADD COLUMN utterances_json TEXT",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    add_column_if_missing(conn, &columns, "title", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, &columns, "notes", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, &columns, "summary", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, &columns, "folder", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, &columns, "duration", "REAL NOT NULL DEFAULT 0")?;
    add_column_if_missing(
        conn,
        &columns,
        "speaker_names_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    add_column_if_missing(conn, &columns, "updated_at", "TEXT")?;
    conn.execute(
        "UPDATE transcriptions SET updated_at = created_at
         WHERE updated_at IS NULL OR updated_at = ''",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    columns: &[String],
    name: &str,
    definition: &str,
) -> Result<(), String> {
    if columns.iter().any(|column| column == name) {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE transcriptions ADD COLUMN {name} {definition}"),
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn save_meeting_entry(
    session_id: &str,
    utterances: &[MeetingHistoryUtterance],
) -> Result<(), String> {
    let text = utterances
        .iter()
        .map(|utterance| utterance.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if session_id.trim().is_empty() || text.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(utterances).map_err(|e| e.to_string())?;
    let timestamp = chrono::Utc::now().to_rfc3339();
    let duration = utterances
        .iter()
        .map(|utterance| utterance.end)
        .filter(|end| end.is_finite())
        .fold(0.0, f64::max);
    with_connection(|conn| {
        conn.execute(
            "INSERT INTO transcriptions
                (session_id, text, created_at, updated_at, kind, utterances_json, duration)
             VALUES (?1, ?2, ?3, ?3, 'meeting', ?4, ?5)
             ON CONFLICT(session_id) DO UPDATE SET
                text = excluded.text,
                updated_at = excluded.updated_at,
                kind = excluded.kind,
                utterances_json = excluded.utterances_json,
                duration = excluded.duration",
            params![session_id.trim(), text, timestamp, json, duration],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn list_meetings() -> Result<Vec<MeetingRecord>, String> {
    with_connection(list_meetings_conn)
}

pub fn get_meeting(id: &str) -> Result<Option<MeetingRecord>, String> {
    with_connection(|conn| get_meeting_conn(conn, id.trim()))
}

pub fn update_meeting(id: &str, update: MeetingUpdate) -> Result<MeetingRecord, String> {
    with_connection(|conn| update_meeting_conn(conn, id.trim(), update))
}

pub fn delete_meeting(id: &str) -> Result<bool, String> {
    with_connection(|conn| delete_meeting_conn(conn, id.trim()))
}

pub fn meeting_markdown(id: &str) -> Result<String, String> {
    let meeting = get_meeting(id)?.ok_or_else(|| "Meeting not found".to_string())?;
    Ok(render_meeting_markdown(&meeting))
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
            "INSERT INTO transcriptions (session_id, text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
                text = excluded.text,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at",
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
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
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
            "SELECT session_id, text, created_at, kind, utterances_json FROM transcriptions
             ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;
    let items = stmt
        .query_map(params![page_size, offset], |row| {
            Ok(Row {
                session_id: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
                kind: row.get(3)?,
                utterances_json: row.get(4)?,
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
            "SELECT session_id, text, created_at, kind, utterances_json
             FROM transcriptions ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Row {
                session_id: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
                kind: row.get(3)?,
                utterances_json: row.get(4)?,
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

fn list_meetings_conn(conn: &Connection) -> Result<Vec<MeetingRecord>, String> {
    let mut statement = conn
        .prepare(
            "SELECT session_id, text, created_at, COALESCE(updated_at, created_at),
                    utterances_json, title, notes, summary, folder, duration, speaker_names_json
             FROM transcriptions
             WHERE kind = 'meeting'
             ORDER BY created_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], meeting_row_from_sql)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().map(meeting_row_to_record).collect())
}

fn get_meeting_conn(conn: &Connection, id: &str) -> Result<Option<MeetingRecord>, String> {
    if id.is_empty() {
        return Ok(None);
    }
    let mut statement = conn
        .prepare(
            "SELECT session_id, text, created_at, COALESCE(updated_at, created_at),
                    utterances_json, title, notes, summary, folder, duration, speaker_names_json
             FROM transcriptions
             WHERE session_id = ?1 AND kind = 'meeting'",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement
        .query_map([id], meeting_row_from_sql)
        .map_err(|error| error.to_string())?;
    rows.next()
        .transpose()
        .map(|row| row.map(meeting_row_to_record))
        .map_err(|error| error.to_string())
}

fn meeting_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingRow> {
    Ok(MeetingRow {
        session_id: row.get(0)?,
        text: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        utterances_json: row.get(4)?,
        title: row.get(5)?,
        notes: row.get(6)?,
        summary: row.get(7)?,
        folder: row.get(8)?,
        duration: row.get(9)?,
        speaker_names_json: row.get(10)?,
    })
}

fn meeting_row_to_record(row: MeetingRow) -> MeetingRecord {
    MeetingRecord {
        id: row.session_id,
        title: row.title,
        notes: row.notes,
        summary: row.summary,
        folder: row.folder,
        duration: row.duration,
        speaker_names: serde_json::from_str(&row.speaker_names_json).unwrap_or_default(),
        text: row.text,
        utterances: row
            .utterances_json
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn update_meeting_conn(
    conn: &Connection,
    id: &str,
    update: MeetingUpdate,
) -> Result<MeetingRecord, String> {
    let mut meeting = get_meeting_conn(conn, id)?.ok_or_else(|| "Meeting not found".to_string())?;
    if let Some(duration) = update.duration {
        if !duration.is_finite() || duration < 0.0 {
            return Err("Meeting duration must be a finite, non-negative number".to_string());
        }
        meeting.duration = duration;
    }
    if let Some(title) = update.title {
        meeting.title = title.trim().to_string();
    }
    if let Some(notes) = update.notes {
        meeting.notes = notes;
    }
    if let Some(summary) = update.summary {
        meeting.summary = summary;
    }
    if let Some(folder) = update.folder {
        meeting.folder = folder.trim().to_string();
    }
    if let Some(speaker_names) = update.speaker_names {
        meeting.speaker_names = speaker_names
            .into_iter()
            .filter_map(|(speaker, name)| {
                let speaker = speaker.trim().to_string();
                let name = name.trim().to_string();
                (!speaker.is_empty() && !name.is_empty()).then_some((speaker, name))
            })
            .collect();
    }
    meeting.updated_at = chrono::Utc::now().to_rfc3339();
    let speaker_names_json =
        serde_json::to_string(&meeting.speaker_names).map_err(|error| error.to_string())?;
    conn.execute(
        "UPDATE transcriptions
         SET title = ?2, notes = ?3, summary = ?4, folder = ?5, duration = ?6,
             speaker_names_json = ?7, updated_at = ?8
         WHERE session_id = ?1 AND kind = 'meeting'",
        params![
            id,
            meeting.title,
            meeting.notes,
            meeting.summary,
            meeting.folder,
            meeting.duration,
            speaker_names_json,
            meeting.updated_at,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(meeting)
}

fn delete_meeting_conn(conn: &Connection, id: &str) -> Result<bool, String> {
    if id.is_empty() {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM transcriptions WHERE session_id = ?1 AND kind = 'meeting'",
        [id],
    )
    .map(|deleted| deleted > 0)
    .map_err(|error| error.to_string())
}

fn render_meeting_markdown(meeting: &MeetingRecord) -> String {
    let title = if meeting.title.is_empty() {
        "Untitled meeting"
    } else {
        &meeting.title
    };
    let mut markdown = format!("# {title}\n\n- Date: {}", meeting.created_at);
    if meeting.duration > 0.0 {
        markdown.push_str(&format!(
            "\n- Duration: {}",
            format_duration(meeting.duration)
        ));
    }
    if !meeting.folder.is_empty() {
        markdown.push_str(&format!("\n- Folder: {}", meeting.folder));
    }
    if !meeting.summary.trim().is_empty() {
        markdown.push_str("\n\n## Summary\n\n");
        markdown.push_str(meeting.summary.trim());
    }
    if !meeting.notes.trim().is_empty() {
        markdown.push_str("\n\n## Notes\n\n");
        markdown.push_str(meeting.notes.trim());
    }
    markdown.push_str("\n\n## Transcript");
    for utterance in &meeting.utterances {
        let speaker = if utterance.owner {
            "You".to_string()
        } else if let Some(index) = utterance.speaker {
            let key = index.to_string();
            meeting
                .speaker_names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| format!("Speaker {}", index + 1))
        } else {
            meeting
                .speaker_names
                .get("others")
                .cloned()
                .unwrap_or_else(|| "Others".to_string())
        };
        markdown.push_str(&format!("\n\n**{speaker}:** {}", utterance.text.trim()));
    }
    markdown.push('\n');
    markdown
}

fn format_duration(seconds: f64) -> String {
    let total_seconds = seconds.round() as u64;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn row_to_entry(row: Row) -> TranscriptionHistoryEntry {
    TranscriptionHistoryEntry {
        id: row.session_id,
        text: row.text,
        created_at: row.created_at,
        kind: row.kind,
        utterances: row
            .utterances_json
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
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

    #[test]
    fn meeting_rows_restore_channel_ownership() {
        let conn = test_conn();
        let utterances = serde_json::json!([
            {"text":"hello","channel":0,"owner":true},
            {"text":"welcome","channel":1,"owner":false,"speaker":2}
        ]);
        conn.execute(
            "INSERT INTO transcriptions
                (session_id, text, created_at, kind, utterances_json)
             VALUES (?1, ?2, ?3, 'meeting', ?4)",
            params![
                "meeting-1",
                "hello welcome",
                "2026-01-01T00:00:00Z",
                utterances.to_string()
            ],
        )
        .unwrap();

        let page = list_entries_conn(&conn, None, 1, 20).unwrap();
        assert_eq!(page.items[0].kind, "meeting");
        assert!(page.items[0].utterances[0].owner);
        assert_eq!(page.items[0].utterances[1].channel, 1);
        assert_eq!(page.items[0].utterances[1].speaker, Some(2));
    }

    #[test]
    fn migrates_existing_rows_with_meeting_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE transcriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL UNIQUE,
                text TEXT NOT NULL,
                created_at TEXT NOT NULL
             );
             INSERT INTO transcriptions (session_id, text, created_at)
             VALUES ('legacy', 'old text', '2025-01-02T03:04:05Z');",
        )
        .unwrap();

        init_schema(&conn).unwrap();

        let values: (String, String, String, String, f64, String, String) = conn
            .query_row(
                "SELECT kind, title, notes, summary, duration, speaker_names_json, updated_at
                 FROM transcriptions WHERE session_id = 'legacy'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            values,
            (
                "dictation".to_string(),
                String::new(),
                String::new(),
                String::new(),
                0.0,
                "{}".to_string(),
                "2025-01-02T03:04:05Z".to_string(),
            )
        );
    }

    #[test]
    fn meeting_crud_does_not_modify_dictation_rows() {
        let conn = test_conn();
        save_entry_conn(&conn, "dictation-1", "keep me").unwrap();
        insert_meeting(&conn, "meeting-1");

        let meetings = list_meetings_conn(&conn).unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(meetings[0].id, "meeting-1");
        assert_eq!(meetings[0].duration, 8.5);
        assert!(meetings[0].title.is_empty());
        assert_eq!(meetings[0].created_at, meetings[0].updated_at);

        let update = MeetingUpdate {
            title: Some("  Weekly sync  ".to_string()),
            notes: Some("Follow up tomorrow.".to_string()),
            summary: Some("Decisions were made.".to_string()),
            folder: Some("  Team  ".to_string()),
            duration: Some(90.0),
            speaker_names: Some(BTreeMap::from([
                ("0".to_string(), " Alice ".to_string()),
                ("ignored".to_string(), " ".to_string()),
            ])),
        };
        let updated = update_meeting_conn(&conn, "meeting-1", update).unwrap();
        assert_eq!(updated.title, "Weekly sync");
        assert_eq!(updated.folder, "Team");
        assert_eq!(updated.speaker_names.get("0").unwrap(), "Alice");
        assert!(updated.updated_at >= updated.created_at);

        assert!(delete_meeting_conn(&conn, "meeting-1").unwrap());
        assert!(!delete_meeting_conn(&conn, "meeting-1").unwrap());
        assert!(get_meeting_conn(&conn, "meeting-1").unwrap().is_none());
        let dictation_count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM transcriptions WHERE session_id = 'dictation-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dictation_count, 1);
    }

    #[test]
    fn markdown_contains_metadata_and_named_transcript() {
        let conn = test_conn();
        insert_meeting(&conn, "meeting-md");
        let meeting = update_meeting_conn(
            &conn,
            "meeting-md",
            MeetingUpdate {
                title: Some("Planning".to_string()),
                summary: Some("Agreed on the scope.".to_string()),
                notes: Some("Send recap.".to_string()),
                folder: Some("Product".to_string()),
                speaker_names: Some(BTreeMap::from([
                    ("owner".to_string(), "Jordan".to_string()),
                    ("0".to_string(), "Alex".to_string()),
                ])),
                ..MeetingUpdate::default()
            },
        )
        .unwrap();

        let markdown = render_meeting_markdown(&meeting);
        assert!(markdown.starts_with("# Planning\n"));
        assert!(markdown.contains("- Duration: 0:09"));
        assert!(markdown.contains("## Summary\n\nAgreed on the scope."));
        assert!(markdown.contains("**You:** hello"));
        assert!(markdown.contains("**Alex:** welcome"));
    }

    #[test]
    fn meeting_update_rejects_invalid_duration() {
        let conn = test_conn();
        insert_meeting(&conn, "meeting-duration");
        let result = update_meeting_conn(
            &conn,
            "meeting-duration",
            MeetingUpdate {
                duration: Some(f64::NAN),
                ..MeetingUpdate::default()
            },
        );
        assert!(result.is_err());
    }

    fn insert_meeting(conn: &Connection, id: &str) {
        let utterances = serde_json::json!([
            {"text":"hello","channel":0,"owner":true,"start":0.0,"end":3.0},
            {"text":"welcome","channel":1,"owner":false,"speaker":0,"start":4.0,"end":8.5}
        ]);
        conn.execute(
            "INSERT INTO transcriptions
                (session_id, text, created_at, updated_at, kind, utterances_json, duration)
             VALUES (?1, 'hello welcome', '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z', 'meeting', ?2, 8.5)",
            params![id, utterances.to_string()],
        )
        .unwrap();
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
