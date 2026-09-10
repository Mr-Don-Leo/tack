//! The Quick Add window and its global shortcut.
//!
//! Quick Add is a separate borderless window rather than a mode of the main
//! window, so it can appear over other applications without dragging the whole
//! app forward.

use tauri::{AppHandle, Manager};

use crate::error::{Result, rejected};

pub const WINDOW_LABEL: &str = "quickadd";

/// Shows Quick Add if it is hidden, hides it if it is already up. This is what
/// makes the shortcut feel like a toggle rather than a one-way door.
pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show(app);
    }
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
    // Tell the field to clear itself; reopening should never resume a
    // half-typed task from an hour ago.
    let _ = tauri::Emitter::emit_to(app, WINDOW_LABEL, "tack://quickadd-open", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// Binds `accelerator` to Quick Add, replacing whatever was bound before.
///
/// Returns an error the settings screen can show verbatim when the combination
/// is malformed or already claimed by another application.
#[cfg(desktop)]
pub fn register_shortcut(app: &AppHandle, accelerator: &str) -> Result<()> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let manager = app.global_shortcut();
    let _ = manager.unregister_all();

    if accelerator.trim().is_empty() {
        return Ok(()); // An empty accelerator means "no global shortcut".
    }
    let shortcut: tauri_plugin_global_shortcut::Shortcut = accelerator
        .parse()
        .map_err(|_| rejected(format!("“{accelerator}” is not a valid shortcut")))?;

    manager
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            // Fire on press only; without this the window toggles twice per
            // keystroke and appears not to open at all.
            if event.state == ShortcutState::Pressed {
                toggle(app);
            }
        })
        .map_err(|err| rejected(format!("Could not register {accelerator}: {err}")))?;
    Ok(())
}

#[cfg(not(desktop))]
pub fn register_shortcut(_app: &AppHandle, _accelerator: &str) -> Result<()> {
    Ok(())
}
