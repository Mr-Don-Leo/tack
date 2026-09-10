//! SQLite setup: durability pragmas, schema migrations, seed data and backups.
//!
//! The database lives at `<app data>/tack.db` in WAL mode with full syncing, so
//! a crash or power loss mid-write rolls back to the last committed transaction
//! rather than leaving a torn file. On startup we verify integrity and fall back
//! to the newest good backup if the primary file is unreadable.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::util::{new_id, now};

/// Bump when adding a migration step below.
const SCHEMA_VERSION: i64 = 1;

/// How many rolling backups to keep before pruning the oldest.
pub const MAX_BACKUPS: usize = 10;

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("tack.db")
}

pub fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

pub fn attachments_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("attachments")
}

/// Opens the database, repairing from a backup if the primary file is corrupt.
pub fn open(data_dir: &Path) -> Result<Connection> {
    fs::create_dir_all(data_dir)?;
    fs::create_dir_all(backup_dir(data_dir))?;
    fs::create_dir_all(attachments_dir(data_dir))?;

    let path = db_path(data_dir);
    match open_verified(&path) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            eprintln!("tack: database at {} is unusable ({err}); attempting recovery", path.display());
            recover_from_backup(data_dir, &path)?;
            open_verified(&path)
        }
    }
}

/// Opens a connection, applies pragmas and migrations, and integrity-checks it.
fn open_verified(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    apply_pragmas(&conn)?;

    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err(Error::Rejected(format!("integrity check failed: {integrity}")));
    }

    migrate(&conn)?;
    seed(&conn)?;
    Ok(conn)
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    // `journal_mode` returns a row, so it needs query_row rather than execute.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch(
        "PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

/// Moves the damaged file aside and restores the newest backup in its place.
fn recover_from_backup(data_dir: &Path, path: &Path) -> Result<()> {
    let newest = list_backups(data_dir)?
        .into_iter()
        .next_back()
        .ok_or_else(|| Error::Rejected("database is corrupt and no backup is available".into()))?;

    if path.exists() {
        let quarantine = data_dir.join(format!("tack.corrupt-{}.db", crate::util::now().replace(':', "-")));
        fs::rename(path, &quarantine)?;
        eprintln!("tack: moved damaged database to {}", quarantine.display());
    }
    // Stale WAL/SHM would be replayed onto the restored file and re-corrupt it.
    let _ = fs::remove_file(path.with_extension("db-wal"));
    let _ = fs::remove_file(path.with_extension("db-shm"));
    fs::copy(&newest, path)?;
    eprintln!("tack: restored database from {}", newest.display());
    Ok(())
}

/// Backup files, oldest first. Names sort chronologically by construction.
pub fn list_backups(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let dir = backup_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "db"))
        .collect();
    files.sort();
    Ok(files)
}

/// Writes a consistent snapshot using SQLite's online backup API, then prunes.
///
/// This is safe to call while the app is running: the backup API copies pages
/// under a read lock instead of reading the file behind SQLite's back.
pub fn create_backup(conn: &Connection, data_dir: &Path) -> Result<PathBuf> {
    let dir = backup_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest_path = dir.join(format!("tack-{stamp}.db"));

    let mut dest = Connection::open(&dest_path)?;
    {
        let backup = rusqlite::backup::Backup::new(conn, &mut dest)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
    }
    drop(dest);

    prune_backups(data_dir)?;
    Ok(dest_path)
}

fn prune_backups(data_dir: &Path) -> Result<()> {
    let files = list_backups(data_dir)?;
    if files.len() > MAX_BACKUPS {
        for old in &files[..files.len() - MAX_BACKUPS] {
            let _ = fs::remove_file(old);
        }
    }
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
    }
    conn.execute(&format!("PRAGMA user_version = {SCHEMA_VERSION}"), [])?;
    Ok(())
}

/// Creates the permanent Main Board on first run. Idempotent.
fn seed(conn: &Connection) -> Result<()> {
    let existing: Option<String> = conn
        .query_row("SELECT id FROM boards WHERE is_main = 1", [], |r| r.get(0))
        .optional()?;
    if existing.is_some() {
        return Ok(());
    }

    let ts = now();
    let board_id = new_id();
    conn.execute(
        "INSERT INTO boards (id, name, color, icon, position, is_main, archived, created_at, updated_at)
         VALUES (?1, 'Main Board', NULL, '📌', 1024.0, 1, 0, ?2, ?2)",
        rusqlite::params![board_id, ts],
    )?;

    for (i, (name, is_done)) in [("Inbox", 0), ("To Do", 0), ("In Progress", 0), ("Done", 1)]
        .iter()
        .enumerate()
    {
        conn.execute(
            "INSERT INTO lists (id, board_id, name, position, is_done_list, wip_limit, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, ?6, ?6)",
            rusqlite::params![
                new_id(),
                board_id,
                name,
                (i as f64 + 1.0) * crate::util::POSITION_STEP,
                is_done,
                ts
            ],
        )?;
    }

    // Global labels, available on every board.
    for (name, color) in [
        ("Urgent", "#FF3B30"),
        ("Work", "#007AFF"),
        ("Personal", "#34C759"),
        ("Idea", "#AF52DE"),
        ("Waiting", "#FF9500"),
    ] {
        conn.execute(
            "INSERT INTO labels (id, name, color, board_id, created_at) VALUES (?1, ?2, ?3, NULL, ?4)",
            rusqlite::params![new_id(), name, color, ts],
        )?;
    }

    Ok(())
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS boards (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  color      TEXT,
  icon       TEXT,
  position   REAL NOT NULL,
  is_main    INTEGER NOT NULL DEFAULT 0,
  archived   INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_boards_position ON boards(position);

CREATE TABLE IF NOT EXISTS lists (
  id           TEXT PRIMARY KEY,
  board_id     TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  position     REAL NOT NULL,
  is_done_list INTEGER NOT NULL DEFAULT 0,
  wip_limit    INTEGER,
  archived     INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lists_board ON lists(board_id, position);

CREATE TABLE IF NOT EXISTS tasks (
  id           TEXT PRIMARY KEY,
  board_id     TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
  list_id      TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
  title        TEXT NOT NULL,
  description  TEXT NOT NULL DEFAULT '',
  notes        TEXT NOT NULL DEFAULT '',
  priority     INTEGER NOT NULL DEFAULT 0,
  due_at       TEXT,
  due_has_time INTEGER NOT NULL DEFAULT 0,
  completed_at TEXT,
  archived     INTEGER NOT NULL DEFAULT 0,
  position     REAL NOT NULL,
  recurrence   TEXT,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_list ON tasks(list_id, position);
CREATE INDEX IF NOT EXISTS idx_tasks_board ON tasks(board_id, archived);
CREATE INDEX IF NOT EXISTS idx_tasks_due ON tasks(due_at) WHERE due_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_open ON tasks(completed_at, archived);

CREATE TABLE IF NOT EXISTS labels (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  color      TEXT NOT NULL,
  board_id   TEXT REFERENCES boards(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_labels_board ON labels(board_id);

CREATE TABLE IF NOT EXISTS task_labels (
  task_id  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  label_id TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
  PRIMARY KEY (task_id, label_id)
);
CREATE INDEX IF NOT EXISTS idx_task_labels_label ON task_labels(label_id);

CREATE TABLE IF NOT EXISTS checklist_items (
  id         TEXT PRIMARY KEY,
  task_id    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  text       TEXT NOT NULL,
  done       INTEGER NOT NULL DEFAULT 0,
  position   REAL NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_checklist_task ON checklist_items(task_id, position);

CREATE TABLE IF NOT EXISTS attachments (
  id         TEXT PRIMARY KEY,
  task_id    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  path       TEXT NOT NULL,
  size       INTEGER NOT NULL DEFAULT 0,
  mime       TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_attachments_task ON attachments(task_id);

CREATE TABLE IF NOT EXISTS reminders (
  id             TEXT PRIMARY KEY,
  task_id        TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind           TEXT NOT NULL,
  offset_minutes INTEGER,
  fire_at        TEXT,
  recurrence     TEXT,
  snoozed_until  TEXT,
  fired_at       TEXT,
  dismissed      INTEGER NOT NULL DEFAULT 0,
  created_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_reminders_task ON reminders(task_id);
CREATE INDEX IF NOT EXISTS idx_reminders_pending ON reminders(dismissed, fire_at);

CREATE TABLE IF NOT EXISTS automations (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  enabled     INTEGER NOT NULL DEFAULT 1,
  board_id    TEXT REFERENCES boards(id) ON DELETE CASCADE,
  trigger     TEXT NOT NULL,
  conditions  TEXT NOT NULL DEFAULT '[]',
  actions     TEXT NOT NULL DEFAULT '[]',
  last_run_at TEXT,
  run_count   INTEGER NOT NULL DEFAULT 0,
  position    REAL NOT NULL DEFAULT 1024.0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_automations_board ON automations(board_id, enabled);

CREATE TABLE IF NOT EXISTS activity (
  id         TEXT PRIMARY KEY,
  task_id    TEXT REFERENCES tasks(id) ON DELETE CASCADE,
  board_id   TEXT REFERENCES boards(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  message    TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_activity_task ON activity(task_id, created_at DESC);

-- Bookkeeping so scheduled automations and overdue checks fire exactly once
-- per occurrence even across restarts.
CREATE TABLE IF NOT EXISTS trigger_log (
  key        TEXT PRIMARY KEY,
  created_at TEXT NOT NULL
);
"#;
