//! The IPC surface.
//!
//! Commands are thin: they validate and translate, then delegate to `store` for
//! data and `ops` for anything with side effects. Every mutating command
//! releases the database lock before telling the UI and the tray to refresh, so
//! a refresh can never deadlock against the write that caused it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::error::{Error, Result, rejected};
use crate::models::{
    Attachment, ActivityEntry, Automation, Board, BoardView, ChecklistItem, GlobalCounts, Label,
    List, Recurrence, Reminder, SearchHit, Task, TaskQuery,
};
use crate::nlp;
use crate::ops;
use crate::portability::{self, ImportMode, ImportSummary};
use crate::state::AppState;
use crate::store::{
    attachments, automations as auto_store, boards, labels, lists, query, reminders, settings, tasks,
};

/// Distinguishes an absent field from an explicit `null`, so the UI can clear a
/// due date without a separate command.
fn double_option<'de, T, D>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

// ------------------------------------------------------------------ startup

/// Everything the UI needs to render its first frame, in one round trip.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub boards: Vec<Board>,
    pub main_board_id: String,
    pub labels: Vec<Label>,
    pub settings: BTreeMap<String, Value>,
    pub counts: GlobalCounts,
    /// Reminders that fired while the window was closed.
    pub alerts: Vec<Task>,
    pub platform: String,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap> {
    let conn = state.db();
    Ok(Bootstrap {
        boards: boards::list(&conn, false)?,
        main_board_id: boards::main_board(&conn)?.id,
        labels: labels::all(&conn)?,
        settings: settings::all(&conn)?,
        counts: query::counts(&conn)?,
        alerts: crate::engine::outstanding_alerts(&conn)?,
        platform: std::env::consts::OS.to_string(),
    })
}

// ------------------------------------------------------------------- boards

#[tauri::command]
pub fn list_boards(state: State<'_, AppState>, include_archived: bool) -> Result<Vec<Board>> {
    let conn = state.db();
    boards::list(&conn, include_archived)
}

#[tauri::command]
pub fn board_view(state: State<'_, AppState>, board_id: String) -> Result<BoardView> {
    let conn = state.db();
    Ok(BoardView {
        board: boards::get(&conn, &board_id)?,
        lists: lists::for_board(&conn, &board_id)?,
        tasks: tasks::for_board(&conn, &board_id, false)?,
        labels: labels::for_board(&conn, &board_id)?,
    })
}

#[tauri::command]
pub fn create_board(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
    icon: Option<String>,
) -> Result<Board> {
    let board = state.transaction(|conn| boards::create(conn, &name, color.as_deref(), icon.as_deref()))?;
    ops::notify_data_changed(&app);
    Ok(board)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub color: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub icon: Option<Option<String>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

#[tauri::command]
pub fn update_board(
    app: AppHandle,
    state: State<'_, AppState>,
    board_id: String,
    patch: BoardUpdate,
) -> Result<Board> {
    let board = state.transaction(|conn| {
        boards::update(
            conn,
            &board_id,
            boards::BoardPatch {
                name: patch.name,
                color: patch.color,
                icon: patch.icon,
                archived: patch.archived,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(board)
}

#[tauri::command]
pub fn delete_board(app: AppHandle, state: State<'_, AppState>, board_id: String) -> Result<()> {
    state.transaction(|conn| boards::delete(conn, &board_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn reorder_board(
    app: AppHandle,
    state: State<'_, AppState>,
    board_id: String,
    index: usize,
) -> Result<Vec<Board>> {
    let boards = state.transaction(|conn| boards::reorder(conn, &board_id, index))?;
    ops::notify_data_changed(&app);
    Ok(boards)
}

// -------------------------------------------------------------------- lists

#[tauri::command]
pub fn create_list(
    app: AppHandle,
    state: State<'_, AppState>,
    board_id: String,
    name: String,
    is_done_list: Option<bool>,
) -> Result<List> {
    let list = state
        .transaction(|conn| lists::create(conn, &board_id, &name, is_done_list.unwrap_or(false)))?;
    ops::notify_data_changed(&app);
    Ok(list)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_done_list: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub wip_limit: Option<Option<i64>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

#[tauri::command]
pub fn update_list(
    app: AppHandle,
    state: State<'_, AppState>,
    list_id: String,
    patch: ListUpdate,
) -> Result<List> {
    let list = state.transaction(|conn| {
        lists::update(
            conn,
            &list_id,
            lists::ListPatch {
                name: patch.name,
                is_done_list: patch.is_done_list,
                wip_limit: patch.wip_limit,
                archived: patch.archived,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(list)
}

#[tauri::command]
pub fn delete_list(app: AppHandle, state: State<'_, AppState>, list_id: String) -> Result<()> {
    state.transaction(|conn| lists::delete(conn, &list_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn reorder_list(
    app: AppHandle,
    state: State<'_, AppState>,
    list_id: String,
    index: usize,
) -> Result<Vec<List>> {
    let lists = state.transaction(|conn| lists::reorder(conn, &list_id, index))?;
    ops::notify_data_changed(&app);
    Ok(lists)
}

// -------------------------------------------------------------------- tasks

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub list_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub due_has_time: bool,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub index: Option<usize>,
}

#[tauri::command]
pub fn create_task(app: AppHandle, state: State<'_, AppState>, input: TaskInput) -> Result<Task> {
    let task = state.transaction(|conn| {
        ops::create_task(
            conn,
            &app,
            tasks::NewTask {
                board_id: input.board_id,
                list_id: input.list_id,
                title: input.title,
                description: input.description,
                notes: input.notes,
                priority: input.priority,
                due_at: input.due_at,
                due_has_time: input.due_has_time,
                recurrence: input.recurrence,
                label_ids: input.label_ids,
                index: input.index,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(task)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_at: Option<Option<String>>,
    #[serde(default)]
    pub due_has_time: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub recurrence: Option<Option<Recurrence>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

#[tauri::command]
pub fn update_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    patch: TaskUpdate,
) -> Result<Task> {
    let task = state.transaction(|conn| {
        tasks::update(
            conn,
            &task_id,
            tasks::TaskPatch {
                title: patch.title,
                description: patch.description,
                notes: patch.notes,
                priority: patch.priority,
                due_at: patch.due_at,
                due_has_time: patch.due_has_time,
                recurrence: patch.recurrence,
                archived: patch.archived,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(task)
}

#[tauri::command]
pub fn move_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    list_id: String,
    index: Option<usize>,
) -> Result<Task> {
    let task = state.transaction(|conn| ops::move_task(conn, &app, &task_id, &list_id, index))?;
    ops::notify_data_changed(&app);
    Ok(task)
}

#[tauri::command]
pub fn set_task_completed(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    completed: bool,
) -> Result<Task> {
    let task = state.transaction(|conn| {
        if completed {
            ops::complete_task(conn, &app, &task_id)
        } else {
            ops::uncomplete_task(conn, &task_id)
        }
    })?;
    ops::notify_data_changed(&app);
    Ok(task)
}

#[tauri::command]
pub fn delete_task(app: AppHandle, state: State<'_, AppState>, task_id: String) -> Result<()> {
    state.transaction(|conn| tasks::delete(conn, &task_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn duplicate_task(app: AppHandle, state: State<'_, AppState>, task_id: String) -> Result<Task> {
    let task = state.transaction(|conn| tasks::duplicate(conn, &task_id))?;
    ops::notify_data_changed(&app);
    Ok(task)
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, task_id: String) -> Result<Task> {
    let conn = state.db();
    tasks::get_hydrated(&conn, &task_id)
}

#[tauri::command]
pub fn task_activity(state: State<'_, AppState>, task_id: String) -> Result<Vec<ActivityEntry>> {
    let conn = state.db();
    let mut stmt = conn.prepare(
        "SELECT id, task_id, board_id, kind, message, created_at FROM activity
         WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([&task_id], |row| {
        Ok(ActivityEntry {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            board_id: row.get("board_id")?,
            kind: row.get("kind")?,
            message: row.get("message")?,
            created_at: row.get("created_at")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// --------------------------------------------------------------- checklists

#[tauri::command]
pub fn add_checklist_item(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    text: String,
) -> Result<ChecklistItem> {
    let item = state.transaction(|conn| tasks::add_checklist_item(conn, &task_id, &text))?;
    ops::notify_data_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn update_checklist_item(
    app: AppHandle,
    state: State<'_, AppState>,
    item_id: String,
    text: Option<String>,
    done: Option<bool>,
) -> Result<()> {
    state.transaction(|conn| tasks::update_checklist_item(conn, &item_id, text.as_deref(), done))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_checklist_item(
    app: AppHandle,
    state: State<'_, AppState>,
    item_id: String,
) -> Result<()> {
    state.transaction(|conn| tasks::delete_checklist_item(conn, &item_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

// ------------------------------------------------------------------- labels

#[tauri::command]
pub fn list_labels(state: State<'_, AppState>) -> Result<Vec<Label>> {
    let conn = state.db();
    labels::all(&conn)
}

#[tauri::command]
pub fn create_label(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    color: String,
    board_id: Option<String>,
) -> Result<Label> {
    let label =
        state.transaction(|conn| labels::create(conn, &name, &color, board_id.as_deref()))?;
    ops::notify_data_changed(&app);
    Ok(label)
}

#[tauri::command]
pub fn update_label(
    app: AppHandle,
    state: State<'_, AppState>,
    label_id: String,
    name: Option<String>,
    color: Option<String>,
) -> Result<Label> {
    let label = state
        .transaction(|conn| labels::update(conn, &label_id, name.as_deref(), color.as_deref()))?;
    ops::notify_data_changed(&app);
    Ok(label)
}

#[tauri::command]
pub fn delete_label(app: AppHandle, state: State<'_, AppState>, label_id: String) -> Result<()> {
    state.transaction(|conn| labels::delete(conn, &label_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn set_task_label(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    label_id: String,
    attached: bool,
) -> Result<Task> {
    let task = state.transaction(|conn| {
        if attached {
            ops::add_label(conn, &app, &task_id, &label_id)
        } else {
            ops::remove_label(conn, &app, &task_id, &label_id)
        }
    })?;
    ops::notify_data_changed(&app);
    Ok(task)
}

// ---------------------------------------------------------------- reminders

#[tauri::command]
pub fn add_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    offset_minutes: Option<i64>,
    fire_at: Option<String>,
    recurrence: Option<Recurrence>,
) -> Result<Reminder> {
    let reminder = state.transaction(|conn| match (offset_minutes, fire_at.as_deref()) {
        // A relative reminder needs something to be relative to.
        (Some(offset), None) => {
            let task = tasks::get(conn, &task_id)?;
            if task.due_at.is_none() {
                return Err(rejected("Set a due date before adding a relative reminder"));
            }
            reminders::create_relative(conn, &task_id, offset)
        }
        (None, Some(at)) => reminders::create_absolute(conn, &task_id, at, recurrence.clone()),
        _ => Err(rejected("A reminder needs either an offset or a time")),
    })?;
    ops::notify_data_changed(&app);
    Ok(reminder)
}

#[tauri::command]
pub fn delete_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    reminder_id: String,
) -> Result<()> {
    state.transaction(|conn| reminders::delete(conn, &reminder_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn snooze_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    reminder_id: String,
    minutes: Option<i64>,
) -> Result<Reminder> {
    let reminder = state.transaction(|conn| {
        let minutes = minutes.unwrap_or_else(|| settings::get_i64(conn, "snoozeMinutes", 10));
        reminders::snooze(conn, &reminder_id, minutes)
    })?;
    ops::notify_data_changed(&app);
    Ok(reminder)
}

#[tauri::command]
pub fn dismiss_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    reminder_id: String,
) -> Result<()> {
    state.transaction(|conn| reminders::dismiss(conn, &reminder_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

// -------------------------------------------------------- search and views

#[tauri::command]
pub fn query_tasks(state: State<'_, AppState>, query: TaskQuery) -> Result<Vec<Task>> {
    let conn = state.db();
    query::tasks(&conn, &query)
}

#[tauri::command]
pub fn search_tasks(state: State<'_, AppState>, query: TaskQuery) -> Result<Vec<SearchHit>> {
    let conn = state.db();
    query::search(&conn, &query)
}

#[tauri::command]
pub fn global_counts(state: State<'_, AppState>) -> Result<GlobalCounts> {
    let conn = state.db();
    query::counts(&conn)
}

// -------------------------------------------------------------- automations

#[tauri::command]
pub fn list_automations(state: State<'_, AppState>) -> Result<Vec<Automation>> {
    let conn = state.db();
    auto_store::all(&conn)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInput {
    pub name: String,
    #[serde(default)]
    pub board_id: Option<String>,
    pub trigger: crate::models::Trigger,
    #[serde(default)]
    pub conditions: Vec<crate::models::Condition>,
    pub actions: Vec<crate::models::AutomationAction>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[tauri::command]
pub fn create_automation(
    app: AppHandle,
    state: State<'_, AppState>,
    input: AutomationInput,
) -> Result<Automation> {
    let automation = state.transaction(|conn| {
        auto_store::create(
            conn,
            auto_store::NewAutomation {
                name: input.name,
                board_id: input.board_id,
                trigger: input.trigger,
                conditions: input.conditions,
                actions: input.actions,
                enabled: input.enabled,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(automation)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpdate {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub board_id: Option<Option<String>>,
    #[serde(default)]
    pub trigger: Option<crate::models::Trigger>,
    #[serde(default)]
    pub conditions: Option<Vec<crate::models::Condition>>,
    #[serde(default)]
    pub actions: Option<Vec<crate::models::AutomationAction>>,
}

#[tauri::command]
pub fn update_automation(
    app: AppHandle,
    state: State<'_, AppState>,
    automation_id: String,
    patch: AutomationUpdate,
) -> Result<Automation> {
    let automation = state.transaction(|conn| {
        auto_store::update(
            conn,
            &automation_id,
            auto_store::AutomationPatch {
                name: patch.name,
                enabled: patch.enabled,
                board_id: patch.board_id,
                trigger: patch.trigger,
                conditions: patch.conditions,
                actions: patch.actions,
            },
        )
    })?;
    ops::notify_data_changed(&app);
    Ok(automation)
}

#[tauri::command]
pub fn delete_automation(
    app: AppHandle,
    state: State<'_, AppState>,
    automation_id: String,
) -> Result<()> {
    state.transaction(|conn| auto_store::delete(conn, &automation_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

// ----------------------------------------------------------------- settings

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<BTreeMap<String, Value>> {
    let conn = state.db();
    settings::all(&conn)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    values: BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    // Rebinding the shortcut can fail; do it first so a bad accelerator is
    // reported without the rest of the settings silently not applying.
    if let Some(shortcut) = values.get("quickAddShortcut").and_then(Value::as_str) {
        crate::quickadd::register_shortcut(&app, shortcut)?;
    }
    let saved = state.transaction(|conn| {
        settings::set_many(conn, &values)?;
        settings::all(conn)
    })?;
    state.refresh_cached_settings();
    ops::notify_data_changed(&app);
    Ok(saved)
}

// -------------------------------------------------------------- quick add

#[tauri::command]
pub fn preview_quick_add(text: String) -> nlp::Parsed {
    nlp::parse(&text, chrono::Local::now())
}

/// Creates a task from a line of natural language.
///
/// Falls back to the Main Board, matching the promise that quick capture never
/// requires choosing a destination first.
#[tauri::command]
pub fn quick_add(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    board_id: Option<String>,
    list_id: Option<String>,
) -> Result<Task> {
    let parsed = nlp::parse(&text, chrono::Local::now());
    if parsed.title.trim().is_empty() {
        return Err(rejected("Type something to add a task"));
    }

    let task = state.transaction(|conn| {
        // An explicit board wins; then `@board` from the text; then Main Board.
        let board = match &board_id {
            Some(id) => boards::get(conn, id)?,
            None => match parsed.board.as_deref().and_then(|name| find_board(conn, name)) {
                Some(board) => board,
                None => boards::main_board(conn)?,
            },
        };

        let label_ids = parsed
            .labels
            .iter()
            .filter_map(|name| labels::find_by_name(conn, name, &board.id).ok().flatten())
            .map(|label| label.id)
            .collect();

        ops::create_task(
            conn,
            &app,
            tasks::NewTask {
                board_id: Some(board.id.clone()),
                list_id: list_id.clone(),
                title: parsed.title.clone(),
                priority: parsed.priority,
                due_at: parsed.due_at.clone(),
                due_has_time: parsed.due_has_time,
                recurrence: parsed.recurrence.clone(),
                label_ids,
                ..Default::default()
            },
        )
    })?;

    ops::notify_data_changed(&app);
    Ok(task)
}

fn find_board(conn: &rusqlite::Connection, name: &str) -> Option<Board> {
    boards::list(conn, false)
        .ok()?
        .into_iter()
        .find(|b| b.name.eq_ignore_ascii_case(name.trim()))
}

#[tauri::command]
pub fn hide_quick_add(app: AppHandle) {
    crate::quickadd::hide(&app);
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    ops::show_main_window(&app);
}

// -------------------------------------------------------------- attachments

#[tauri::command]
pub fn add_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    path: String,
) -> Result<Attachment> {
    let source = PathBuf::from(&path);
    let attachment = state.transaction(|conn| {
        attachments::attach_file(conn, &state.data_dir, &task_id, &source)
    })?;
    ops::notify_data_changed(&app);
    Ok(attachment)
}

#[tauri::command]
pub fn delete_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    attachment_id: String,
) -> Result<()> {
    state.transaction(|conn| attachments::delete(conn, &attachment_id))?;
    ops::notify_data_changed(&app);
    Ok(())
}

/// Opens an attachment with the OS default handler.
///
/// The path comes from our own table and is re-checked against the managed
/// store, so a tampered database cannot turn this into "open any file".
#[tauri::command]
pub fn open_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    attachment_id: String,
) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;

    let attachment = {
        let conn = state.db();
        attachments::get(&conn, &attachment_id)?
    };
    let root = crate::db::attachments_dir(&state.data_dir);
    let path = PathBuf::from(&attachment.path);
    if !path.starts_with(&root) {
        return Err(rejected("That attachment is outside the attachment store"));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::Rejected(format!("Could not open the attachment: {e}")))
}

// ----------------------------------------------------------- import/export

#[tauri::command]
pub fn export_data(state: State<'_, AppState>, path: String) -> Result<String> {
    let path = PathBuf::from(path);
    let conn = state.db();
    portability::export_to_file(&conn, &path)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn import_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    mode: ImportMode,
) -> Result<ImportSummary> {
    let path = PathBuf::from(path);
    let summary = state.transaction(|conn| {
        portability::import_from_file(conn, &state.data_dir, &path, mode)
    })?;
    state.refresh_cached_settings();
    ops::notify_data_changed(&app);
    Ok(summary)
}

// -------------------------------------------------------------- maintenance

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: Option<String>,
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupInfo>> {
    let mut out = Vec::new();
    for path in crate::db::list_backups(&state.data_dir)?.into_iter().rev() {
        let meta = std::fs::metadata(&path).ok();
        out.push(BackupInfo {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta
                .and_then(|m| m.modified().ok())
                .map(|t| crate::util::to_ts(chrono::DateTime::<chrono::Utc>::from(t))),
            path: path.to_string_lossy().to_string(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn create_backup_now(state: State<'_, AppState>) -> Result<String> {
    let conn = state.db();
    let path = crate::db::create_backup(&conn, &state.data_dir)?;
    settings::set(&conn, "lastBackupAt", &serde_json::json!(crate::util::now()))?;
    Ok(path.to_string_lossy().to_string())
}

/// The folder holding the database, backups and attachments, for the
/// "Show in file manager" affordance in settings.
#[tauri::command]
pub fn data_directory(state: State<'_, AppState>) -> String {
    state.data_dir.to_string_lossy().to_string()
}
