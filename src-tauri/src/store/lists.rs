//! Lists (board columns). Users define their own workflow; the only special
//! column is one flagged `is_done_list`, which ties dropping a card to
//! completing it.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::List;
use crate::store::touch;
use crate::util::{POSITION_STEP, new_id, needs_rebalance, position_between};

pub fn map(row: &Row<'_>) -> rusqlite::Result<List> {
    Ok(List {
        id: row.get("id")?,
        board_id: row.get("board_id")?,
        name: row.get("name")?,
        position: row.get("position")?,
        is_done_list: row.get::<_, i64>("is_done_list")? != 0,
        wip_limit: row.get("wip_limit")?,
        archived: row.get::<_, i64>("archived")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const SELECT: &str = "SELECT id, board_id, name, position, is_done_list, wip_limit, archived, created_at, updated_at FROM lists";

pub fn for_board(conn: &Connection, board_id: &str) -> Result<Vec<List>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE board_id = ?1 AND archived = 0 ORDER BY position ASC"
    ))?;
    Ok(stmt
        .query_map(params![board_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<List> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("list"))
}

/// The column that means "finished" on a board, if the user kept one.
pub fn done_list(conn: &Connection, board_id: &str) -> Result<Option<List>> {
    Ok(conn
        .query_row(
            &format!("{SELECT} WHERE board_id = ?1 AND is_done_list = 1 AND archived = 0 ORDER BY position LIMIT 1"),
            params![board_id],
            map,
        )
        .optional()?)
}

/// First column on a board — the default landing spot for new tasks.
pub fn first_list(conn: &Connection, board_id: &str) -> Result<List> {
    conn.query_row(
        &format!("{SELECT} WHERE board_id = ?1 AND archived = 0 ORDER BY position LIMIT 1"),
        params![board_id],
        map,
    )
    .optional()?
    .ok_or_else(|| not_found("list"))
}

pub fn create(conn: &Connection, board_id: &str, name: &str, is_done_list: bool) -> Result<List> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rejected("Column name cannot be empty"));
    }
    let max: Option<f64> = conn.query_row(
        "SELECT MAX(position) FROM lists WHERE board_id = ?1",
        params![board_id],
        |r| r.get(0),
    )?;
    let id = new_id();
    let ts = touch();
    conn.execute(
        "INSERT INTO lists (id, board_id, name, position, is_done_list, wip_limit, archived, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, ?6, ?6)",
        params![id, board_id, name, max.unwrap_or(0.0) + POSITION_STEP, i64::from(is_done_list), ts],
    )?;
    if is_done_list {
        clear_other_done_lists(conn, board_id, &id)?;
    }
    get(conn, &id)
}

pub struct ListPatch {
    pub name: Option<String>,
    pub is_done_list: Option<bool>,
    pub wip_limit: Option<Option<i64>>,
    pub archived: Option<bool>,
}

pub fn update(conn: &Connection, id: &str, patch: ListPatch) -> Result<List> {
    let existing = get(conn, id)?;
    if let Some(name) = &patch.name
        && name.trim().is_empty()
    {
        return Err(rejected("Column name cannot be empty"));
    }
    conn.execute(
        "UPDATE lists SET
           name = COALESCE(?2, name),
           is_done_list = COALESCE(?3, is_done_list),
           wip_limit = CASE WHEN ?4 = 1 THEN ?5 ELSE wip_limit END,
           archived = COALESCE(?6, archived),
           updated_at = ?7
         WHERE id = ?1",
        params![
            id,
            patch.name.as_ref().map(|n| n.trim()),
            patch.is_done_list.map(i64::from),
            i64::from(patch.wip_limit.is_some()),
            patch.wip_limit.flatten(),
            patch.archived.map(i64::from),
            touch()
        ],
    )?;
    if patch.is_done_list == Some(true) {
        clear_other_done_lists(conn, &existing.board_id, id)?;
    }
    get(conn, id)
}

/// A board has at most one done column, so promoting one demotes the rest.
fn clear_other_done_lists(conn: &Connection, board_id: &str, keep: &str) -> Result<()> {
    conn.execute(
        "UPDATE lists SET is_done_list = 0 WHERE board_id = ?1 AND id <> ?2",
        params![board_id, keep],
    )?;
    Ok(())
}

/// Deletes a column. Refuses on the last remaining column so a board always has
/// somewhere to put a task.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let list = get(conn, id)?;
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lists WHERE board_id = ?1 AND archived = 0 AND id <> ?2",
        params![list.board_id, id],
        |r| r.get(0),
    )?;
    if remaining == 0 {
        return Err(rejected("A board needs at least one column"));
    }
    conn.execute("DELETE FROM lists WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn reorder(conn: &Connection, id: &str, index: usize) -> Result<Vec<List>> {
    let list = get(conn, id)?;
    let mut others: Vec<List> = for_board(conn, &list.board_id)?
        .into_iter()
        .filter(|l| l.id != id)
        .collect();
    let index = index.min(others.len());

    let before = index.checked_sub(1).and_then(|i| others.get(i)).map(|l| l.position);
    let after = others.get(index).map(|l| l.position);
    conn.execute(
        "UPDATE lists SET position = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, position_between(before, after), touch()],
    )?;

    if needs_rebalance(before, after) {
        others.insert(index, get(conn, id)?);
        for (i, l) in others.iter().enumerate() {
            conn.execute(
                "UPDATE lists SET position = ?2 WHERE id = ?1",
                params![l.id, (i as f64 + 1.0) * POSITION_STEP],
            )?;
        }
    }
    for_board(conn, &list.board_id)
}
