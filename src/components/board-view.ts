// The board: columns, drag-and-drop, inline card creation and column editing.

import * as api from "../api";
import { button, el, icon, iconButton, on, render } from "../dom";
import type { BoardView, List, Task } from "../types";
import { guard, toast } from "./toast";
import { taskCard } from "./task-card";
import { editableText } from "./ui/controls";
import { confirmDialog, openMenu, openModal } from "./ui/overlays";
import type { MenuEntry } from "./ui/overlays";

export interface BoardViewHandlers {
  onOpenTask: (task: Task) => void;
  /** Called after any mutation so the shell can refresh counts and the tray. */
  onChanged: () => void;
}

/** Marks where a dragged card would land, so the drop is never a guess. */
const dropLine = () => el("div", { class: "card-drop-line" });

export function renderBoard(
  container: HTMLElement,
  view: BoardView,
  handlers: BoardViewHandlers,
): void {
  const board = el("div", { class: "board" });
  const byList = new Map<string, Task[]>();
  for (const list of view.lists) byList.set(list.id, []);
  for (const task of view.tasks) byList.get(task.listId)?.push(task);

  for (const list of view.lists) {
    board.appendChild(column(list, byList.get(list.id) ?? [], view, handlers));
  }

  const addColumn = el(
    "button",
    { type: "button", class: "add-column row" },
    icon("plus", 15),
    el("span", { text: "Add column" }),
  );
  on(addColumn, "click", () => {
    void guard(async () => {
      await api.createList(view.board.id, "New column");
      handlers.onChanged();
    });
  });
  board.appendChild(addColumn);

  render(container, board);
}

function column(
  list: List,
  tasks: Task[],
  view: BoardView,
  handlers: BoardViewHandlers,
): HTMLElement {
  const overLimit = list.wipLimit !== null && tasks.length > list.wipLimit;
  const root = el("section", {
    class: `column ${overLimit ? "is-over-limit" : ""}`.trim(),
    "data-list-id": list.id,
  });

  // Header ---------------------------------------------------------------
  const name = el("h3", { class: "column-name", text: list.name });
  on(name, "dblclick", () => {
    editableText(name, list.name, (next) => {
      void guard(async () => {
        await api.updateList(list.id, { name: next });
        handlers.onChanged();
      });
    }, "input column-name-input");
  });

  const header = el(
    "header",
    { class: "column-header", draggable: "true" },
    list.isDoneList ? el("span", { class: "column-done-mark", title: "Completes tasks" }, icon("check", 14)) : null,
    name,
    el("span", {
      class: "column-count",
      text: list.wipLimit ? `${tasks.length}/${list.wipLimit}` : String(tasks.length),
    }),
    el("span", { class: "spacer" }),
    iconButton("more", "Column options", (ev) => columnMenu(ev, list, view, handlers)),
  );

  // Reordering columns by dragging their header.
  on(header, "dragstart", (ev) => {
    ev.dataTransfer?.setData("application/x-tack-list", list.id);
    if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "move";
  });

  // Cards ----------------------------------------------------------------
  const cards = el("div", { class: "column-cards" });
  for (const task of tasks) {
    cards.appendChild(taskCard(task, {
      onOpen: handlers.onOpenTask,
      onToggleComplete: (target, completed) => {
        void guard(async () => {
          await api.setTaskCompleted(target.id, completed);
          handlers.onChanged();
        });
      },
      onContextMenu: (target, ev) => taskMenu(ev, target, view, handlers),
    }));
  }

  wireCardDropTarget(cards, root, list, handlers);
  wireColumnDropTarget(root, list, view, handlers);

  // Footer ---------------------------------------------------------------
  const footer = el("div", { class: "column-footer" });
  footer.appendChild(addCardButton(list, footer, handlers));

  root.append(header, cards, footer);
  return root;
}

/** The "Add a task" affordance, which swaps itself for a one-field form. */
function addCardButton(list: List, footer: HTMLElement, handlers: BoardViewHandlers): HTMLElement {
  const trigger = el(
    "button",
    { type: "button", class: "column-add" },
    icon("plus", 15),
    el("span", { text: "Add a task" }),
  );

  on(trigger, "click", () => {
    const input = el("input", {
      class: "input",
      type: "text",
      placeholder: "Task title — try “review specs tomorrow 3pm”",
      "aria-label": "New task title",
    });

    let submitting = false;
    const cancel = () => render(footer, addCardButton(list, footer, handlers));

    const submit = async (keepOpen: boolean) => {
      const text = input.value.trim();
      if (!text || submitting) return;
      submitting = true;
      try {
        // Quick-add parsing here too, so a card typed on the board understands
        // "tomorrow 3pm" exactly like the global Quick Add does.
        await api.quickAdd(text, list.boardId, list.id);
        handlers.onChanged();
        if (keepOpen) {
          input.value = "";
          submitting = false;
          window.setTimeout(() => input.focus(), 0);
        }
      } catch (error) {
        submitting = false;
        toast(error instanceof Error ? error.message : String(error), "error");
      }
    };

    const form = el(
      "div",
      { class: "column-add-form" },
      input,
      el(
        "div",
        { class: "column-add-actions" },
        button("Add", { class: "btn btn-primary btn-sm", onClick: () => void submit(true) }),
        button("Cancel", { class: "btn btn-ghost btn-sm", onClick: cancel }),
      ),
    );

    on(input, "keydown", (ev) => {
      if (ev.key === "Enter") {
        ev.preventDefault();
        void submit(true);
      }
      if (ev.key === "Escape") cancel();
    });

    render(footer, form);
    input.focus();
  });

  return trigger;
}

/** Where a card dropped at `y` should be inserted among `cards`. */
function insertionIndex(cards: HTMLElement, y: number): number {
  const siblings = Array.from(cards.querySelectorAll<HTMLElement>(".card:not(.is-dragging)"));
  for (let i = 0; i < siblings.length; i += 1) {
    const rect = siblings[i].getBoundingClientRect();
    if (y < rect.top + rect.height / 2) return i;
  }
  return siblings.length;
}

function wireCardDropTarget(
  cards: HTMLElement,
  column: HTMLElement,
  list: List,
  handlers: BoardViewHandlers,
): void {
  let marker: HTMLElement | null = null;

  const clearMarker = () => {
    marker?.remove();
    marker = null;
    column.classList.remove("is-drop-target");
  };

  on(cards, "dragover", (ev) => {
    if (ev.dataTransfer?.types.includes("application/x-tack-list")) return;
    ev.preventDefault();
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = "move";
    column.classList.add("is-drop-target");

    const index = insertionIndex(cards, ev.clientY);
    const siblings = Array.from(cards.querySelectorAll<HTMLElement>(".card:not(.is-dragging)"));
    marker ??= dropLine();
    if (index >= siblings.length) cards.appendChild(marker);
    else cards.insertBefore(marker, siblings[index]);
  });

  on(cards, "dragleave", (ev) => {
    if (!cards.contains(ev.relatedTarget as Node)) clearMarker();
  });

  on(cards, "drop", (ev) => {
    const taskId = ev.dataTransfer?.getData("text/plain");
    const index = insertionIndex(cards, ev.clientY);
    clearMarker();
    if (!taskId) return;
    ev.preventDefault();

    void guard(async () => {
      await api.moveTask(taskId, list.id, index);
      handlers.onChanged();
    });
  });
}

function wireColumnDropTarget(
  root: HTMLElement,
  list: List,
  view: BoardView,
  handlers: BoardViewHandlers,
): void {
  on(root, "dragover", (ev) => {
    if (!ev.dataTransfer?.types.includes("application/x-tack-list")) return;
    ev.preventDefault();
    root.classList.add("is-drop-target");
  });
  on(root, "dragleave", (ev) => {
    if (!root.contains(ev.relatedTarget as Node)) root.classList.remove("is-drop-target");
  });
  on(root, "drop", (ev) => {
    const draggedId = ev.dataTransfer?.getData("application/x-tack-list");
    root.classList.remove("is-drop-target");
    if (!draggedId || draggedId === list.id) return;
    ev.preventDefault();

    // Drop before this column, or after it when the pointer is past halfway.
    const rect = root.getBoundingClientRect();
    const target = view.lists.findIndex((candidate) => candidate.id === list.id);
    const index = ev.clientX > rect.left + rect.width / 2 ? target + 1 : target;

    void guard(async () => {
      await api.reorderList(draggedId, Math.max(0, index));
      handlers.onChanged();
    });
  });
}

function columnMenu(
  ev: MouseEvent,
  list: List,
  view: BoardView,
  handlers: BoardViewHandlers,
): void {
  openMenu(ev, [
    {
      label: "Rename",
      icon: "note",
      onSelect: () => {
        const name = document.querySelector<HTMLElement>(
          `[data-list-id="${CSS.escape(list.id)}"] .column-name`,
        );
        if (name) {
          editableText(name, list.name, (next) => {
            void guard(async () => {
              await api.updateList(list.id, { name: next });
              handlers.onChanged();
            });
          }, "input column-name-input");
        }
      },
    },
    {
      label: list.isDoneList ? "Stop completing tasks here" : "Completing tasks here",
      icon: "check",
      onSelect: () => {
        void guard(async () => {
          await api.updateList(list.id, { isDoneList: !list.isDoneList });
          handlers.onChanged();
        });
      },
    },
    {
      label: list.wipLimit ? `Change limit (${list.wipLimit})` : "Set a card limit",
      icon: "filter",
      onSelect: () => promptWipLimit(list, handlers),
    },
    {
      label: "Delete column",
      icon: "trash",
      danger: true,
      onSelect: () => {
        void (async () => {
          const count = view.tasks.filter((task) => task.listId === list.id).length;
          const ok = await confirmDialog({
            title: `Delete “${list.name}”?`,
            message: count
              ? `${count} task${count === 1 ? "" : "s"} in this column will be deleted too. This cannot be undone.`
              : "This cannot be undone.",
            confirmLabel: "Delete",
            danger: true,
          });
          if (!ok) return;
          await guard(async () => {
            await api.deleteList(list.id);
            handlers.onChanged();
          });
        })();
      },
    },
  ]);
}

function promptWipLimit(list: List, handlers: BoardViewHandlers): void {
  const modal = openModal({ title: `Card limit for “${list.name}”`, width: "sm" });

  const input = el("input", {
    class: "input",
    type: "number",
    min: "0",
    placeholder: "No limit",
    "aria-label": "Card limit",
  });
  input.value = list.wipLimit ? String(list.wipLimit) : "";

  modal.body.append(
    el("p", { class: "meta", text: "Tack will refuse to add more cards to this column once the limit is reached. Leave it empty for no limit." }),
    input,
  );
  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Save", {
      class: "btn btn-primary",
      onClick: () => {
        const raw = input.value.trim();
        const limit = raw === "" ? null : Number.parseInt(raw, 10);
        if (limit !== null && (!Number.isFinite(limit) || limit < 0)) {
          input.classList.add("input-invalid");
          return;
        }
        modal.close();
        void guard(async () => {
          await api.updateList(list.id, { wipLimit: limit === 0 ? null : limit });
          handlers.onChanged();
        });
      },
    }),
  );
  input.focus();
}

/** Right-click menu shared by cards and list rows. */
export function taskMenu(
  ev: MouseEvent,
  task: Task,
  view: BoardView | null,
  handlers: BoardViewHandlers,
): void {
  const entries: MenuEntry[] = [
    {
      label: task.completedAt ? "Mark as not done" : "Mark as done",
      icon: "check" as const,
      onSelect: () =>
        void guard(async () => {
          await api.setTaskCompleted(task.id, !task.completedAt);
          handlers.onChanged();
        }),
    },
    {
      label: "Duplicate",
      icon: "copy" as const,
      onSelect: () =>
        void guard(async () => {
          await api.duplicateTask(task.id);
          handlers.onChanged();
        }),
    },
    {
      label: "Archive",
      icon: "archive" as const,
      onSelect: () =>
        void guard(async () => {
          await api.updateTask(task.id, { archived: true });
          handlers.onChanged();
        }),
    },
    {
      label: "Delete",
      icon: "trash" as const,
      danger: true,
      onSelect: () => {
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
            handlers.onChanged();
          });
        })();
      },
    },
  ];

  // Moving between columns is only meaningful with a board in view.
  if (view && view.lists.length > 1) {
    entries.splice(1, 0, {
      label: "Move to…",
      icon: "board" as const,
      onSelect: () =>
        openMenu(
          ev,
          view.lists
            .filter((list) => list.id !== task.listId)
            .map((list) => ({
              label: list.name,
              onSelect: () =>
                void guard(async () => {
                  await api.moveTask(task.id, list.id, 0);
                  handlers.onChanged();
                }),
            })),
        ),
    });
  }

  openMenu(ev, entries);
}
