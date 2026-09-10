// The task editor.
//
// Edits are saved as they happen rather than behind a Save button: the modal is
// a view onto the task, and closing it should never be able to lose work.

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

import * as api from "../api";
import { button, el, icon, iconButton, on, render } from "../dom";
import {
  PRIORITY_NAMES,
  WEEKDAY_SHORT,
  checklistProgress,
  describeOffset,
  describeRecurrence,
  describeReminder,
  formatBytes,
  formatDay,
  formatRelative,
} from "../format";
import { state } from "../store";
import type { Frequency, Id, Label, List, Recurrence, Task } from "../types";
import { guard, toast } from "./toast";
import { checkbox, dropdown, field } from "./ui/controls";
import { confirmDialog, dueDateControl, openModal } from "./ui/overlays";

export interface TaskModalOptions {
  taskId: Id;
  onChanged: () => void;
}

/** Offsets offered when adding a reminder relative to the due date. */
const REMINDER_PRESETS = [0, 10, 30, 60, 120, 1440, 2880];

export async function openTaskModal(options: TaskModalOptions): Promise<void> {
  let task: Task;
  try {
    task = await api.getTask(options.taskId);
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), "error");
    return;
  }

  let lists: List[] = [];
  let boardLabels: Label[] = [];
  try {
    const view = await api.boardView(task.boardId);
    lists = view.lists;
    boardLabels = view.labels;
  } catch {
    // A board that vanished under us still leaves an editable task.
  }

  const modal = openModal({ title: "Task", onClose: options.onChanged });
  const main = el("div", { class: "task-editor-main" });
  const side = el("div", { class: "task-editor-side" });
  modal.body.appendChild(el("div", { class: "task-editor-grid" }, main, side));

  /** Applies a patch, refreshes the local copy and repaints the editor. */
  const patch = async (changes: Parameters<typeof api.updateTask>[1]) => {
    const updated = await guard(() => api.updateTask(task.id, changes));
    if (updated) {
      task = updated;
      options.onChanged();
      paint();
    }
  };

  const reload = async () => {
    const fresh = await guard(() => api.getTask(task.id));
    if (fresh) {
      task = fresh;
      options.onChanged();
      paint();
    }
  };

  function paint(): void {
    renderHeader();
    renderMain();
    renderSide();
    renderFooter();
  }

  // Header ---------------------------------------------------------------

  function renderHeader(): void {
    const header = modal.root.querySelector(".modal-header");
    if (!header) return;
    const board = state.data.boards.find((candidate) => candidate.id === task.boardId);
    const list = lists.find((candidate) => candidate.id === task.listId);

    const title = el("div", { class: "col", style: "min-width:0;flex:1" },
      el("h2", { class: "truncate", text: task.title }),
      el(
        "div",
        { class: "task-breadcrumb truncate" },
        icon("board", 12),
        el("span", { text: board?.name ?? "Board" }),
        icon("chevronRight", 11),
        el("span", { text: list?.name ?? "Column" }),
      ),
    );

    render(
      header,
      checkbox({
        checked: Boolean(task.completedAt),
        round: true,
        ariaLabel: task.completedAt ? "Mark as not done" : "Mark as done",
        onChange: (checked) => {
          void guard(async () => {
            task = await api.setTaskCompleted(task.id, checked);
            options.onChanged();
            paint();
          });
        },
      }),
      title,
      iconButton("close", "Close", modal.close),
    );
  }

  // Main column ----------------------------------------------------------

  function renderMain(): void {
    const titleInput = el("input", { class: "input input-plain", type: "text", "aria-label": "Task title" });
    titleInput.value = task.title;
    on(titleInput, "blur", () => {
      const next = titleInput.value.trim();
      if (next && next !== task.title) void patch({ title: next });
      else titleInput.value = task.title;
    });
    on(titleInput, "keydown", (ev) => {
      if (ev.key === "Enter") titleInput.blur();
    });

    render(
      main,
      titleInput,
      field({
        label: "Description",
        value: task.description,
        placeholder: "What needs doing?",
        multiline: true,
        rows: 3,
        onCommit: (value) => {
          if (value !== task.description) void patch({ description: value });
        },
      }),
      checklistSection(),
      field({
        label: "Notes",
        value: task.notes,
        placeholder: "Anything else worth keeping with this task",
        multiline: true,
        rows: 3,
        onCommit: (value) => {
          if (value !== task.notes) void patch({ notes: value });
        },
      }),
      attachmentSection(),
      activitySection(),
    );
  }

  function checklistSection(): HTMLElement {
    const progress = checklistProgress(task);
    const items = el("div", { class: "checklist" });

    for (const item of task.checklist) {
      items.appendChild(
        el(
          "div",
          { class: "checklist-item" },
          checkbox({
            checked: item.done,
            label: item.text,
            strike: true,
            onChange: (done) => {
              void guard(async () => {
                await api.updateChecklistItem(item.id, null, done);
                await reload();
              });
            },
          }),
          el("span", { class: "spacer" }),
          iconButton("trash", "Remove item", () => {
            void guard(async () => {
              await api.deleteChecklistItem(item.id);
              await reload();
            });
          }),
        ),
      );
    }

    const input = el("input", {
      class: "input",
      type: "text",
      placeholder: "Add a subtask",
      "aria-label": "Add a subtask",
    });
    on(input, "keydown", (ev) => {
      if (ev.key !== "Enter") return;
      const text = input.value.trim();
      if (!text) return;
      input.value = "";
      void guard(async () => {
        await api.addChecklistItem(task.id, text);
        await reload();
        // Repainting replaces this node, so refocus the fresh one.
        main.querySelector<HTMLInputElement>('input[placeholder="Add a subtask"]')?.focus();
      });
    });

    return el(
      "div",
      { class: "field" },
      el(
        "div",
        { class: "row" },
        el("span", { class: "field-label", text: "Checklist" }),
        el("span", { class: "spacer" }),
        progress.total
          ? el("span", { class: "meta", text: `${progress.done} of ${progress.total}` })
          : null,
      ),
      progress.total
        ? el(
            "div",
            { class: "checklist-progress" },
            (() => {
              const fill = el("div", { class: "checklist-progress-fill" });
              fill.style.width = `${Math.round((progress.done / progress.total) * 100)}%`;
              return fill;
            })(),
          )
        : null,
      items,
      input,
    );
  }

  function attachmentSection(): HTMLElement {
    const rows = task.attachments.map((attachment) =>
      el(
        "div",
        { class: "attachment" },
        icon("paperclip", 14),
        el("span", { class: "attachment-name truncate", text: attachment.name }),
        el("span", { class: "attachment-size", text: formatBytes(attachment.size) }),
        iconButton("upload", "Open", () => void guard(() => api.openAttachment(attachment.id))),
        iconButton("trash", "Remove", () => {
          void guard(async () => {
            await api.deleteAttachment(attachment.id);
            await reload();
          });
        }),
      ),
    );

    return el(
      "div",
      { class: "field" },
      el("span", { class: "field-label", text: "Attachments" }),
      ...rows,
      button("Attach a file", {
        class: "btn btn-sm",
        onClick: () => {
          void (async () => {
            const picked = await openFileDialog({ multiple: false, title: "Attach a file" });
            if (typeof picked !== "string") return;
            await guard(async () => {
              await api.addAttachment(task.id, picked);
              await reload();
            });
          })();
        },
      }),
    );
  }

  function activitySection(): HTMLElement {
    const container = el("div", { class: "field" }, el("span", { class: "field-label", text: "Activity" }));
    void api.taskActivity(task.id).then((entries) => {
      for (const entry of entries.slice(0, 8)) {
        container.appendChild(
          el(
            "div",
            { class: "activity-line" },
            el("span", { class: "activity-time", text: formatRelative(entry.createdAt) }),
            el("span", { text: entry.message }),
          ),
        );
      }
    }).catch(() => {});
    return container;
  }

  // Side column ----------------------------------------------------------

  function renderSide(): void {
    render(
      side,
      lists.length
        ? el(
            "label",
            { class: "field" },
            el("span", { class: "field-label", text: "Column" }),
            dropdown({
              value: task.listId,
              ariaLabel: "Column",
              options: lists.map((list) => ({ value: list.id, label: list.name })),
              onChange: (listId) => {
                void guard(async () => {
                  task = await api.moveTask(task.id, listId, 0);
                  options.onChanged();
                  paint();
                });
              },
            }),
          )
        : null,
      el(
        "label",
        { class: "field" },
        el("span", { class: "field-label", text: "Due" }),
        dueDateControl({
          value: task.dueAt,
          hasTime: task.dueHasTime,
          weekStartsOn: state.data.settings.weekStartsOn ?? 0,
          onChange: (iso, hasTime) => void patch({ dueAt: iso, dueHasTime: hasTime }),
        }),
      ),
      el(
        "label",
        { class: "field" },
        el("span", { class: "field-label", text: "Priority" }),
        dropdown({
          value: task.priority,
          ariaLabel: "Priority",
          options: PRIORITY_NAMES.map((label, value) => ({
            value,
            label,
            color: value === 0 ? undefined : `var(--priority-${value})`,
          })),
          onChange: (priority) => void patch({ priority }),
        }),
      ),
      labelSection(),
      reminderSection(),
      recurrenceSection(),
    );
  }

  function labelSection(): HTMLElement {
    const attached = new Set(task.labels.map((label) => label.id));
    const available = boardLabels.length ? boardLabels : state.data.labels;

    const chips = task.labels.map((label) => {
      const chip = el(
        "span",
        { class: "chip" },
        el("span", { text: label.name }),
        (() => {
          const remove = el("button", { type: "button", class: "chip-remove", "aria-label": `Remove ${label.name}` }, icon("close", 11, 2.4));
          on(remove, "click", () => {
            void guard(async () => {
              task = await api.setTaskLabel(task.id, label.id, false);
              options.onChanged();
              paint();
            });
          });
          return remove;
        })(),
      );
      chip.style.setProperty("--label-color", label.color);
      chip.style.background = `color-mix(in srgb, ${label.color} 16%, transparent)`;
      chip.style.color = label.color;
      return chip;
    });

    const unattached = available.filter((label) => !attached.has(label.id));

    return el(
      "div",
      { class: "field" },
      el("span", { class: "field-label", text: "Labels" }),
      chips.length ? el("div", { class: "chip-row" }, ...chips) : null,
      unattached.length
        ? dropdown({
            value: "" as Id,
            placeholder: "Add a label…",
            ariaLabel: "Add a label",
            options: unattached.map((label) => ({
              value: label.id,
              label: label.name,
              color: label.color,
            })),
            onChange: (labelId) => {
              if (!labelId) return;
              void guard(async () => {
                task = await api.setTaskLabel(task.id, labelId, true);
                options.onChanged();
                paint();
              });
            },
          })
        : null,
    );
  }

  function reminderSection(): HTMLElement {
    const rows = task.reminders.map((reminder) =>
      el(
        "div",
        { class: `reminder-row ${reminder.firedAt ? "is-fired" : ""}`.trim() },
        icon("bell", 13),
        el("span", { class: "truncate", text: describeReminder(reminder) }),
        el("span", { class: "spacer" }),
        iconButton("close", "Remove reminder", () => {
          void guard(async () => {
            await api.deleteReminder(reminder.id);
            await reload();
          });
        }),
      ),
    );

    const presets = REMINDER_PRESETS.map((minutes) => ({
      value: minutes,
      label: minutes === 0 ? "At the due time" : `${describeOffset(minutes)} before`,
    }));

    return el(
      "div",
      { class: "field" },
      el("span", { class: "field-label", text: "Reminders" }),
      ...rows,
      task.dueAt
        ? dropdown({
            value: -1,
            placeholder: "Add a reminder…",
            ariaLabel: "Add a reminder",
            options: presets,
            onChange: (minutes) => {
              if (minutes < 0) return;
              void guard(async () => {
                await api.addRelativeReminder(task.id, minutes);
                await reload();
              });
            },
          })
        : el("span", { class: "meta", text: "Set a due date to add reminders relative to it." }),
      button("Remind me at a specific time…", {
        class: "btn btn-ghost btn-sm",
        onClick: () => void promptAbsoluteReminder(),
      }),
    );
  }

  async function promptAbsoluteReminder(): Promise<void> {
    const picker = openModal({ title: "Remind me at…", width: "sm" });
    let chosen: string | null = null;

    picker.body.appendChild(
      dueDateControl({
        value: task.dueAt,
        hasTime: true,
        weekStartsOn: state.data.settings.weekStartsOn ?? 0,
        label: "Pick a date and time",
        onChange: (iso) => {
          chosen = iso;
        },
      }),
    );
    picker.footer.append(
      el("span", { class: "spacer" }),
      button("Cancel", { class: "btn", onClick: picker.close }),
      button("Add reminder", {
        class: "btn btn-primary",
        onClick: () => {
          if (!chosen) {
            toast("Pick a date first", "error");
            return;
          }
          const at = chosen;
          picker.close();
          void guard(async () => {
            await api.addAbsoluteReminder(task.id, at);
            await reload();
          });
        },
      }),
    );
  }

  function recurrenceSection(): HTMLElement {
    const current = task.recurrence;

    const setRecurrence = (next: Recurrence | null) => void patch({ recurrence: next });

    const freqOptions: Array<{ value: Frequency | "none"; label: string }> = [
      { value: "none", label: "Does not repeat" },
      { value: "daily", label: "Every day" },
      { value: "weekdays", label: "Every weekday" },
      { value: "weekly", label: "Every week" },
      { value: "monthly", label: "Every month" },
      { value: "yearly", label: "Every year" },
    ];

    const controls: Node[] = [
      dropdown({
        value: (current?.freq ?? "none") as Frequency | "none",
        ariaLabel: "Repeat",
        options: freqOptions,
        onChange: (freq) => {
          if (freq === "none") return setRecurrence(null);
          setRecurrence({
            freq,
            interval: current?.interval ?? 1,
            weekdays: freq === "weekly" ? (current?.weekdays ?? []) : [],
            dayOfMonth: null,
            until: null,
            count: null,
            occurrences: 0,
          });
        },
      }),
    ];

    if (current) {
      const interval = el("input", {
        class: "input",
        type: "number",
        min: "1",
        max: "99",
        "aria-label": "Repeat interval",
        style: "width:70px",
      });
      interval.value = String(current.interval);
      on(interval, "change", () => {
        const value = Math.max(1, Math.min(99, Number.parseInt(interval.value, 10) || 1));
        interval.value = String(value);
        setRecurrence({ ...current, interval: value });
      });

      controls.push(
        el("div", { class: "row" }, el("span", { class: "meta", text: "Every" }), interval,
          el("span", { class: "meta", text: unitFor(current.freq, current.interval) })),
      );

      if (current.freq === "weekly") {
        const days = el("div", { class: "chip-row" });
        for (let index = 0; index < 7; index += 1) {
          const active = current.weekdays.includes(index);
          const chip = el("button", {
            type: "button",
            class: `pill ${active ? "pill-accent" : ""}`.trim(),
            text: WEEKDAY_SHORT[index],
            "aria-pressed": String(active),
          });
          on(chip, "click", () => {
            const weekdays = active
              ? current.weekdays.filter((day) => day !== index)
              : [...current.weekdays, index].sort((a, b) => a - b);
            setRecurrence({ ...current, weekdays });
          });
          days.appendChild(chip);
        }
        controls.push(days);
      }

      controls.push(el("span", { class: "meta", text: describeRecurrence(current) }));
    }

    return el("div", { class: "field" }, el("span", { class: "field-label", text: "Repeat" }), ...controls);
  }

  // Footer ---------------------------------------------------------------

  function renderFooter(): void {
    render(
      modal.footer,
      el("span", { class: "meta", text: `Created ${formatDay(new Date(task.createdAt))}` }),
      el("span", { class: "spacer" }),
      button("Duplicate", {
        class: "btn btn-ghost btn-sm",
        onClick: () =>
          void guard(async () => {
            await api.duplicateTask(task.id);
            options.onChanged();
            toast("Task duplicated", "success");
          }),
      }),
      button("Archive", {
        class: "btn btn-ghost btn-sm",
        onClick: () =>
          void guard(async () => {
            await api.updateTask(task.id, { archived: true });
            options.onChanged();
            modal.close();
          }),
      }),
      button("Delete", {
        class: "btn btn-danger btn-sm",
        onClick: () => {
          void (async () => {
            const ok = await confirmDialog({
              title: "Delete this task?",
              message: `“${task.title}” and its checklist and attachments will be removed. This cannot be undone.`,
              confirmLabel: "Delete",
              danger: true,
            });
            if (!ok) return;
            await guard(async () => {
              await api.deleteTask(task.id);
              options.onChanged();
              modal.close();
            });
          })();
        },
      }),
    );
  }

  paint();
}

function unitFor(freq: Frequency, interval: number): string {
  const plural = interval === 1 ? "" : "s";
  switch (freq) {
    case "daily":
      return `day${plural}`;
    case "weekdays":
      return `weekday${plural}`;
    case "weekly":
      return `week${plural}`;
    case "monthly":
      return `month${plural}`;
    case "yearly":
      return `year${plural}`;
  }
}
