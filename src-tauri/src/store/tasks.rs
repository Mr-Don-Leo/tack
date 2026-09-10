//! Tasks and their checklist items.
//!
//! These functions only touch the database. Side effects that belong to the
//! product rather than the data — firing automations, spawning the next
//! occurrence of a repeating task — live in `crate::ops`.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::{ChecklistItem, Id, Recurrence, Task};
use crate::store::{json_column, json_value, touch};
use crate::util::{POSITION_STEP, needs_rebalance, new_id, now, parse_ts, position_between};

/// Longest accepted title. Guards the database and the notification body
/// against a pasted document.
const MAX_TITLE: usize = 500;
const MAX_TEXT: usize = 100_000;

pub fn map(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get("id")?,
        board_id: row.get("board_id")?,
        list_id: row.get("list_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        notes: row.get("notes")?,
        priority: row.get("priority")?,
        due_at: row.get("due_at")?,
        due_has_time: row.get::<_, i64>("due_has_time")? != 0,
        completed_at: row.get("completed_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
        position: row.get("position")?,
        recurrence: json_column(row, "recurrence")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        labels: Vec::new(),
        checklist: Vec::new(),
        attachments: Vec::new(),
        reminders: Vec::new(),
    })
}

pub const SELECT: &str = "SELECT id, board_id, list_id, title, description, notes, priority, due_at,
        due_has_time, completed_at, archived, position, recurrence, created_at, updated_at FROM tasks";

/// Fills in the relations that the board and detail views need.
pub fn hydrate(conn: &Connection, mut task: Task) -> Result<Task> {
    task.labels = super::labels::for_task(conn, &task.id)?;
    task.checklist = checklist(conn, &task.id)?;
    task.attachments = super::attachments::for_task(conn, &task.id)?;
    task.reminders = super::reminders::for_task(conn, &task.id)?;
    Ok(task)
}

pub fn hydrate_all(conn: &Connection, tasks: Vec<Task>) -> Result<Vec<Task>> {
    tasks.into_iter().map(|t| hydrate(conn, t)).collect()
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("task"))
}

pub fn get_hydrated(conn: &Connection, id: &str) -> Result<Task> {
    let task = get(conn, id)?;
    hydrate(conn, task)
}

pub fn for_board(conn: &Connection, board_id: &str, include_archived: bool) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE board_id = ?1 {} ORDER BY position ASC",
        if include_archived { "" } else { "AND archived = 0" }
    ))?;
    let tasks = stmt
        .query_map(params![board_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    hydrate_all(conn, tasks)
}

pub fn count_in_list(conn: &Connection, list_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE list_id = ?1 AND archived = 0",
        params![list_id],
        |r| r.get(0),
    )?)
}

/// Enforces an optional work-in-progress cap on a column.
fn check_wip_limit(conn: &Connection, list: &crate::models::List) -> Result<()> {
    // Capping the done column would make completing a task fail, which is
    // never what a work-in-progress limit is for.
    if list.is_done_list {
        return Ok(());
    }
    let Some(limit) = list.wip_limit.filter(|l| *l > 0) else {
        return Ok(());
    };
    if count_in_list(conn, &list.id)? >= limit {
        return Err(rejected(format!(
            "“{}” is limited to {limit} card{}",
            list.name,
            if limit == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

/// Fields accepted when creating a task. Everything except the title is
/// optional so quick-add stays a single keystroke away.
#[derive(Default)]
pub struct NewTask {
    pub board_id: Option<Id>,
    pub list_id: Option<Id>,
    pub title: String,
    pub description: String,
    pub notes: String,
    pub priority: i64,
    pub due_at: Option<String>,
    pub due_has_time: bool,
    pub recurrence: Option<Recurrence>,
    pub label_ids: Vec<Id>,
    /// Insert position within the column; `None` appends.
    pub index: Option<usize>,
}

pub fn create(conn: &Connection, input: NewTask) -> Result<Task> {
    let title = sanitize(&input.title, MAX_TITLE);
    if title.is_empty() {
        return Err(rejected("A task needs a title"));
    }
    validate_priority(input.priority)?;
    validate_due(&input.due_at)?;

    // Resolve the destination, falling back to the Main Board's first column so
    // quick-add always has somewhere to land.
    let board_id = match &input.board_id {
        Some(id) => super::boards::get(conn, id)?.id,
        None => super::boards::main_board(conn)?.id,
    };
    let list = match &input.list_id {
        Some(id) => {
            let list = super::lists::get(conn, id)?;
            if list.board_id != board_id {
                return Err(rejected("That column belongs to a different board"));
            }
            list
        }
        None => super::lists::first_list(conn, &board_id)?,
    };
    check_wip_limit(conn, &list)?;

    let position = insert_position(conn, &list.id, input.index)?;
    let id = new_id();
    let ts = now();

    conn.execute(
        "INSERT INTO tasks (id, board_id, list_id, title, description, notes, priority, due_at,
                            due_has_time, completed_at, archived, position, recurrence, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0, ?10, ?11, ?12, ?12)",
        params![
            id,
            board_id,
            list.id,
            title,
            sanitize(&input.description, MAX_TEXT),
            sanitize(&input.notes, MAX_TEXT),
            input.priority,
            input.due_at,
            i64::from(input.due_has_time),
            position,
            json_value(&input.recurrence),
            ts
        ],
    )?;

    for label_id in &input.label_ids {
        super::labels::attach(conn, &id, label_id)?;
    }

    super::log_activity(conn, Some(&id), Some(&board_id), "task.created", "Created");
    get_hydrated(conn, &id)
}

/// A partial update. `Option<Option<T>>` distinguishes "leave alone" (`None`)
/// from "clear this field" (`Some(None)`).
#[derive(Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<i64>,
    pub due_at: Option<Option<String>>,
    pub due_has_time: Option<bool>,
    pub recurrence: Option<Option<Recurrence>>,
    pub archived: Option<bool>,
}

pub fn update(conn: &Connection, id: &str, patch: TaskPatch) -> Result<Task> {
    let existing = get(conn, id)?;

    if let Some(title) = &patch.title
        && sanitize(title, MAX_TITLE).is_empty()
    {
        return Err(rejected("A task needs a title"));
    }
    if let Some(priority) = patch.priority {
        validate_priority(priority)?;
    }
    if let Some(due) = &patch.due_at {
        validate_due(due)?;
    }

    conn.execute(
        "UPDATE tasks SET
           title = COALESCE(?2, title),
           description = COALESCE(?3, description),
           notes = COALESCE(?4, notes),
           priority = COALESCE(?5, priority),
           due_at = CASE WHEN ?6 = 1 THEN ?7 ELSE due_at END,
           due_has_time = COALESCE(?8, due_has_time),
           recurrence = CASE WHEN ?9 = 1 THEN ?10 ELSE recurrence END,
           archived = COALESCE(?11, archived),
           updated_at = ?12
         WHERE id = ?1",
        params![
            id,
            patch.title.as_deref().map(|t| sanitize(t, MAX_TITLE)),
            patch.description.as_deref().map(|t| sanitize(t, MAX_TEXT)),
            patch.notes.as_deref().map(|t| sanitize(t, MAX_TEXT)),
            patch.priority,
            i64::from(patch.due_at.is_some()),
            patch.due_at.clone().flatten(),
            patch.due_has_time.map(i64::from),
            i64::from(patch.recurrence.is_some()),
            json_value(&patch.recurrence.clone().flatten()),
            patch.archived.map(i64::from),
            touch()
        ],
    )?;

    // A moved due date drags its relative reminders along with it.
    if patch.due_at.is_some() && patch.due_at.clone().flatten() != existing.due_at {
        super::reminders::recompute_for_task(conn, id)?;
    }
    get_hydrated(conn, id)
}

/// Moves a task to `list_id` at `index`, optionally across boards.
pub fn move_task(conn: &Connection, id: &str, list_id: &str, index: Option<usize>) -> Result<Task> {
    let task = get(conn, id)?;
    let list = super::lists::get(conn, list_id)?;
    if task.list_id != list_id {
        check_wip_limit(conn, &list)?;
    }
    let position = insert_position_excluding(conn, list_id, index, id)?;

    conn.execute(
        "UPDATE tasks SET list_id = ?2, board_id = ?3, position = ?4, updated_at = ?5 WHERE id = ?1",
        params![id, list_id, list.board_id, position, touch()],
    )?;

    if task.list_id != list_id {
        super::log_activity(
            conn,
            Some(id),
            Some(&list.board_id),
            "task.moved",
            &format!("Moved to “{}”", list.name),
        );
    }
    get_hydrated(conn, id)
}

/// Marks a task complete. Reminders on it are silenced; if the board has a done
/// column the card slides there so the board reflects reality.
pub fn complete(conn: &Connection, id: &str) -> Result<Task> {
    let task = get(conn, id)?;
    if task.completed_at.is_some() {
        return get_hydrated(conn, id);
    }
    conn.execute(
        "UPDATE tasks SET completed_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    super::reminders::dismiss_for_task(conn, id)?;

    if let Some(done) = super::lists::done_list(conn, &task.board_id)?
        && done.id != task.list_id
    {
        move_task(conn, id, &done.id, Some(0))?;
    }
    super::log_activity(conn, Some(id), Some(&task.board_id), "task.completed", "Completed");
    get_hydrated(conn, id)
}

/// Reopens a task. If it is sitting in the done column it moves back to the
/// first column so it is not stranded among finished work.
pub fn uncomplete(conn: &Connection, id: &str) -> Result<Task> {
    let task = get(conn, id)?;
    conn.execute(
        "UPDATE tasks SET completed_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;

    if let Some(done) = super::lists::done_list(conn, &task.board_id)?
        && done.id == task.list_id
    {
        let first = super::lists::first_list(conn, &task.board_id)?;
        if first.id != task.list_id {
            move_task(conn, id, &first.id, None)?;
        }
    }
    get_hydrated(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    // Detach files before the row disappears, so nothing is orphaned on disk.
    super::attachments::delete_all_for_task(conn, id)?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Copies a task, its labels and its checklist into the same column.
pub fn duplicate(conn: &Connection, id: &str) -> Result<Task> {
    let source = get_hydrated(conn, id)?;
    let copy = create(
        conn,
        NewTask {
            board_id: Some(source.board_id.clone()),
            list_id: Some(source.list_id.clone()),
            title: format!("{} (copy)", source.title),
            description: source.description.clone(),
            notes: source.notes.clone(),
            priority: source.priority,
            due_at: source.due_at.clone(),
            due_has_time: source.due_has_time,
            recurrence: source.recurrence.clone(),
            label_ids: source.labels.iter().map(|l| l.id.clone()).collect(),
            index: None,
        },
    )?;
    for item in &source.checklist {
        add_checklist_item(conn, &copy.id, &item.text)?;
    }
    get_hydrated(conn, &copy.id)
}

// ---------------------------------------------------------------- checklists

pub fn checklist(conn: &Connection, task_id: &str) -> Result<Vec<ChecklistItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, text, done, position, created_at
         FROM checklist_items WHERE task_id = ?1 ORDER BY position ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(ChecklistItem {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            text: row.get("text")?,
            done: row.get::<_, i64>("done")? != 0,
            position: row.get("position")?,
            created_at: row.get("created_at")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_checklist_item(conn: &Connection, task_id: &str, text: &str) -> Result<ChecklistItem> {
    let text = sanitize(text, MAX_TITLE);
    if text.is_empty() {
        return Err(rejected("A checklist item needs some text"));
    }
    let max: Option<f64> = conn.query_row(
        "SELECT MAX(position) FROM checklist_items WHERE task_id = ?1",
        params![task_id],
        |r| r.get(0),
    )?;
    let id = new_id();
    conn.execute(
        "INSERT INTO checklist_items (id, task_id, text, done, position, created_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![id, task_id, text, max.unwrap_or(0.0) + POSITION_STEP, now()],
    )?;
    checklist(conn, task_id)?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| not_found("checklist item"))
}

pub fn update_checklist_item(
    conn: &Connection,
    id: &str,
    text: Option<&str>,
    done: Option<bool>,
) -> Result<()> {
    if let Some(text) = text
        && sanitize(text, MAX_TITLE).is_empty()
    {
        return Err(rejected("A checklist item needs some text"));
    }
    conn.execute(
        "UPDATE checklist_items SET text = COALESCE(?2, text), done = COALESCE(?3, done) WHERE id = ?1",
        params![id, text.map(|t| sanitize(t, MAX_TITLE)), done.map(i64::from)],
    )?;
    Ok(())
}

pub fn delete_checklist_item(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM checklist_items WHERE id = ?1", params![id])?;
    Ok(())
}

// ------------------------------------------------------------------ ordering

fn insert_position(conn: &Connection, list_id: &str, index: Option<usize>) -> Result<f64> {
    insert_position_excluding(conn, list_id, index, "")
}

/// Position for dropping a card at `index` in `list_id`, ignoring `exclude_id`
/// so a card dragged within its own column measures against its neighbours
/// rather than itself.
fn insert_position_excluding(
    conn: &Connection,
    list_id: &str,
    index: Option<usize>,
    exclude_id: &str,
) -> Result<f64> {
    let mut stmt = conn.prepare(
        "SELECT position FROM tasks WHERE list_id = ?1 AND archived = 0 AND id <> ?2 ORDER BY position ASC",
    )?;
    let positions: Vec<f64> = stmt
        .query_map(params![list_id, exclude_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let index = index.unwrap_or(positions.len()).min(positions.len());
    let before = index.checked_sub(1).and_then(|i| positions.get(i)).copied();
    let after = positions.get(index).copied();

    if needs_rebalance(before, after) {
        rebalance(conn, list_id)?;
        return Ok(((index as f64) + 0.5) * POSITION_STEP);
    }
    Ok(position_between(before, after))
}

/// Renumbers a column onto clean multiples of the step. Only runs when repeated
/// drops into the same gap have exhausted float precision.
fn rebalance(conn: &Connection, list_id: &str) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id FROM tasks WHERE list_id = ?1 AND archived = 0 ORDER BY position ASC",
    )?;
    let ids: Vec<String> = stmt
        .query_map(params![list_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE tasks SET position = ?2 WHERE id = ?1",
            params![id, (i as f64 + 1.0) * POSITION_STEP],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------- validation

/// Trims, strips control characters, and caps length. Text is rendered as DOM
/// text nodes rather than markup, so this is about sane data, not escaping.
fn sanitize(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let trimmed = cleaned.trim();
    match trimmed.char_indices().nth(max) {
        Some((cut, _)) => trimmed[..cut].to_string(),
        None => trimmed.to_string(),
    }
}

fn validate_priority(priority: i64) -> Result<()> {
    if (0..=4).contains(&priority) {
        Ok(())
    } else {
        Err(rejected("Priority must be between 0 and 4"))
    }
}

fn validate_due(due: &Option<String>) -> Result<()> {
    match due {
        Some(value) if parse_ts(value).is_none() => Err(rejected("Due date is not a valid date")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_trims_and_caps() {
        assert_eq!(sanitize("  hello  ", 500), "hello");
        assert_eq!(sanitize("abcdef", 3), "abc");
    }

    #[test]
    fn sanitize_drops_control_characters_but_keeps_newlines() {
        assert_eq!(sanitize("a\u{0}b\nc", 500), "ab\nc");
    }

    #[test]
    fn sanitize_caps_by_character_not_byte() {
        // Cutting by byte index here would split a multi-byte character.
        assert_eq!(sanitize("ααα", 2), "αα");
    }
}
