//! Tack — boards, tasks, reminders and automations for the desktop.
//!
//! The app is split so the parts that must work without a window really can:
//! `store` is pure data access, `ops` adds product behaviour, `engine` drives
//! reminders and scheduled automations on its own thread, and only `commands`
//! and `tray` know a user interface exists.

mod automations;
mod commands;
mod db;
mod engine;
mod error;
mod models;
mod nlp;
mod notify;
mod ops;
mod portability;
mod quickadd;
mod recurrence;
mod state;
mod store;
mod tray;
mod util;

use tauri::{Manager, WindowEvent};

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // A second launch should surface the running app, not start a rival copy
    // with its own lock on the database.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            ops::show_main_window(app);
        }));
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::list_boards,
            commands::board_view,
            commands::create_board,
            commands::update_board,
            commands::delete_board,
            commands::reorder_board,
            commands::create_list,
            commands::update_list,
            commands::delete_list,
            commands::reorder_list,
            commands::create_task,
            commands::update_task,
            commands::move_task,
            commands::set_task_completed,
            commands::delete_task,
            commands::duplicate_task,
            commands::get_task,
            commands::task_activity,
            commands::add_checklist_item,
            commands::update_checklist_item,
            commands::delete_checklist_item,
            commands::list_labels,
            commands::create_label,
            commands::update_label,
            commands::delete_label,
            commands::set_task_label,
            commands::add_reminder,
            commands::delete_reminder,
            commands::snooze_reminder,
            commands::dismiss_reminder,
            commands::query_tasks,
            commands::search_tasks,
            commands::global_counts,
            commands::list_automations,
            commands::create_automation,
            commands::update_automation,
            commands::delete_automation,
            commands::get_settings,
            commands::save_settings,
            commands::preview_quick_add,
            commands::quick_add,
            commands::hide_quick_add,
            commands::show_main_window,
            commands::add_attachment,
            commands::delete_attachment,
            commands::open_attachment,
            commands::export_data,
            commands::import_data,
            commands::list_backups,
            commands::create_backup_now,
            commands::data_directory,
        ])
        .run(tauri::generate_context!())
        .expect("Tack failed to start");
}

fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let data_dir = handle.path().app_data_dir()?;

    let conn = db::open(&data_dir)?;
    // Files left behind by a crash between the copy and the insert.
    if let Ok(removed) = store::attachments::prune_orphans(&conn, &data_dir)
        && removed > 0
    {
        eprintln!("tack: removed {removed} orphaned attachment file(s)");
    }
    automations::seed_default_rules(&conn)?;

    let start_minimized = store::settings::get_bool(&conn, "startMinimized", false);
    let shortcut = store::settings::get_string(&conn, "quickAddShortcut")?.unwrap_or_default();

    app.manage(AppState::new(conn, data_dir));

    notify::init(&handle);
    if let Err(err) = tray::build(&handle) {
        // A missing status area is a degraded experience, not a fatal one.
        eprintln!("tack: system tray unavailable: {err}");
    }
    #[cfg(desktop)]
    if let Err(err) = quickadd::register_shortcut(&handle, &shortcut) {
        eprintln!("tack: {err}");
    }

    engine::start(handle.clone());

    if !start_minimized
        && let Some(window) = app.get_webview_window("main")
    {
        window.show()?;
    }
    Ok(())
}

/// Keeps the app alive in the background when the main window is closed, so
/// reminders and automations keep running. Quick Add always just hides.
fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    match (window.label(), event) {
        (quickadd::WINDOW_LABEL, WindowEvent::Focused(false)) => {
            // Clicking away from Quick Add dismisses it, like a spotlight panel.
            let _ = window.hide();
        }
        (quickadd::WINDOW_LABEL, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            let _ = window.hide();
        }
        ("main", WindowEvent::CloseRequested { api, .. }) => {
            let app = window.app_handle();
            if app.state::<AppState>().close_to_tray() {
                api.prevent_close();
                let _ = window.hide();
            }
        }
        _ => {}
    }
}
