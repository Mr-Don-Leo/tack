//! Labels. A label with `board_id = NULL` is global and offered on every board.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::Label;
use crate::util::new_id;

pub fn map(row: &Row<'_>) -> rusqlite::Result<Label> {
    Ok(Label {
        id: row.get("id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        board_id: row.get("board_id")?,
        created_at: row.get("created_at")?,
    })
}

const SELECT: &str = "SELECT id, name, color, board_id, created_at FROM labels";

/// Every label the user can pick anywhere, for settings and global filters.
pub fn all(conn: &Connection) -> Result<Vec<Label>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY board_id IS NOT NULL, name COLLATE NOCASE"))?;
    Ok(stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Global labels plus the ones scoped to `board_id`.
pub fn for_board(conn: &Connection, board_id: &str) -> Result<Vec<Label>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE board_id IS NULL OR board_id = ?1 ORDER BY board_id IS NOT NULL, name COLLATE NOCASE"
    ))?;
    Ok(stmt
        .query_map(params![board_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn for_task(conn: &Connection, task_id: &str) -> Result<Vec<Label>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.name, l.color, l.board_id, l.created_at
         FROM labels l JOIN task_labels tl ON tl.label_id = l.id
         WHERE tl.task_id = ?1 ORDER BY l.name COLLATE NOCASE",
    )?;
    Ok(stmt
        .query_map(params![task_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Label> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("label"))
}

/// Looks a label up by name, case-insensitively, preferring a board-scoped one.
/// Used by quick-add `#tag` parsing.
pub fn find_by_name(conn: &Connection, name: &str, board_id: &str) -> Result<Option<Label>> {
    Ok(conn
        .query_row(
            &format!(
                "{SELECT} WHERE name = ?1 COLLATE NOCASE AND (board_id IS NULL OR board_id = ?2)
                 ORDER BY board_id IS NULL LIMIT 1"
            ),
            params![name.trim(), board_id],
            map,
        )
        .optional()?)
}

pub fn create(conn: &Connection, name: &str, color: &str, board_id: Option<&str>) -> Result<Label> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rejected("Label name cannot be empty"));
    }
    if !is_hex_color(color) {
        return Err(rejected("Label colour must be a hex value like #FF3B30"));
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO labels (id, name, color, board_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, color, board_id, crate::util::now()],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, name: Option<&str>, color: Option<&str>) -> Result<Label> {
    if let Some(color) = color
        && !is_hex_color(color)
    {
        return Err(rejected("Label colour must be a hex value like #FF3B30"));
    }
    if let Some(name) = name
        && name.trim().is_empty()
    {
        return Err(rejected("Label name cannot be empty"));
    }
    conn.execute(
        "UPDATE labels SET name = COALESCE(?2, name), color = COALESCE(?3, color) WHERE id = ?1",
        params![id, name.map(str::trim), color],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM labels WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn attach(conn: &Connection, task_id: &str, label_id: &str) -> Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO task_labels (task_id, label_id) VALUES (?1, ?2)",
        params![task_id, label_id],
    )?;
    Ok(changed > 0)
}

pub fn detach(conn: &Connection, task_id: &str, label_id: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM task_labels WHERE task_id = ?1 AND label_id = ?2",
        params![task_id, label_id],
    )?;
    Ok(changed > 0)
}

/// Rejects anything that is not `#rgb` or `#rrggbb`, so colours can be dropped
/// into CSS custom properties without further escaping.
fn is_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}
