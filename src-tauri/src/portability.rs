//! Export and import.
//!
//! The export is a single JSON document holding every board, column, task,
//! label, automation and setting, so a user can move their data somewhere else
//! or keep a copy outside the app. Attachment *files* are referenced rather
//! than embedded to keep the document readable and small; import re-copies any
//! that are still on disk.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, rejected};
use crate::models::{Automation, Board, Label, List, ReminderKind, Task};
use crate::store::{attachments, automations, boards, labels, lists, reminders, settings, tasks};

/// Bumped if the document shape changes incompatibly.
pub const EXPORT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    pub version: u32,
    pub app: String,
    pub exported_at: String,
    pub boards: Vec<Board>,
    pub lists: Vec<List>,
    pub labels: Vec<Label>,
    /// Tasks arrive hydrated, so checklists, labels and reminders travel too.
    pub tasks: Vec<Task>,
    pub automations: Vec<Automation>,
    pub settings: BTreeMap<String, Value>,
}

/// How an import should treat the data already in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportMode {
    /// Add the imported boards alongside the existing ones under fresh ids.
    Merge,
    /// Discard everything currently stored and restore the document as-is.
    Replace,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub boards: usize,
    pub lists: usize,
    pub tasks: usize,
    pub labels: usize,
    pub automations: usize,
    /// Attachments whose source file was no longer on disk.
    pub skipped_attachments: usize,
}

pub fn export(conn: &Connection) -> Result<Export> {
    let boards = boards::list(conn, true)?;
    let mut all_lists = Vec::new();
    let mut all_tasks = Vec::new();

    for board in &boards {
        all_lists.extend(lists::for_board(conn, &board.id)?);
        all_tasks.extend(tasks::for_board(conn, &board.id, true)?);
    }

    Ok(Export {
        version: EXPORT_VERSION,
        app: "Tack".into(),
        exported_at: crate::util::now(),
        boards,
        lists: all_lists,
        labels: labels::all(conn)?,
        tasks: all_tasks,
        automations: automations::all(conn)?,
        settings: settings::all(conn)?,
    })
}

pub fn export_to_file(conn: &Connection, path: &Path) -> Result<()> {
    let document = export(conn)?;
    let json = serde_json::to_string_pretty(&document)?;
    // Write beside the target and rename, so an interrupted export cannot
    // truncate a previous good file.
    let temp = path.with_extension("json.part");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

pub fn import_from_file(
    conn: &Connection,
    data_dir: &Path,
    path: &Path,
    mode: ImportMode,
) -> Result<ImportSummary> {
    let raw = std::fs::read_to_string(path)?;
    let document: Export = serde_json::from_str(&raw)
        .map_err(|e| rejected(format!("That file is not a Tack export ({e})")))?;
    import(conn, data_dir, document, mode)
}

pub fn import(
    conn: &Connection,
    data_dir: &Path,
    document: Export,
    mode: ImportMode,
) -> Result<ImportSummary> {
    if document.version > EXPORT_VERSION {
        return Err(rejected(
            "That export came from a newer version of Tack. Update the app and try again.",
        ));
    }

    if mode == ImportMode::Replace {
        // Order matters only for the tables without cascades; the rest follow
        // their foreign keys.
        conn.execute_batch(
            "DELETE FROM automations;
             DELETE FROM activity;
             DELETE FROM trigger_log;
             DELETE FROM tasks;
             DELETE FROM lists;
             DELETE FROM labels;
             DELETE FROM boards;",
        )?;
    }

    let mut summary = ImportSummary::default();
    // Fresh identifiers throughout, so a merge cannot collide with existing
    // rows and a replace cannot inherit a duplicate from a hand-edited file.
    let mut board_ids = BTreeMap::new();
    let mut list_ids = BTreeMap::new();
    let mut label_ids = BTreeMap::new();

    let existing_main = boards::main_board(conn).ok();

    for board in &document.boards {
        // On merge the app already has a Main Board, so an imported one becomes
        // an ordinary board rather than a second permanent one.
        let reuse_main = board.is_main && mode == ImportMode::Merge && existing_main.is_some();
        if reuse_main {
            let main = existing_main.as_ref().expect("checked above");
            board_ids.insert(board.id.clone(), main.id.clone());
            continue;
        }

        let created = boards::create(conn, &board.name, board.color.as_deref(), board.icon.as_deref())?;
        // `create` seeds a starter workflow; the import supplies its own.
        conn.execute("DELETE FROM lists WHERE board_id = ?1", [&created.id])?;
        conn.execute(
            "UPDATE boards SET is_main = ?2, archived = ?3, position = ?4 WHERE id = ?1",
            rusqlite::params![
                created.id,
                i64::from(board.is_main && mode == ImportMode::Replace),
                i64::from(board.archived),
                board.position
            ],
        )?;
        board_ids.insert(board.id.clone(), created.id);
        summary.boards += 1;
    }

    for list in &document.lists {
        let Some(board_id) = board_ids.get(&list.board_id) else {
            continue; // Orphaned column in a hand-edited file.
        };
        let created = lists::create(conn, board_id, &list.name, list.is_done_list)?;
        conn.execute(
            "UPDATE lists SET position = ?2, wip_limit = ?3, archived = ?4 WHERE id = ?1",
            rusqlite::params![created.id, list.position, list.wip_limit, i64::from(list.archived)],
        )?;
        list_ids.insert(list.id.clone(), created.id);
        summary.lists += 1;
    }

    for label in &document.labels {
        let board_id = label.board_id.as_ref().and_then(|id| board_ids.get(id)).cloned();
        // Reuse a global label of the same name instead of stacking duplicates.
        let existing = labels::all(conn)?.into_iter().find(|l| {
            l.name.eq_ignore_ascii_case(&label.name) && l.board_id == board_id
        });
        let resolved = match existing {
            Some(found) => found,
            None => {
                summary.labels += 1;
                labels::create(conn, &label.name, &label.color, board_id.as_deref())?
            }
        };
        label_ids.insert(label.id.clone(), resolved.id);
    }

    for task in &document.tasks {
        let Some(board_id) = board_ids.get(&task.board_id) else { continue };
        let Some(list_id) = list_ids.get(&task.list_id) else { continue };

        let created = tasks::create(
            conn,
            tasks::NewTask {
                board_id: Some(board_id.clone()),
                list_id: Some(list_id.clone()),
                title: task.title.clone(),
                description: task.description.clone(),
                notes: task.notes.clone(),
                priority: task.priority,
                due_at: task.due_at.clone(),
                due_has_time: task.due_has_time,
                recurrence: task.recurrence.clone(),
                label_ids: task
                    .labels
                    .iter()
                    .filter_map(|l| label_ids.get(&l.id).cloned())
                    .collect(),
                index: None,
            },
        )?;
        conn.execute(
            "UPDATE tasks SET completed_at = ?2, archived = ?3, position = ?4, created_at = ?5 WHERE id = ?1",
            rusqlite::params![
                created.id,
                task.completed_at,
                i64::from(task.archived),
                task.position,
                task.created_at
            ],
        )?;

        for item in &task.checklist {
            let added = tasks::add_checklist_item(conn, &created.id, &item.text)?;
            if item.done {
                tasks::update_checklist_item(conn, &added.id, None, Some(true))?;
            }
        }

        for reminder in &task.reminders {
            match reminder.kind {
                ReminderKind::RelativeToDue => {
                    reminders::create_relative(conn, &created.id, reminder.offset_minutes.unwrap_or(0))?;
                }
                ReminderKind::Absolute => {
                    if let Some(fire_at) = &reminder.fire_at {
                        reminders::create_absolute(
                            conn,
                            &created.id,
                            fire_at,
                            reminder.recurrence.clone(),
                        )?;
                    }
                }
            };
        }

        for attachment in &task.attachments {
            let source = Path::new(&attachment.path);
            if source.exists() {
                attachments::attach_file(conn, data_dir, &created.id, source)?;
            } else {
                summary.skipped_attachments += 1;
            }
        }
        summary.tasks += 1;
    }

    for automation in &document.automations {
        let board_id = automation.board_id.as_ref().and_then(|id| board_ids.get(id)).cloned();
        // A rule scoped to a board that did not import would silently never
        // run, so it is dropped rather than quietly promoted to global.
        if automation.board_id.is_some() && board_id.is_none() {
            continue;
        }
        automations::create(
            conn,
            automations::NewAutomation {
                name: automation.name.clone(),
                board_id,
                trigger: remap_trigger(&automation.trigger, &list_ids, &label_ids),
                conditions: automation.conditions.clone(),
                actions: automation.actions.clone(),
                enabled: automation.enabled,
            },
        )?;
        summary.automations += 1;
    }

    if mode == ImportMode::Replace {
        for (key, value) in &document.settings {
            // Paths and timestamps from another machine are meaningless here.
            if matches!(key.as_str(), "lastBackupAt" | "lastBoardId") {
                continue;
            }
            settings::set(conn, key, value)?;
        }
        ensure_main_board(conn)?;
    }

    Ok(summary)
}

/// Points a trigger's column and label filters at the newly created rows.
fn remap_trigger(
    trigger: &crate::models::Trigger,
    list_ids: &BTreeMap<String, String>,
    label_ids: &BTreeMap<String, String>,
) -> crate::models::Trigger {
    use crate::models::Trigger;

    let remap = |id: &Option<String>, map: &BTreeMap<String, String>| -> Option<String> {
        id.as_ref().and_then(|old| map.get(old).cloned())
    };

    match trigger {
        Trigger::TaskCreated { list_id } => Trigger::TaskCreated { list_id: remap(list_id, list_ids) },
        Trigger::TaskCompleted { list_id } => {
            Trigger::TaskCompleted { list_id: remap(list_id, list_ids) }
        }
        Trigger::TaskMoved { from_list_id, to_list_id } => Trigger::TaskMoved {
            from_list_id: remap(from_list_id, list_ids),
            to_list_id: remap(to_list_id, list_ids),
        },
        Trigger::LabelAdded { label_id } => Trigger::LabelAdded { label_id: remap(label_id, label_ids) },
        Trigger::LabelRemoved { label_id } => {
            Trigger::LabelRemoved { label_id: remap(label_id, label_ids) }
        }
        other => other.clone(),
    }
}

/// Guarantees the invariant the whole app relies on: exactly one Main Board.
fn ensure_main_board(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM boards WHERE is_main = 1", [], |r| r.get(0))?;
    match count {
        1 => Ok(()),
        0 => {
            let first: Option<String> = conn
                .query_row("SELECT id FROM boards ORDER BY position LIMIT 1", [], |r| r.get(0))
                .ok();
            match first {
                Some(id) => {
                    conn.execute("UPDATE boards SET is_main = 1 WHERE id = ?1", [&id])?;
                }
                None => {
                    let board = boards::create(conn, "Main Board", None, Some("📌"))?;
                    conn.execute("UPDATE boards SET is_main = 1 WHERE id = ?1", [&board.id])?;
                }
            }
            Ok(())
        }
        _ => {
            // Keep the oldest and demote the rest.
            conn.execute(
                "UPDATE boards SET is_main = 0 WHERE id NOT IN
                   (SELECT id FROM boards WHERE is_main = 1 ORDER BY created_at LIMIT 1)",
                [],
            )?;
            Ok(())
        }
    }
}
