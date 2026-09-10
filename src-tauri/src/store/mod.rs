//! Data access. Every module here takes a `&Connection` and does no I/O beyond
//! the database, so the reminder/automation engine can reuse it without a UI.

pub mod attachments;
pub mod automations;
pub mod boards;
pub mod labels;
pub mod lists;
pub mod query;
pub mod reminders;
pub mod settings;
pub mod tasks;

use rusqlite::Row;

use crate::models::Timestamp;
use crate::util::new_id;

/// Records a line of task history. Failures here are non-fatal: losing an audit
/// line must never abort the user's actual edit.
pub fn log_activity(
    conn: &rusqlite::Connection,
    task_id: Option<&str>,
    board_id: Option<&str>,
    kind: &str,
    message: &str,
) {
    let _ = conn.execute(
        "INSERT INTO activity (id, task_id, board_id, kind, message, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![new_id(), task_id, board_id, kind, message, crate::util::now()],
    );
}

/// Reads a nullable JSON column, treating malformed values as absent so one bad
/// row cannot break a whole list query.
pub fn json_column<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    idx: &str,
) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = row.get(idx)?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

/// Serializes an optional value for storage in a nullable JSON column.
pub fn json_value<T: serde::Serialize>(value: &Option<T>) -> Option<String> {
    value.as_ref().and_then(|v| serde_json::to_string(v).ok())
}

pub fn touch() -> Timestamp {
    crate::util::now()
}
