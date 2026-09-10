//! Automation rules. Trigger, conditions and actions are stored as JSON so the
//! rule vocabulary can grow without a migration.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::{Automation, AutomationAction, Condition, Trigger};
use crate::store::touch;
use crate::util::{POSITION_STEP, new_id, now};

pub fn map(row: &Row<'_>) -> rusqlite::Result<Automation> {
    let parse = |raw: String, column: &str| -> rusqlite::Result<serde_json::Value> {
        serde_json::from_str(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{column}: {e}")),
            ))
        })
    };
    let trigger: Trigger = serde_json::from_value(parse(row.get("trigger")?, "trigger")?)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
    let conditions: Vec<Condition> =
        serde_json::from_value(parse(row.get("conditions")?, "conditions")?).unwrap_or_default();
    let actions: Vec<AutomationAction> =
        serde_json::from_value(parse(row.get("actions")?, "actions")?).unwrap_or_default();

    Ok(Automation {
        id: row.get("id")?,
        name: row.get("name")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        board_id: row.get("board_id")?,
        trigger,
        conditions,
        actions,
        last_run_at: row.get("last_run_at")?,
        run_count: row.get("run_count")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

const SELECT: &str = "SELECT id, name, enabled, board_id, trigger, conditions, actions,
        last_run_at, run_count, position, created_at, updated_at FROM automations";

/// All rules, newest board-scoped last. Rows that fail to parse (for example
/// written by a newer build) are skipped rather than failing the whole list.
pub fn all(conn: &Connection) -> Result<Vec<Automation>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY board_id IS NOT NULL, position ASC"))?;
    let rows = stmt.query_map([], map)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Rules that apply to `board_id`: global ones plus that board's own.
pub fn for_board(conn: &Connection, board_id: &str) -> Result<Vec<Automation>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE board_id IS NULL OR board_id = ?1 ORDER BY board_id IS NOT NULL, position ASC"
    ))?;
    let rows = stmt.query_map(params![board_id], map)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Enabled rules that apply to `board_id`, used on every trigger evaluation.
pub fn active_for_board(conn: &Connection, board_id: &str) -> Result<Vec<Automation>> {
    Ok(for_board(conn, board_id)?.into_iter().filter(|a| a.enabled).collect())
}

/// Enabled rules with a wall-clock trigger, evaluated by the background engine.
pub fn active_scheduled(conn: &Connection) -> Result<Vec<Automation>> {
    Ok(all(conn)?
        .into_iter()
        .filter(|a| a.enabled && matches!(a.trigger, Trigger::Scheduled { .. }))
        .collect())
}

pub fn get(conn: &Connection, id: &str) -> Result<Automation> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("automation"))
}

pub struct NewAutomation {
    pub name: String,
    pub board_id: Option<String>,
    pub trigger: Trigger,
    pub conditions: Vec<Condition>,
    pub actions: Vec<AutomationAction>,
    pub enabled: bool,
}

pub fn create(conn: &Connection, input: NewAutomation) -> Result<Automation> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(rejected("An automation needs a name"));
    }
    if input.actions.is_empty() {
        return Err(rejected("An automation needs at least one action"));
    }
    let max: Option<f64> = conn.query_row("SELECT MAX(position) FROM automations", [], |r| r.get(0))?;
    let id = new_id();
    let ts = now();
    conn.execute(
        "INSERT INTO automations (id, name, enabled, board_id, trigger, conditions, actions,
                                  last_run_at, run_count, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 0, ?8, ?9, ?9)",
        params![
            id,
            name,
            i64::from(input.enabled),
            input.board_id,
            serde_json::to_string(&input.trigger)?,
            serde_json::to_string(&input.conditions)?,
            serde_json::to_string(&input.actions)?,
            max.unwrap_or(0.0) + POSITION_STEP,
            ts
        ],
    )?;
    get(conn, &id)
}

#[derive(Default)]
pub struct AutomationPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub board_id: Option<Option<String>>,
    pub trigger: Option<Trigger>,
    pub conditions: Option<Vec<Condition>>,
    pub actions: Option<Vec<AutomationAction>>,
}

pub fn update(conn: &Connection, id: &str, patch: AutomationPatch) -> Result<Automation> {
    if let Some(name) = &patch.name
        && name.trim().is_empty()
    {
        return Err(rejected("An automation needs a name"));
    }
    if let Some(actions) = &patch.actions
        && actions.is_empty()
    {
        return Err(rejected("An automation needs at least one action"));
    }
    conn.execute(
        "UPDATE automations SET
           name = COALESCE(?2, name),
           enabled = COALESCE(?3, enabled),
           board_id = CASE WHEN ?4 = 1 THEN ?5 ELSE board_id END,
           trigger = COALESCE(?6, trigger),
           conditions = COALESCE(?7, conditions),
           actions = COALESCE(?8, actions),
           updated_at = ?9
         WHERE id = ?1",
        params![
            id,
            patch.name.as_deref().map(str::trim),
            patch.enabled.map(i64::from),
            i64::from(patch.board_id.is_some()),
            patch.board_id.clone().flatten(),
            patch.trigger.as_ref().map(serde_json::to_string).transpose()?,
            patch.conditions.as_ref().map(serde_json::to_string).transpose()?,
            patch.actions.as_ref().map(serde_json::to_string).transpose()?,
            touch()
        ],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM automations WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn record_run(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE automations SET last_run_at = ?2, run_count = run_count + 1 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

/// Claims a one-shot key, returning true only the first time it is seen.
///
/// Scheduled rules and overdue detection use this so a restart, a clock jump or
/// an overlapping tick cannot fire the same occurrence twice.
pub fn claim_once(conn: &Connection, key: &str) -> Result<bool> {
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO trigger_log (key, created_at) VALUES (?1, ?2)",
        params![key, now()],
    )?;
    Ok(inserted > 0)
}

/// Drops trigger-log entries older than `days`, keeping the table small.
pub fn prune_trigger_log(conn: &Connection, days: i64) -> Result<()> {
    let cutoff = crate::util::to_ts(chrono::Utc::now() - chrono::Duration::days(days));
    conn.execute("DELETE FROM trigger_log WHERE created_at < ?1", params![cutoff])?;
    Ok(())
}
