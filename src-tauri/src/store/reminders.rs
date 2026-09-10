//! Reminders. A reminder is either pinned to an absolute instant or anchored to
//! its task's due date, in which case `fire_at` is recomputed whenever the due
//! date moves.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::{Id, Reminder, ReminderKind, Recurrence};
use crate::store::{json_value, json_column};
use crate::util::{new_id, now, parse_ts, to_ts};

pub fn map(row: &Row<'_>) -> rusqlite::Result<Reminder> {
    let kind = match row.get::<_, String>("kind")?.as_str() {
        "relativeToDue" => ReminderKind::RelativeToDue,
        _ => ReminderKind::Absolute,
    };
    Ok(Reminder {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        kind,
        offset_minutes: row.get("offset_minutes")?,
        fire_at: row.get("fire_at")?,
        recurrence: json_column(row, "recurrence")?,
        snoozed_until: row.get("snoozed_until")?,
        fired_at: row.get("fired_at")?,
        dismissed: row.get::<_, i64>("dismissed")? != 0,
        created_at: row.get("created_at")?,
    })
}

fn kind_str(kind: ReminderKind) -> &'static str {
    match kind {
        ReminderKind::Absolute => "absolute",
        ReminderKind::RelativeToDue => "relativeToDue",
    }
}

const SELECT: &str = "SELECT id, task_id, kind, offset_minutes, fire_at, recurrence, snoozed_until, fired_at, dismissed, created_at FROM reminders";

pub fn for_task(conn: &Connection, task_id: &str) -> Result<Vec<Reminder>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE task_id = ?1 ORDER BY fire_at IS NULL, fire_at ASC"))?;
    Ok(stmt
        .query_map(params![task_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Reminder> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("reminder"))
}

/// The instant a reminder should actually fire: a snooze overrides `fire_at`.
pub fn effective_time(reminder: &Reminder) -> Option<DateTime<Utc>> {
    reminder
        .snoozed_until
        .as_deref()
        .and_then(parse_ts)
        .or_else(|| reminder.fire_at.as_deref().and_then(parse_ts))
}

/// Adds a reminder anchored to the task's due date, e.g. "10 minutes before".
pub fn create_relative(conn: &Connection, task_id: &str, offset_minutes: i64) -> Result<Reminder> {
    if !(0..=60 * 24 * 365).contains(&offset_minutes) {
        return Err(rejected("Reminder offset must be between 0 minutes and a year"));
    }
    let id = insert(conn, task_id, ReminderKind::RelativeToDue, Some(offset_minutes), None, None)?;
    recompute_for_task(conn, task_id)?;
    get(conn, &id)
}

/// Adds a reminder that fires at a fixed instant, independent of the due date.
pub fn create_absolute(
    conn: &Connection,
    task_id: &str,
    fire_at: &str,
    recurrence: Option<Recurrence>,
) -> Result<Reminder> {
    if parse_ts(fire_at).is_none() {
        return Err(rejected("Reminder time is not a valid date"));
    }
    let id = insert(conn, task_id, ReminderKind::Absolute, None, Some(fire_at), recurrence)?;
    get(conn, &id)
}

fn insert(
    conn: &Connection,
    task_id: &str,
    kind: ReminderKind,
    offset_minutes: Option<i64>,
    fire_at: Option<&str>,
    recurrence: Option<Recurrence>,
) -> Result<Id> {
    let id = new_id();
    conn.execute(
        "INSERT INTO reminders (id, task_id, kind, offset_minutes, fire_at, recurrence, snoozed_until, fired_at, dismissed, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, 0, ?7)",
        params![id, task_id, kind_str(kind), offset_minutes, fire_at, json_value(&recurrence), now()],
    )?;
    Ok(id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM reminders WHERE id = ?1", params![id])?;
    Ok(())
}

/// Re-anchors relative reminders after the task's due date changed.
///
/// A reminder that lands back in the future is re-armed, so moving a due date
/// forward makes an already-fired reminder fire again for the new time.
pub fn recompute_for_task(conn: &Connection, task_id: &str) -> Result<()> {
    let due: Option<String> = conn
        .query_row("SELECT due_at FROM tasks WHERE id = ?1", params![task_id], |r| r.get(0))
        .optional()?
        .flatten();
    let due = due.as_deref().and_then(parse_ts);

    for reminder in for_task(conn, task_id)? {
        if reminder.kind != ReminderKind::RelativeToDue {
            continue;
        }
        let fire_at = due.map(|d| to_ts(d - Duration::minutes(reminder.offset_minutes.unwrap_or(0))));
        let still_pending = fire_at
            .as_deref()
            .and_then(parse_ts)
            .is_some_and(|t| t > Utc::now());
        conn.execute(
            "UPDATE reminders SET fire_at = ?2,
               fired_at = CASE WHEN ?3 = 1 THEN NULL ELSE fired_at END,
               dismissed = CASE WHEN ?3 = 1 THEN 0 ELSE dismissed END,
               snoozed_until = CASE WHEN ?3 = 1 THEN NULL ELSE snoozed_until END
             WHERE id = ?1",
            params![reminder.id, fire_at, i64::from(still_pending)],
        )?;
    }
    Ok(())
}

/// Reminders that are due to fire now, on tasks that are still open.
///
/// Rows are returned alongside their task title and board so the engine can
/// build a notification without a second query per reminder.
pub struct DueReminder {
    pub reminder: Reminder,
    pub task_id: Id,
    pub task_title: String,
    pub board_name: String,
}

pub fn due_now(conn: &Connection, at: DateTime<Utc>) -> Result<Vec<DueReminder>> {
    let cutoff = to_ts(at);
    let mut stmt = conn.prepare(
        "SELECT r.id, r.task_id, r.kind, r.offset_minutes, r.fire_at, r.recurrence,
                r.snoozed_until, r.fired_at, r.dismissed, r.created_at,
                t.title AS task_title, t.board_id AS board_id, b.name AS board_name
         FROM reminders r
         JOIN tasks t ON t.id = r.task_id
         JOIN boards b ON b.id = t.board_id
         WHERE r.dismissed = 0
           AND t.completed_at IS NULL
           AND t.archived = 0
           AND COALESCE(r.snoozed_until, r.fire_at) IS NOT NULL
           AND COALESCE(r.snoozed_until, r.fire_at) <= ?1
           AND (r.fired_at IS NULL OR (r.snoozed_until IS NOT NULL AND r.snoozed_until > r.fired_at))
         ORDER BY COALESCE(r.snoozed_until, r.fire_at) ASC",
    )?;
    let rows = stmt.query_map(params![cutoff], |row| {
        Ok(DueReminder {
            reminder: map(row)?,
            task_id: row.get("task_id")?,
            task_title: row.get("task_title")?,
            board_name: row.get("board_name")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Marks a reminder as delivered. Repeating reminders roll forward instead of
/// being retired, so they keep firing on their own schedule.
pub fn mark_fired(conn: &Connection, reminder: &Reminder) -> Result<()> {
    let stamp = now();
    let next = reminder
        .recurrence
        .as_ref()
        .zip(effective_time(reminder))
        .and_then(|(rec, from)| crate::recurrence::next_after(rec, from));

    match next {
        Some(next_at) => {
            conn.execute(
                "UPDATE reminders SET fired_at = ?2, snoozed_until = NULL, fire_at = ?3 WHERE id = ?1",
                params![reminder.id, stamp, to_ts(next_at)],
            )?;
        }
        None => {
            conn.execute(
                "UPDATE reminders SET fired_at = ?2, snoozed_until = NULL WHERE id = ?1",
                params![reminder.id, stamp],
            )?;
        }
    }
    Ok(())
}

/// Pushes a reminder out by `minutes` and re-arms it.
pub fn snooze(conn: &Connection, id: &str, minutes: i64) -> Result<Reminder> {
    if !(1..=60 * 24 * 30).contains(&minutes) {
        return Err(rejected("Snooze must be between a minute and 30 days"));
    }
    let until = to_ts(Utc::now() + Duration::minutes(minutes));
    conn.execute(
        "UPDATE reminders SET snoozed_until = ?2, dismissed = 0 WHERE id = ?1",
        params![id, until],
    )?;
    get(conn, id)
}

/// Silences a reminder without touching the task.
pub fn dismiss(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET dismissed = 1, snoozed_until = NULL WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Silences every reminder on a task — used when the task is completed.
pub fn dismiss_for_task(conn: &Connection, task_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET dismissed = 1, snoozed_until = NULL WHERE task_id = ?1",
        params![task_id],
    )?;
    Ok(())
}
