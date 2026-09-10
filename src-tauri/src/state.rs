//! Shared application state: the single database connection plus the few flags
//! that need to be readable without taking the database lock.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::{Mutex, MutexGuard};
use rusqlite::Connection;

use crate::error::Result;

pub struct AppState {
    conn: Mutex<Connection>,
    pub data_dir: PathBuf,
    /// Mirror of the `notificationsEnabled` setting.
    ///
    /// The notification path is reached from inside automation actions, which
    /// already hold the database lock; reading the setting from the database
    /// there would deadlock, so the flag is cached here instead.
    notifications_enabled: AtomicBool,
    /// Mirror of `closeToTray`, read from the window close handler.
    close_to_tray: AtomicBool,
}

impl AppState {
    pub fn new(conn: Connection, data_dir: PathBuf) -> Self {
        let state = Self {
            conn: Mutex::new(conn),
            data_dir,
            notifications_enabled: AtomicBool::new(true),
            close_to_tray: AtomicBool::new(true),
        };
        state.refresh_cached_settings();
        state
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Runs `f` with the connection inside a transaction, committing on `Ok`
    /// and rolling back on `Err` so a failed multi-step edit leaves no residue.
    pub fn transaction<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    pub fn notifications_enabled(&self) -> bool {
        self.notifications_enabled.load(Ordering::Relaxed)
    }

    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray.load(Ordering::Relaxed)
    }

    /// Re-reads the cached settings. Called at startup and after every save.
    pub fn refresh_cached_settings(&self) {
        let conn = self.conn.lock();
        let notifications = crate::store::settings::get_bool(&conn, "notificationsEnabled", true);
        let close_to_tray = crate::store::settings::get_bool(&conn, "closeToTray", true);
        self.notifications_enabled.store(notifications, Ordering::Relaxed);
        self.close_to_tray.store(close_to_tray, Ordering::Relaxed);
    }
}
