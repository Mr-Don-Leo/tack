//! Native OS notifications.
//!
//! Action buttons are only offered where the platform actually delivers them
//! back to us. On Linux the freedesktop spec gives us Open / Snooze / Complete
//! and a click on the body; macOS and Windows show the same notification
//! without buttons, because neither `mac-notification-sys` nor the WinRT toast
//! path used here routes a button press back into the process. Every fired
//! reminder is also pushed to the UI as an in-app alert, so Snooze and Complete
//! are always one click away regardless of platform.

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// Emitted to the frontend whenever a reminder fires.
pub const REMINDER_FIRED_EVENT: &str = "tack://reminder-fired";

pub struct Notification {
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub reminder_id: Option<String>,
    /// Offer Snooze / Complete buttons where the platform supports them.
    pub actions: bool,
}

/// Shows a notification, honouring the user's notification setting.
///
/// Never blocks the caller: on Linux the wait for a button press happens on a
/// detached thread so the reminder engine's tick stays short.
pub fn send(app: &AppHandle, notification: Notification) {
    let state = app.state::<AppState>();
    if !state.notifications_enabled() {
        return;
    }

    if notification.actions
        && let (Some(task_id), Some(reminder_id)) = (&notification.task_id, &notification.reminder_id)
    {
        let _ = app.emit(
            REMINDER_FIRED_EVENT,
            serde_json::json!({
                "taskId": task_id,
                "reminderId": reminder_id,
                "title": notification.title,
                "body": notification.body,
            }),
        );
    }

    show_native(app, notification);
}

#[cfg(target_os = "linux")]
fn show_native(app: &AppHandle, notification: Notification) {
    let app = app.clone();
    let icon = icon_hint(&app);

    // `wait_for_action` blocks until the user acts or the notification expires.
    std::thread::spawn(move || {
        let mut builder = notify_rust::Notification::new();
        builder
            .appname("Tack")
            .summary(&notification.title)
            .body(&notification.body)
            .icon(&icon)
            .timeout(notify_rust::Timeout::Milliseconds(20_000));

        if notification.actions {
            builder
                // Per the freedesktop spec, "default" is the click-the-body
                // action and is not drawn as a button, so the explicit "open"
                // is what daemons that ignore "default" fall back to.
                .action("default", "Open")
                .action("open", "Open")
                .action("snooze", "Snooze")
                .action("complete", "Complete");
        }

        match builder.show() {
            Ok(handle) => handle.wait_for_action(|action| {
                crate::ops::handle_notification_action(
                    &app,
                    action,
                    notification.task_id.as_deref(),
                    notification.reminder_id.as_deref(),
                );
            }),
            Err(err) => eprintln!("tack: could not show notification: {err}"),
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn show_native(app: &AppHandle, notification: Notification) {
    let icon = icon_hint(app);
    std::thread::spawn(move || {
        let mut builder = notify_rust::Notification::new();
        builder
            .appname("Tack")
            .summary(&notification.title)
            .body(&notification.body);

        #[cfg(target_os = "windows")]
        builder.icon(&icon);
        #[cfg(not(target_os = "windows"))]
        let _ = &icon;

        if let Err(err) = builder.show() {
            eprintln!("tack: could not show notification: {err}");
        }
    });
}

/// A path to the bundled icon when we can find one, otherwise the desktop-entry
/// name, which is what an installed build will match on.
fn icon_hint(app: &AppHandle) -> String {
    app.path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("icons/128x128.png"))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "app.tack.desktop".to_string())
}

/// Registers the app with the macOS notification centre. A no-op elsewhere.
pub fn init(_app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // Without this, notifications are attributed to the terminal that
        // launched the process during development.
        let _ = notify_rust::set_application("app.tack.desktop");
    }
}
