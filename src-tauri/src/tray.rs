//! System tray / menu-bar presence.
//!
//! The menu is rebuilt whenever the data changes so "Today" and "Upcoming"
//! always show real tasks rather than a static placeholder. Rebuilding takes
//! the database lock, so it must never be called from a context that already
//! holds it.

use chrono::{Local, Utc};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::error::Result;
use crate::models::{Task, TaskQuery};
use crate::state::AppState;
use crate::store::query;
use crate::util::parse_ts;

pub const TRAY_ID: &str = "tack-tray";
/// Asks the UI to switch to one of the global views.
pub const OPEN_VIEW_EVENT: &str = "tack://open-view";

/// How many tasks to list under each section before it becomes a wall of text.
const MAX_ITEMS: usize = 8;

/// Monochrome pin, tinted by the OS on macOS and drawn as-is elsewhere.
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

pub fn build(app: &AppHandle) -> Result<()> {
    let menu = build_menu(app)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::image::Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Tack")
        .menu(&menu)
        // On Windows and Linux a left click should open the app; macOS reserves
        // the left click for the menu, which is the platform convention.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && !cfg!(target_os = "macos")
            {
                crate::ops::show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuilds the tray menu in place. Does nothing when there is no tray, which
/// is the case on desktops without a status-area implementation.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(err) => eprintln!("tack: could not rebuild the tray menu: {err}"),
    }
}

fn build_menu(app: &AppHandle) -> Result<Menu<Wry>> {
    let (today, upcoming, counts) = {
        let state = app.state::<AppState>();
        let conn = state.db();
        let counts = query::counts(&conn)?;
        let today = query::tasks(
            &conn,
            &TaskQuery {
                scope: Some("today".into()),
                limit: Some(MAX_ITEMS as i64),
                ..Default::default()
            },
        )?;
        let upcoming = query::upcoming_for_tray(&conn, 7, MAX_ITEMS as i64)?;
        (today, upcoming, counts)
    };

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "quick-add", "Add Task…", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    menu.append(&task_section(
        app,
        "today",
        &format!("Today ({})", counts.today),
        &today,
        "Nothing due today",
    )?)?;
    menu.append(&task_section(
        app,
        "upcoming",
        &format!("Upcoming ({})", counts.upcoming),
        &upcoming,
        "Nothing coming up",
    )?)?;

    if counts.overdue > 0 {
        menu.append(&MenuItem::with_id(
            app,
            "view:overdue",
            format!("Overdue ({})", counts.overdue),
            true,
            None::<&str>,
        )?)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "open", "Open Tack", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Tack", true, None::<&str>)?)?;
    Ok(menu)
}

/// A submenu listing tasks, with a "Show all…" entry so the section is useful
/// even when the list is empty.
fn task_section(
    app: &AppHandle,
    scope: &str,
    label: &str,
    tasks: &[Task],
    empty_text: &str,
) -> Result<Submenu<Wry>> {
    let submenu = Submenu::new(app, label, true)?;

    if tasks.is_empty() {
        submenu.append(&MenuItem::with_id(
            app,
            format!("empty:{scope}"),
            empty_text,
            false,
            None::<&str>,
        )?)?;
    } else {
        for task in tasks {
            submenu.append(&MenuItem::with_id(
                app,
                format!("task:{}", task.id),
                menu_label(task),
                true,
                None::<&str>,
            )?)?;
        }
    }
    submenu.append(&PredefinedMenuItem::separator(app)?)?;
    submenu.append(&MenuItem::with_id(app, format!("view:{scope}"), "Show all…", true, None::<&str>)?)?;
    Ok(submenu)
}

/// Task title with its due time, truncated so one long title cannot stretch the
/// menu across the screen.
fn menu_label(task: &Task) -> String {
    let title: String = if task.title.chars().count() > 44 {
        format!("{}…", task.title.chars().take(43).collect::<String>())
    } else {
        task.title.clone()
    };

    match task.due_at.as_deref().and_then(parse_ts) {
        Some(due) if task.due_has_time => {
            let overdue = if due < Utc::now() { "• " } else { "" };
            format!("{overdue}{title}  ·  {}", due.with_timezone(&Local).format("%H:%M"))
        }
        _ => title,
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "open" => crate::ops::show_main_window(app),
        "quick-add" => crate::quickadd::toggle(app),
        _ if id.starts_with("task:") => {
            crate::ops::open_task(app, id.trim_start_matches("task:"));
        }
        _ if id.starts_with("view:") => {
            crate::ops::show_main_window(app);
            let _ = app.emit(OPEN_VIEW_EVENT, id.trim_start_matches("view:").to_string());
        }
        _ => {}
    }
}
