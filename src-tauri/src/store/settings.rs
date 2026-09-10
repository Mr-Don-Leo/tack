//! Application settings, stored as a flat key/value table of JSON values.
//!
//! Unknown keys are preserved on read, so a setting added by a newer build is
//! not silently dropped by an older one.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use crate::error::Result;

/// Defaults merged under whatever the user has saved.
pub fn defaults() -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("theme".into(), json!("system")),
        ("skin".into(), json!("apple")),
        ("quickAddShortcut".into(), json!(default_shortcut())),
        ("closeToTray".into(), json!(true)),
        ("startMinimized".into(), json!(false)),
        ("notificationsEnabled".into(), json!(true)),
        ("notificationSound".into(), json!(true)),
        ("snoozeMinutes".into(), json!(10)),
        ("defaultReminderOffsets".into(), json!([0, 10, 60, 1440])),
        ("backupIntervalHours".into(), json!(6)),
        // Index into a Monday-first week: 0 = Monday, 6 = Sunday.
        ("weekStartsOn".into(), json!(0)),
        ("lastBoardId".into(), Value::Null),
        ("lastBackupAt".into(), Value::Null),
    ])
}

fn default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "Command+Shift+Space"
    } else {
        "Control+Shift+Space"
    }
}

pub fn all(conn: &Connection) -> Result<BTreeMap<String, Value>> {
    let mut settings = defaults();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>("key")?, row.get::<_, String>("value")?))
    })?;
    for row in rows {
        let (key, raw) = row?;
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            settings.insert(key, value);
        }
    }
    Ok(settings)
}

pub fn get(conn: &Connection, key: &str) -> Result<Value> {
    Ok(all(conn)?.get(key).cloned().unwrap_or(Value::Null))
}

pub fn get_string(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(get(conn, key)?.as_str().map(str::to_string))
}

pub fn get_bool(conn: &Connection, key: &str, fallback: bool) -> bool {
    get(conn, key)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(fallback)
}

pub fn get_i64(conn: &Connection, key: &str, fallback: i64) -> i64 {
    get(conn, key).ok().and_then(|v| v.as_i64()).unwrap_or(fallback)
}

pub fn set(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

pub fn set_many(conn: &Connection, values: &BTreeMap<String, Value>) -> Result<()> {
    for (key, value) in values {
        set(conn, key, value)?;
    }
    Ok(())
}
