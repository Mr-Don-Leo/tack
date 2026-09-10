//! The background engine.
//!
//! One thread drives reminders, time-based automation triggers and rolling
//! backups. It owns no UI state, so it keeps running with every window closed —
//! which is what makes "closing the window keeps reminders working" true rather
//! than aspirational.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use tauri::{AppHandle, Manager};

use crate::error::Result;
use crate::models::Task;
use crate::state::AppState;
use crate::store::{automations as auto_store, reminders, settings};
use crate::util::{parse_ts, to_ts};

/// How often the engine wakes. Short enough that a reminder is never noticeably
/// late, long enough to be invisible in a CPU graph.
const TICK: StdDuration = StdDuration::from_secs(20);

/// Starts the engine thread. Returns a flag that stops it when cleared.
pub fn start(app: AppHandle) -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();

    std::thread::Builder::new()
        .name("tack-engine".into())
        .spawn(move || {
            // A first pass right away so anything missed while the app was
            // closed fires as soon as it opens.
            while flag.load(Ordering::Relaxed) {
                if let Err(err) = tick(&app) {
                    eprintln!("tack: engine tick failed: {err}");
                }
                std::thread::sleep(TICK);
            }
        })
        .expect("engine thread spawns");

    running
}

fn tick(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let now = Utc::now();

    // Reminders first: the user-visible work should not wait on housekeeping.
    let fired = state.transaction(|conn| {
        let due = reminders::due_now(conn, now)?;
        for item in &due {
            reminders::mark_fired(conn, &item.reminder)?;
        }
        Ok(due)
    })?;

    for item in &fired {
        crate::notify::send(
            app,
            crate::notify::Notification {
                title: item.task_title.clone(),
                body: reminder_body(&item.board_name, item.reminder.fire_at.as_deref()),
                task_id: Some(item.task_id.clone()),
                reminder_id: Some(item.reminder.id.clone()),
                actions: true,
            },
        );
    }

    let changed = state.transaction(|conn| {
        crate::automations::run_time_triggers(conn, app)?;
        crate::automations::run_scheduled(conn, app)?;
        Ok(())
    });
    if let Err(err) = changed {
        eprintln!("tack: automation pass failed: {err}");
    }

    housekeeping(&state)?;

    if !fired.is_empty() {
        crate::ops::notify_data_changed(app);
    }
    Ok(())
}

/// Body text for a reminder: which board it belongs to, and how it relates to
/// the due time.
fn reminder_body(board_name: &str, fire_at: Option<&str>) -> String {
    let when = fire_at.and_then(parse_ts).map(|at| {
        let delta = at - Utc::now();
        let minutes = delta.num_minutes();
        match minutes {
            m if m > 1440 => format!("in {} days", m / 1440),
            m if m > 60 => format!("in {} hours", m / 60),
            m if m > 1 => format!("in {m} minutes"),
            m if m >= -1 => "now".to_string(),
            m if m > -60 => format!("{} minutes ago", -m),
            m if m > -1440 => format!("{} hours ago", -m / 60),
            m => format!("{} days ago", -m / 1440),
        }
    });
    match when {
        Some(when) => format!("{board_name} · Due {when}"),
        None => board_name.to_string(),
    }
}

/// Rolling backups and log pruning, rate-limited by a stored timestamp so they
/// survive restarts without running on every launch.
fn housekeeping(state: &AppState) -> Result<()> {
    let conn = state.db();

    let interval_hours = settings::get_i64(&conn, "backupIntervalHours", 6).clamp(1, 24 * 7);
    let last_backup = settings::get_string(&conn, "lastBackupAt")?;
    let due = last_backup
        .as_deref()
        .and_then(parse_ts)
        .is_none_or(|last| Utc::now() - last >= Duration::hours(interval_hours));
    if !due {
        return Ok(());
    }

    match crate::db::create_backup(&conn, &state.data_dir) {
        Ok(path) => {
            settings::set(&conn, "lastBackupAt", &serde_json::json!(to_ts(Utc::now())))?;
            eprintln!("tack: wrote backup {}", path.display());
        }
        // A failed backup is worth reporting but must not stop the engine.
        Err(err) => eprintln!("tack: backup failed: {err}"),
    }
    // The trigger log only exists to deduplicate recent firings.
    let _ = auto_store::prune_trigger_log(&conn, 30);
    Ok(())
}

/// Reminders that have already fired but were never acted on, so the UI can
/// show them the next time the window opens.
pub fn outstanding_alerts(conn: &rusqlite::Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "{} WHERE completed_at IS NULL AND archived = 0 AND id IN (
            SELECT task_id FROM reminders WHERE dismissed = 0 AND fired_at IS NOT NULL
         ) ORDER BY due_at IS NULL, due_at ASC LIMIT 20",
        crate::store::tasks::SELECT
    ))?;
    let tasks = stmt
        .query_map([], crate::store::tasks::map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    crate::store::tasks::hydrate_all(conn, tasks)
}
