// Card and list-row renderers, shared by the board and the global views.

import { el, icon, on } from "../dom";
import {
  checklistProgress,
  describeRecurrence,
  dueTone,
  formatDue,
  formatFull,
  priorityName,
} from "../format";
import type { Label, Task } from "../types";
import { checkbox } from "./ui/controls";

export interface TaskCardHandlers {
  onOpen: (task: Task) => void;
  onToggleComplete: (task: Task, completed: boolean) => void;
  onContextMenu?: (task: Task, ev: MouseEvent) => void;
}

function labelPill(label: Label): HTMLElement {
  const pill = el("span", { class: "pill pill-label", text: label.name });
  // The label's own colour drives both the fill and the text via color-mix.
  pill.style.setProperty("--label-color", label.color);
  return pill;
}

/** The due-date, checklist, attachment and repeat chips under a title. */
function metaRow(task: Task, className: string): HTMLElement | null {
  const items: Node[] = [];

  if (task.dueAt) {
    const tone = dueTone(task);
    items.push(
      el(
        "span",
        {
          class: `card-meta-item ${tone === "overdue" ? "is-overdue" : tone === "today" ? "is-today" : ""}`.trim(),
          title: formatFull(new Date(task.dueAt)),
        },
        icon(task.dueHasTime ? "clock" : "calendar", 12),
        formatDue(task.dueAt, task.dueHasTime),
      ),
    );
  }

  const progress = checklistProgress(task);
  if (progress.total > 0) {
    items.push(
      el(
        "span",
        { class: "card-meta-item" },
        icon("check", 12),
        `${progress.done}/${progress.total}`,
      ),
    );
  }

  const pendingReminders = task.reminders.filter((reminder) => !reminder.dismissed);
  if (pendingReminders.length > 0) {
    items.push(
      el(
        "span",
        { class: "card-meta-item", title: `${pendingReminders.length} reminder(s)` },
        icon("bell", 12),
        pendingReminders.length > 1 ? String(pendingReminders.length) : "",
      ),
    );
  }

  if (task.recurrence) {
    items.push(
      el(
        "span",
        { class: "card-meta-item", title: describeRecurrence(task.recurrence) },
        icon("repeat", 12),
      ),
    );
  }

  if (task.attachments.length > 0) {
    items.push(
      el("span", { class: "card-meta-item" }, icon("paperclip", 12), String(task.attachments.length)),
    );
  }

  if (task.description.trim() || task.notes.trim()) {
    items.push(el("span", { class: "card-meta-item", title: "Has notes" }, icon("note", 12)));
  }

  if (task.priority > 0) {
    items.push(
      el("span", { class: "card-meta-item", title: `${priorityName(task.priority)} priority` },
        icon("flag", 12), priorityName(task.priority)),
    );
  }

  return items.length ? el("div", { class: className }, ...items) : null;
}

/** A draggable board card. */
export function taskCard(task: Task, handlers: TaskCardHandlers): HTMLElement {
  const card = el("article", {
    class: `card ${task.completedAt ? "is-completed" : ""}`.trim(),
    draggable: "true",
    "data-task-id": task.id,
    "data-priority": String(task.priority),
    tabindex: "0",
    role: "button",
    "aria-label": task.title,
  });

  const body = el(
    "div",
    { class: "card-body" },
    task.labels.length
      ? el("div", { class: "card-labels" }, ...task.labels.map(labelPill))
      : null,
    el("div", { class: "card-title", text: task.title }),
    metaRow(task, "card-meta"),
  );

  card.append(
    checkbox({
      checked: Boolean(task.completedAt),
      round: true,
      ariaLabel: task.completedAt ? "Mark as not done" : "Mark as done",
      onChange: (checked) => handlers.onToggleComplete(task, checked),
    }),
    body,
  );

  on(card, "click", (ev) => {
    // The completion checkbox handles its own clicks.
    if ((ev.target as HTMLElement).closest(".checkbox")) return;
    handlers.onOpen(task);
  });
  on(card, "keydown", (ev) => {
    if (ev.key === "Enter" || ev.key === " ") {
      ev.preventDefault();
      handlers.onOpen(task);
    }
  });
  if (handlers.onContextMenu) {
    on(card, "contextmenu", (ev) => {
      ev.preventDefault();
      handlers.onContextMenu!(task, ev);
    });
  }

  on(card, "dragstart", (ev) => {
    ev.dataTransfer?.setData("text/plain", task.id);
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "move";
    // Deferred: setting the class synchronously cancels the drag image.
    window.setTimeout(() => card.classList.add("is-dragging"), 0);
  });
  on(card, "dragend", () => card.classList.remove("is-dragging"));

  return card;
}

/** A flat row for the global views and search results. */
export function taskRow(
  task: Task,
  handlers: TaskCardHandlers,
  context?: { boardName?: string; listName?: string; extra?: Node },
): HTMLElement {
  const row = el("div", {
    class: `task-row ${task.completedAt ? "is-completed" : ""}`.trim(),
    "data-task-id": task.id,
    tabindex: "0",
    role: "button",
    "aria-label": task.title,
  });

  const meta = metaRow(task, "task-row-meta");
  const where =
    context?.boardName &&
    el(
      "span",
      { class: "card-meta-item" },
      icon("board", 12),
      context.listName ? `${context.boardName} · ${context.listName}` : context.boardName,
    );

  if (where && meta) meta.prepend(where);

  row.append(
    checkbox({
      checked: Boolean(task.completedAt),
      round: true,
      ariaLabel: task.completedAt ? "Mark as not done" : "Mark as done",
      onChange: (checked) => handlers.onToggleComplete(task, checked),
    }),
    el(
      "div",
      { class: "task-row-body" },
      el("div", { class: "task-row-title", text: task.title }),
      context?.extra ?? null,
      meta ?? (where ? el("div", { class: "task-row-meta" }, where) : null),
      task.labels.length
        ? el("div", { class: "card-labels", style: "margin-top:5px" }, ...task.labels.map(labelPill))
        : null,
    ),
  );

  on(row, "click", (ev) => {
    if ((ev.target as HTMLElement).closest(".checkbox")) return;
    handlers.onOpen(task);
  });
  on(row, "keydown", (ev) => {
    if (ev.key === "Enter") handlers.onOpen(task);
  });
  if (handlers.onContextMenu) {
    on(row, "contextmenu", (ev) => {
      ev.preventDefault();
      handlers.onContextMenu!(task, ev);
    });
  }
  return row;
}
