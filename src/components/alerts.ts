// In-app reminder alerts.
//
// Native notifications only carry Snooze and Complete buttons on Linux, so
// every fired reminder also lands here. That keeps both actions one click away
// on Windows and macOS, and gives a place for reminders that fired while the
// window was closed.

import * as api from "../api";
import { button, el, icon, render } from "../dom";
import { formatDue } from "../format";
import { dismissAlert, state } from "../store";
import type { Task } from "../types";
import { guard } from "./toast";

export interface AlertHandlers {
  onOpenTask: (task: Task) => void;
  onChanged: () => void;
}

export function renderAlerts(container: HTMLElement, handlers: AlertHandlers): void {
  const alerts = state.alerts;
  if (alerts.length === 0) {
    render(container);
    return;
  }

  // Newest first, and capped so a long absence does not bury the board.
  const visible = [...alerts].reverse().slice(0, 4);
  render(container, ...visible.map((task) => alertCard(task, handlers)));
}

function alertCard(task: Task, handlers: AlertHandlers): HTMLElement {
  const pending = task.reminders.filter((reminder) => !reminder.dismissed);
  const reminderId = pending[0]?.id ?? null;

  const dismissAll = async () => {
    for (const reminder of pending) {
      await api.dismissReminder(reminder.id);
    }
    dismissAlert(task.id);
  };

  return el(
    "div",
    { class: "alert", role: "alert" },
    el(
      "div",
      { class: "row" },
      icon("bell", 14),
      el("span", { class: "alert-title truncate", text: task.title }),
    ),
    el("div", {
      class: "alert-body",
      text: task.dueAt ? `Due ${formatDue(task.dueAt, task.dueHasTime)}` : "Reminder",
    }),
    el(
      "div",
      { class: "alert-actions" },
      button("Open", {
        class: "btn btn-sm btn-primary",
        onClick: () => {
          void guard(async () => {
            await dismissAll();
            handlers.onOpenTask(task);
            handlers.onChanged();
          });
        },
      }),
      button("Snooze", {
        class: "btn btn-sm",
        onClick: () => {
          if (!reminderId) return;
          void guard(async () => {
            await api.snoozeReminder(reminderId, state.data.settings.snoozeMinutes ?? 10);
            dismissAlert(task.id);
            handlers.onChanged();
          });
        },
      }),
      button("Complete", {
        class: "btn btn-sm",
        onClick: () => {
          void guard(async () => {
            await api.setTaskCompleted(task.id, true);
            dismissAlert(task.id);
            handlers.onChanged();
          });
        },
      }),
      el("span", { class: "spacer" }),
      button("Dismiss", {
        class: "btn btn-ghost btn-sm",
        onClick: () => {
          void guard(async () => {
            await dismissAll();
            handlers.onChanged();
          });
        },
      }),
    ),
  );
}
