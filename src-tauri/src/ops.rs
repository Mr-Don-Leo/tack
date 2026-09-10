//! Product-level operations: the store write plus everything that has to happen
//! around it — recurrence, automations, and telling the UI to refresh.
//!
//! Commands and the background engine both go through here so a task completed
//! from the tray behaves exactly like one completed on the board.

use chrono::Utc;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

use crate::automations::{self, Event};
use crate::error::Result;
use crate::models::Task;
use crate::state::AppState;
use crate::store::{labels, lists, reminders, tasks};
use crate::util::{parse_ts, to_ts};

/// Tells every window that the database changed and views should reload.
pub const DATA_CHANGED_EVENT: &str = "tack://data-changed";
/// Asks the main window to open a specific task.
pub const OPEN_TASK_EVENT: &str = "tack://open-task";

pub fn notify_data_changed(app: &AppHandle) {
    let _ = app.emit(DATA_CHANGED_EVENT, ());
    crate::tray::refresh(app);
}

pub fn create_task(conn: &Connection, app: &AppHandle, input: tasks::NewTask) -> Result<Task> {
    let task = tasks::create(conn, input)?;
    automations::dispatch(conn, app, Event::TaskCreated { task_id: task.id.clone() });
    tasks::get_hydrated(conn, &task.id)
}

/// Completes a task, spawns the next occurrence if it repeats, and fires rules.
pub fn complete_task(conn: &Connection, app: &AppHandle, task_id: &str) -> Result<Task> {
    let task = tasks::complete(conn, task_id)?;
    spawn_next_occurrence(conn, task_id)?;
    automations::dispatch(conn, app, Event::TaskCompleted { task_id: task_id.to_string() });
    let _ = task;
    tasks::get_hydrated(conn, task_id)
}

pub fn uncomplete_task(conn: &Connection, task_id: &str) -> Result<Task> {
    tasks::uncomplete(conn, task_id)
}

/// Moves a task and fires move rules when the column actually changed.
pub fn move_task(
    conn: &Connection,
    app: &AppHandle,
    task_id: &str,
    list_id: &str,
    index: Option<usize>,
) -> Result<Task> {
    let before = tasks::get(conn, task_id)?;
    let task = tasks::move_task(conn, task_id, list_id, index)?;

    if before.list_id != list_id {
        automations::dispatch(
            conn,
            app,
            Event::TaskMoved {
                task_id: task_id.to_string(),
                from_list_id: before.list_id,
                to_list_id: list_id.to_string(),
            },
        );
    }
    let _ = task;
    tasks::get_hydrated(conn, task_id)
}

pub fn add_label(conn: &Connection, app: &AppHandle, task_id: &str, label_id: &str) -> Result<Task> {
    if labels::attach(conn, task_id, label_id)? {
        automations::dispatch(
            conn,
            app,
            Event::LabelAdded { task_id: task_id.to_string(), label_id: label_id.to_string() },
        );
    }
    tasks::get_hydrated(conn, task_id)
}

pub fn remove_label(conn: &Connection, app: &AppHandle, task_id: &str, label_id: &str) -> Result<Task> {
    if labels::detach(conn, task_id, label_id)? {
        automations::dispatch(
            conn,
            app,
            Event::LabelRemoved { task_id: task_id.to_string(), label_id: label_id.to_string() },
        );
    }
    tasks::get_hydrated(conn, task_id)
}

/// Creates the next instance of a repeating task once the current one is done.
///
/// A fresh task is created rather than the due date being pushed forward, so
/// completed history survives and the next occurrence starts with a clean
/// checklist. Returns `None` when the task does not repeat or the series ended.
pub fn spawn_next_occurrence(conn: &Connection, task_id: &str) -> Result<Option<Task>> {
    let task = tasks::get_hydrated(conn, task_id)?;
    let Some(recurrence) = task.recurrence.clone() else {
        return Ok(None);
    };

    // Anchor to the original due date so a late completion does not shift the
    // whole series; fall back to now for repeating tasks with no due date.
    let anchor = task.due_at.as_deref().and_then(parse_ts).unwrap_or_else(Utc::now);
    let Some(next_due) = crate::recurrence::next_after(&recurrence, anchor) else {
        return Ok(None);
    };

    let mut next_recurrence = recurrence;
    next_recurrence.occurrences += 1;

    // Repeating cards belong back at the start of the workflow, not in Done.
    let target_list = match lists::done_list(conn, &task.board_id)? {
        Some(done) if done.id == task.list_id => lists::first_list(conn, &task.board_id)?.id,
        _ => task.list_id.clone(),
    };

    let next = tasks::create(
        conn,
        tasks::NewTask {
            board_id: Some(task.board_id.clone()),
            list_id: Some(target_list),
            title: task.title.clone(),
            description: task.description.clone(),
            notes: task.notes.clone(),
            priority: task.priority,
            due_at: Some(to_ts(next_due)),
            due_has_time: task.due_has_time,
            recurrence: Some(next_recurrence),
            label_ids: task.labels.iter().map(|l| l.id.clone()).collect(),
            index: None,
        },
    )?;

    for item in &task.checklist {
        tasks::add_checklist_item(conn, &next.id, &item.text)?;
    }
    // Relative reminders re-anchor themselves to the new due date.
    for reminder in &task.reminders {
        if reminder.kind == crate::models::ReminderKind::RelativeToDue {
            reminders::create_relative(conn, &next.id, reminder.offset_minutes.unwrap_or(0))?;
        }
    }

    // The series only makes sense as a chain, so the current task keeps its
    // recurrence rule for the record but stops producing further occurrences.
    crate::store::log_activity(
        conn,
        Some(&task.id),
        Some(&task.board_id),
        "task.repeated",
        "Scheduled the next occurrence",
    );
    tasks::get_hydrated(conn, &next.id).map(Some)
}

/// Handles a button press on a native notification.
///
/// Runs on the notification thread, so it takes the database lock itself and
/// must not be called from a context that already holds it.
pub fn handle_notification_action(
    app: &AppHandle,
    action: &str,
    task_id: Option<&str>,
    reminder_id: Option<&str>,
) {
    // The freedesktop backend reports a dismissed notification this way; there
    // is nothing to do, and treating it as "open" would be hostile.
    if action == "__closed" {
        return;
    }
    let state = app.state::<AppState>();

    let outcome = state.transaction(|conn| {
        match action {
            "snooze" => {
                if let Some(id) = reminder_id {
                    let minutes = crate::store::settings::get_i64(conn, "snoozeMinutes", 10);
                    reminders::snooze(conn, id, minutes)?;
                }
            }
            "complete" => {
                if let Some(id) = task_id {
                    complete_task(conn, app, id)?;
                }
            }
            _ => {}
        }
        Ok(())
    });

    if let Err(err) = outcome {
        eprintln!("tack: notification action “{action}” failed: {err}");
        return;
    }

    if matches!(action, "default" | "open")
        && let Some(id) = task_id
    {
        open_task(app, id);
    }
    notify_data_changed(app);
}

/// Brings the main window forward, restoring it if it was minimised or hidden.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Focuses the app and asks the UI to open a task.
pub fn open_task(app: &AppHandle, task_id: &str) {
    show_main_window(app);
    let _ = app.emit(OPEN_TASK_EVENT, task_id.to_string());
}
