//! Boards. Exactly one board is flagged `is_main`; it is permanent and always
//! present, and every other board is fully user-managed.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::Board;
use crate::store::touch;
use crate::util::{POSITION_STEP, new_id, position_between};

pub fn map(row: &Row<'_>) -> rusqlite::Result<Board> {
    Ok(Board {
        id: row.get("id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        icon: row.get("icon")?,
        position: row.get("position")?,
        is_main: row.get::<_, i64>("is_main")? != 0,
        archived: row.get::<_, i64>("archived")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const SELECT: &str = "SELECT id, name, color, icon, position, is_main, archived, created_at, updated_at FROM boards";

pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Board>> {
    let sql = format!(
        "{SELECT} {} ORDER BY is_main DESC, position ASC",
        if include_archived { "" } else { "WHERE archived = 0" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Board> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("board"))
}

/// The permanent default board. Guaranteed to exist by the seed migration.
pub fn main_board(conn: &Connection) -> Result<Board> {
    conn.query_row(&format!("{SELECT} WHERE is_main = 1"), [], map)
        .optional()?
        .ok_or_else(|| not_found("main board"))
}

/// Creates a board with a starter workflow so it is usable immediately.
pub fn create(conn: &Connection, name: &str, color: Option<&str>, icon: Option<&str>) -> Result<Board> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rejected("Board name cannot be empty"));
    }
    let ts = touch();
    let id = new_id();
    let max: Option<f64> = conn.query_row("SELECT MAX(position) FROM boards", [], |r| r.get(0))?;

    conn.execute(
        "INSERT INTO boards (id, name, color, icon, position, is_main, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?6)",
        params![id, name, color, icon, max.unwrap_or(0.0) + POSITION_STEP, ts],
    )?;

    for (i, list_name) in ["To Do", "In Progress", "Done"].iter().enumerate() {
        conn.execute(
            "INSERT INTO lists (id, board_id, name, position, is_done_list, wip_limit, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, ?6, ?6)",
            params![
                new_id(),
                id,
                list_name,
                (i as f64 + 1.0) * POSITION_STEP,
                i64::from(*list_name == "Done"),
                ts
            ],
        )?;
    }

    super::log_activity(conn, None, Some(&id), "board.created", &format!("Created board “{name}”"));
    get(conn, &id)
}

pub struct BoardPatch {
    pub name: Option<String>,
    pub color: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub archived: Option<bool>,
}

pub fn update(conn: &Connection, id: &str, patch: BoardPatch) -> Result<Board> {
    let board = get(conn, id)?;

    if let Some(name) = &patch.name
        && name.trim().is_empty()
    {
        return Err(rejected("Board name cannot be empty"));
    }
    // The Main Board must stay reachable, so it can never be archived.
    if board.is_main && patch.archived == Some(true) {
        return Err(rejected("The Main Board cannot be archived"));
    }

    conn.execute(
        "UPDATE boards SET
           name = COALESCE(?2, name),
           color = CASE WHEN ?3 = 1 THEN ?4 ELSE color END,
           icon = CASE WHEN ?5 = 1 THEN ?6 ELSE icon END,
           archived = COALESCE(?7, archived),
           updated_at = ?8
         WHERE id = ?1",
        params![
            id,
            patch.name.as_ref().map(|n| n.trim()),
            i64::from(patch.color.is_some()),
            patch.color.clone().flatten(),
            i64::from(patch.icon.is_some()),
            patch.icon.clone().flatten(),
            patch.archived.map(i64::from),
            touch()
        ],
    )?;
    get(conn, id)
}

/// Deletes a board and everything on it. The Main Board is protected.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let board = get(conn, id)?;
    if board.is_main {
        return Err(rejected("The Main Board cannot be deleted"));
    }
    conn.execute("DELETE FROM boards WHERE id = ?1", params![id])?;
    Ok(())
}

/// Reorders `id` to sit at `index` among the non-archived, non-main boards.
pub fn reorder(conn: &Connection, id: &str, index: usize) -> Result<Vec<Board>> {
    let mut others: Vec<Board> = list(conn, false)?
        .into_iter()
        .filter(|b| b.id != id && !b.is_main)
        .collect();
    let index = index.min(others.len());

    let before = index.checked_sub(1).and_then(|i| others.get(i)).map(|b| b.position);
    let after = others.get(index).map(|b| b.position);
    let position = position_between(before, after);

    conn.execute(
        "UPDATE boards SET position = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, position, touch()],
    )?;

    // Floats stop subdividing cleanly once neighbours converge; renumber then.
    if crate::util::needs_rebalance(before, after) {
        others.insert(index, get(conn, id)?);
        for (i, board) in others.iter().enumerate() {
            conn.execute(
                "UPDATE boards SET position = ?2 WHERE id = ?1",
                params![board.id, (i as f64 + 1.0) * POSITION_STEP],
            )?;
        }
    }
    list(conn, false)
}
