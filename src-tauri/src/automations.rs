//! The automation engine: trigger → conditions → actions.
//!
//! It runs entirely against the database and an `AppHandle` for notifications,
//! so it works with the window closed. Actions that themselves look like
//! triggers (moving, completing) are re-fed into the engine with a depth cap,
//! which lets rules chain without a pair of rules looping forever.

use chrono::{Duration, Local, TimeZone, Utc};
use rusqlite::Connection;
use tauri::AppHandle;

use crate::error::Result;
use crate::models::{Automation, AutomationAction, Condition, Id, Schedule, Task, Trigger};
use crate::store::{self, automations as auto_store, labels, lists, reminders, tasks};
use crate::util::{parse_ts, to_ts};

/// How many times an action may re-trigger the engine before we stop. Three is
/// enough for realistic chains ("move to Done" → "complete" → "notify") and
/// short enough that a cycle costs nothing.
const MAX_DEPTH: u8 = 3;

/// Something that happened and that rules may react to.
#[derive(Debug, Clone)]
pub enum Event {
    TaskCreated { task_id: Id },
    TaskCompleted { task_id: Id },
    TaskOverdue { task_id: Id },
    DueDateReached { task_id: Id },
    TaskMoved { task_id: Id, from_list_id: Id, to_list_id: Id },
    LabelAdded { task_id: Id, label_id: Id },
    LabelRemoved { task_id: Id, label_id: Id },
}

impl Event {
    fn task_id(&self) -> &str {
        match self {
            Event::TaskCreated { task_id }
            | Event::TaskCompleted { task_id }
            | Event::TaskOverdue { task_id }
            | Event::DueDateReached { task_id }
            | Event::TaskMoved { task_id, .. }
            | Event::LabelAdded { task_id, .. }
            | Event::LabelRemoved { task_id, .. } => task_id,
        }
    }
}

/// Runs every rule that matches `event`. Errors are logged, not propagated: a
/// broken rule must not roll back the user's edit that triggered it.
pub fn dispatch(conn: &Connection, app: &AppHandle, event: Event) {
    if let Err(err) = dispatch_inner(conn, app, &event, 0) {
        eprintln!("tack: automation dispatch failed: {err}");
    }
}

fn dispatch_inner(conn: &Connection, app: &AppHandle, event: &Event, depth: u8) -> Result<()> {
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    let Ok(task) = tasks::get_hydrated(conn, event.task_id()) else {
        return Ok(()); // The task was deleted between the edit and the dispatch.
    };

    for automation in auto_store::active_for_board(conn, &task.board_id)? {
        // Re-read: an earlier rule in this pass may have changed the task.
        let Ok(current) = tasks::get_hydrated(conn, &task.id) else {
            break;
        };
        if !trigger_matches(&automation.trigger, event, &current) {
            continue;
        }
        if !conditions_pass(&automation.conditions, Some(&current))? {
            continue;
        }
        run_actions(conn, app, &automation, Some(&current), depth)?;
        auto_store::record_run(conn, &automation.id)?;
    }
    Ok(())
}

/// `task` supplies the column for triggers whose filter is about where the task
/// currently sits rather than about the event itself.
fn trigger_matches(trigger: &Trigger, event: &Event, task: &Task) -> bool {
    /// `None` in a rule means "any", so an unset filter always matches.
    fn matches_opt(filter: &Option<Id>, actual: &str) -> bool {
        filter.as_deref().is_none_or(|want| want == actual)
    }

    match (trigger, event) {
        (Trigger::TaskCreated { list_id }, Event::TaskCreated { .. }) => {
            matches_opt(list_id, &task.list_id)
        }
        (Trigger::TaskCompleted { list_id }, Event::TaskCompleted { .. }) => {
            matches_opt(list_id, &task.list_id)
        }
        (Trigger::TaskOverdue, Event::TaskOverdue { .. }) => true,
        (Trigger::DueDateReached, Event::DueDateReached { .. }) => true,
        (
            Trigger::TaskMoved { from_list_id, to_list_id },
            Event::TaskMoved { from_list_id: from, to_list_id: to, .. },
        ) => matches_opt(from_list_id, from) && matches_opt(to_list_id, to),
        (Trigger::LabelAdded { label_id }, Event::LabelAdded { label_id: actual, .. }) => {
            matches_opt(label_id, actual)
        }
        (Trigger::LabelRemoved { label_id }, Event::LabelRemoved { label_id: actual, .. }) => {
            matches_opt(label_id, actual)
        }
        _ => false,
    }
}

/// Every condition must hold. Task-scoped conditions fail when there is no
/// subject task, which is what keeps a scheduled rule from firing blindly.
fn conditions_pass(conditions: &[Condition], task: Option<&Task>) -> Result<bool> {
    for condition in conditions {
        let Some(task) = task else { return Ok(false) };
        let ok = match condition {
            Condition::HasLabel { label_id } => task.labels.iter().any(|l| &l.id == label_id),
            Condition::LacksLabel { label_id } => !task.labels.iter().any(|l| &l.id == label_id),
            Condition::PriorityAtLeast { priority } => task.priority >= *priority,
            Condition::PriorityEquals { priority } => task.priority == *priority,
            Condition::InList { list_id } => &task.list_id == list_id,
            Condition::InBoard { board_id } => &task.board_id == board_id,
            Condition::TitleContains { text } => {
                task.title.to_lowercase().contains(&text.trim().to_lowercase())
            }
            Condition::IsOverdue => parse_ts_due(task).is_some_and(|due| due < Utc::now()),
            Condition::IsCompleted => task.completed_at.is_some(),
            Condition::IsNotCompleted => task.completed_at.is_none(),
            Condition::HasDueDate => task.due_at.is_some(),
            Condition::HasNoDueDate => task.due_at.is_none(),
            Condition::DueWithinMinutes { minutes } => parse_ts_due(task)
                .is_some_and(|due| due <= Utc::now() + Duration::minutes(*minutes) && due >= Utc::now()),
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_ts_due(task: &Task) -> Option<chrono::DateTime<Utc>> {
    task.due_at.as_deref().and_then(parse_ts)
}

fn run_actions(
    conn: &Connection,
    app: &AppHandle,
    automation: &Automation,
    subject: Option<&Task>,
    depth: u8,
) -> Result<()> {
    for action in &automation.actions {
        if let Err(err) = run_action(conn, app, automation, action, subject, depth) {
            eprintln!("tack: automation “{}” action failed: {err}", automation.name);
        }
    }
    Ok(())
}

fn run_action(
    conn: &Connection,
    app: &AppHandle,
    automation: &Automation,
    action: &AutomationAction,
    subject: Option<&Task>,
    depth: u8,
) -> Result<()> {
    let next_depth = depth + 1;

    match action {
        AutomationAction::MoveTask { list_id, .. } => {
            let Some(task) = subject else { return Ok(()) };
            if task.list_id == *list_id {
                return Ok(());
            }
            let from = task.list_id.clone();
            tasks::move_task(conn, &task.id, list_id, Some(0))?;
            dispatch_inner(
                conn,
                app,
                &Event::TaskMoved {
                    task_id: task.id.clone(),
                    from_list_id: from,
                    to_list_id: list_id.clone(),
                },
                next_depth,
            )?;
        }

        AutomationAction::CreateTask {
            board_id,
            list_id,
            title,
            description,
            priority,
            due_in_minutes,
            label_ids,
        } => {
            let board_id = board_id
                .clone()
                .or_else(|| automation.board_id.clone())
                .or_else(|| subject.map(|t| t.board_id.clone()));
            let created = tasks::create(
                conn,
                tasks::NewTask {
                    board_id,
                    list_id: list_id.clone(),
                    title: title.clone(),
                    description: description.clone(),
                    priority: *priority,
                    due_at: due_in_minutes.map(|m| to_ts(Utc::now() + Duration::minutes(m))),
                    due_has_time: due_in_minutes.is_some(),
                    label_ids: label_ids.clone(),
                    ..Default::default()
                },
            )?;
            dispatch_inner(conn, app, &Event::TaskCreated { task_id: created.id }, next_depth)?;
        }

        AutomationAction::CompleteTask => {
            let Some(task) = subject else { return Ok(()) };
            if task.completed_at.is_some() {
                return Ok(());
            }
            tasks::complete(conn, &task.id)?;
            crate::ops::spawn_next_occurrence(conn, &task.id)?;
            dispatch_inner(conn, app, &Event::TaskCompleted { task_id: task.id.clone() }, next_depth)?;
        }

        AutomationAction::SetPriority { priority } => {
            let Some(task) = subject else { return Ok(()) };
            tasks::update(
                conn,
                &task.id,
                tasks::TaskPatch { priority: Some(*priority), ..Default::default() },
            )?;
        }

        AutomationAction::AddLabel { label_id } => {
            let Some(task) = subject else { return Ok(()) };
            if labels::attach(conn, &task.id, label_id)? {
                dispatch_inner(
                    conn,
                    app,
                    &Event::LabelAdded { task_id: task.id.clone(), label_id: label_id.clone() },
                    next_depth,
                )?;
            }
        }

        AutomationAction::RemoveLabel { label_id } => {
            let Some(task) = subject else { return Ok(()) };
            if labels::detach(conn, &task.id, label_id)? {
                dispatch_inner(
                    conn,
                    app,
                    &Event::LabelRemoved { task_id: task.id.clone(), label_id: label_id.clone() },
                    next_depth,
                )?;
            }
        }

        AutomationAction::SetDueDate { in_minutes } => {
            let Some(task) = subject else { return Ok(()) };
            tasks::update(
                conn,
                &task.id,
                tasks::TaskPatch {
                    due_at: Some(Some(to_ts(Utc::now() + Duration::minutes(*in_minutes)))),
                    due_has_time: Some(true),
                    ..Default::default()
                },
            )?;
        }

        AutomationAction::SetReminder { in_minutes } => {
            let Some(task) = subject else { return Ok(()) };
            let fire_at = to_ts(Utc::now() + Duration::minutes(*in_minutes));
            reminders::create_absolute(conn, &task.id, &fire_at, None)?;
        }

        AutomationAction::Notify { title, body } => {
            let body = match subject {
                Some(task) => body.replace("{task}", &task.title),
                None => body.clone(),
            };
            crate::notify::send(
                app,
                crate::notify::Notification {
                    title: title.clone(),
                    body,
                    task_id: subject.map(|t| t.id.clone()),
                    reminder_id: None,
                    actions: false,
                },
            );
        }

        AutomationAction::DuplicateTask => {
            let Some(task) = subject else { return Ok(()) };
            let copy = tasks::duplicate(conn, &task.id)?;
            dispatch_inner(conn, app, &Event::TaskCreated { task_id: copy.id }, next_depth)?;
        }

        AutomationAction::ArchiveTask => {
            let Some(task) = subject else { return Ok(()) };
            tasks::update(
                conn,
                &task.id,
                tasks::TaskPatch { archived: Some(true), ..Default::default() },
            )?;
            reminders::dismiss_for_task(conn, &task.id)?;
        }
    }
    Ok(())
}

/// How far back a missed scheduled occurrence is still worth running. Covers a
/// laptop that was asleep over the rule's time without replaying a week of
/// "Plan the week" tasks after a long shutdown.
const SCHEDULE_CATCHUP_MINUTES: i64 = 60;

/// Runs scheduled rules whose wall-clock time has arrived.
///
/// Rather than testing "is it exactly now", this finds each rule's most recent
/// occurrence and claims it in `trigger_log`. That makes the engine tolerant of
/// a skipped tick, a suspended machine or a restart, while still firing exactly
/// once per occurrence.
pub fn run_scheduled(conn: &Connection, app: &AppHandle) -> Result<()> {
    let now = Local::now();

    for automation in auto_store::active_scheduled(conn)? {
        let Trigger::Scheduled { schedule } = &automation.trigger else {
            continue;
        };
        let Some(occurrence) = last_occurrence(schedule, now) else {
            continue;
        };
        if (now - occurrence).num_minutes() > SCHEDULE_CATCHUP_MINUTES {
            continue;
        }
        let key = format!("sched:{}:{}", automation.id, occurrence.format("%Y-%m-%dT%H:%M"));
        if !auto_store::claim_once(conn, &key)? {
            continue;
        }
        // Task-scoped conditions cannot hold without a subject, so a scheduled
        // rule with conditions attached simply never fires.
        if !conditions_pass(&automation.conditions, None)? {
            continue;
        }
        run_actions(conn, app, &automation, None, 0)?;
        auto_store::record_run(conn, &automation.id)?;
    }
    Ok(())
}

/// The latest instant at or before `now` that satisfies `schedule`, searching
/// back far enough to cover a monthly rule.
fn last_occurrence(
    schedule: &Schedule,
    now: chrono::DateTime<Local>,
) -> Option<chrono::DateTime<Local>> {
    use chrono::Datelike;

    if schedule.hour > 23 || schedule.minute > 59 {
        return None;
    }
    for days_back in 0..=62 {
        let day = (now - Duration::days(days_back)).date_naive();

        if !schedule.weekdays.is_empty() {
            let weekday = day.weekday().num_days_from_monday() as u8;
            if !schedule.weekdays.contains(&weekday) {
                continue;
            }
        }
        if let Some(target) = schedule.day_of_month
            && day.day() != target
        {
            continue;
        }

        let naive = day.and_hms_opt(schedule.hour, schedule.minute, 0)?;
        let Some(candidate) = Local.from_local_datetime(&naive).earliest() else {
            continue; // Skipped by a daylight-saving transition.
        };
        if candidate <= now {
            return Some(candidate);
        }
    }
    None
}

/// Detects tasks that have just crossed their due date and fires the matching
/// triggers. Claims keep each task to one `DueDateReached` and one `TaskOverdue`.
pub fn run_time_triggers(conn: &Connection, app: &AppHandle) -> Result<()> {
    let now = Utc::now();
    let mut stmt = conn.prepare(&format!(
        "{} WHERE completed_at IS NULL AND archived = 0 AND due_at IS NOT NULL AND due_at <= ?1
         ORDER BY due_at ASC LIMIT 200",
        tasks::SELECT
    ))?;
    let due: Vec<Task> = stmt
        .query_map(rusqlite::params![to_ts(now)], tasks::map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for task in due {
        let Some(due_at) = task.due_at.as_deref() else { continue };

        if auto_store::claim_once(conn, &format!("due:{}:{due_at}", task.id))? {
            dispatch_inner(conn, app, &Event::DueDateReached { task_id: task.id.clone() }, 0)?;
        }
        // "Overdue" is a distinct moment from "due": give it a short grace
        // period so both triggers are not indistinguishable to the user.
        if parse_ts(due_at).is_some_and(|d| d + Duration::minutes(1) <= now)
            && auto_store::claim_once(conn, &format!("overdue:{}:{due_at}", task.id))?
        {
            dispatch_inner(conn, app, &Event::TaskOverdue { task_id: task.id.clone() }, 0)?;
        }
    }
    Ok(())
}

/// Convenience used by the "when a card is moved to Done" default rule.
pub fn seed_default_rules(conn: &Connection) -> Result<()> {
    if !auto_store::all(conn)?.is_empty() {
        return Ok(());
    }
    let main = store::boards::main_board(conn)?;
    let Some(done) = lists::done_list(conn, &main.id)? else {
        return Ok(());
    };
    auto_store::create(
        conn,
        auto_store::NewAutomation {
            name: "Completing cards moved to Done".into(),
            board_id: None,
            trigger: Trigger::TaskMoved { from_list_id: None, to_list_id: Some(done.id) },
            conditions: vec![Condition::IsNotCompleted],
            actions: vec![AutomationAction::CompleteTask],
            enabled: true,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task(list_id: &str) -> Task {
        Task {
            id: "t".into(),
            board_id: "b".into(),
            list_id: list_id.into(),
            title: "Write the release notes".into(),
            description: String::new(),
            notes: String::new(),
            priority: 0,
            due_at: None,
            due_has_time: false,
            completed_at: None,
            archived: false,
            position: 1024.0,
            recurrence: None,
            created_at: "2026-09-08T09:00:00Z".into(),
            updated_at: "2026-09-08T09:00:00Z".into(),
            labels: vec![],
            checklist: vec![],
            attachments: vec![],
            reminders: vec![],
        }
    }

    #[test]
    fn move_trigger_respects_target_column() {
        let trigger = Trigger::TaskMoved {
            from_list_id: None,
            to_list_id: Some("done".into()),
        };
        let to_done = Event::TaskMoved {
            task_id: "t".into(),
            from_list_id: "todo".into(),
            to_list_id: "done".into(),
        };
        let to_review = Event::TaskMoved {
            task_id: "t".into(),
            from_list_id: "todo".into(),
            to_list_id: "review".into(),
        };
        let task = sample_task("done");
        assert!(trigger_matches(&trigger, &to_done, &task));
        assert!(!trigger_matches(&trigger, &to_review, &task));
    }

    #[test]
    fn label_trigger_without_a_filter_matches_any_label() {
        let trigger = Trigger::LabelAdded { label_id: None };
        let event = Event::LabelAdded { task_id: "t".into(), label_id: "urgent".into() };
        assert!(trigger_matches(&trigger, &event, &sample_task("todo")));
    }

    #[test]
    fn created_trigger_can_be_scoped_to_a_column() {
        let trigger = Trigger::TaskCreated { list_id: Some("inbox".into()) };
        let event = Event::TaskCreated { task_id: "t".into() };
        assert!(trigger_matches(&trigger, &event, &sample_task("inbox")));
        assert!(!trigger_matches(&trigger, &event, &sample_task("todo")));
    }

    #[test]
    fn conditions_without_a_subject_never_pass() {
        let conditions = vec![Condition::IsNotCompleted];
        assert!(!conditions_pass(&conditions, None).unwrap());
        assert!(conditions_pass(&[], None).unwrap());
    }

    #[test]
    fn title_condition_is_case_insensitive() {
        let task = sample_task("todo");
        let condition = vec![Condition::TitleContains { text: "RELEASE".into() }];
        assert!(conditions_pass(&condition, Some(&task)).unwrap());
    }

    #[test]
    fn weekly_schedule_resolves_to_the_selected_weekday() {
        // 2026-09-07 is a Monday; asking on Tuesday should point back to it.
        let tuesday = Local.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap();
        let schedule = Schedule { hour: 9, minute: 0, weekdays: vec![0], day_of_month: None };
        let occurrence = last_occurrence(&schedule, tuesday).unwrap();
        assert_eq!(occurrence, Local.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap());
    }

    #[test]
    fn daily_schedule_before_its_time_resolves_to_yesterday() {
        let early = Local.with_ymd_and_hms(2026, 9, 8, 7, 30, 0).unwrap();
        let schedule = Schedule { hour: 9, minute: 0, weekdays: vec![], day_of_month: None };
        let occurrence = last_occurrence(&schedule, early).unwrap();
        assert_eq!(occurrence, Local.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap());
    }

    #[test]
    fn monthly_schedule_honours_the_day_of_month() {
        let now = Local.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).unwrap();
        let schedule = Schedule { hour: 8, minute: 0, weekdays: vec![], day_of_month: Some(1) };
        let occurrence = last_occurrence(&schedule, now).unwrap();
        assert_eq!(occurrence, Local.with_ymd_and_hms(2026, 9, 1, 8, 0, 0).unwrap());
    }

    #[test]
    fn stale_occurrences_fall_outside_the_catch_up_window() {
        let now = Local.with_ymd_and_hms(2026, 9, 8, 18, 0, 0).unwrap();
        let schedule = Schedule { hour: 9, minute: 0, weekdays: vec![], day_of_month: None };
        let occurrence = last_occurrence(&schedule, now).unwrap();
        assert!((now - occurrence).num_minutes() > SCHEDULE_CATCHUP_MINUTES);
    }
}
