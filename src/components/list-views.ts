// The global task views (Today, Upcoming, Overdue, …) and search results.
//
// Both are the same list with a different query, so filters behave identically
// whether the user arrived from the sidebar or from the search field.

import * as api from "../api";
import { el, highlight, render } from "../dom";
import { groupByDue } from "../format";
import { state } from "../store";
import type { Scope, SearchHit, Task, TaskQuery } from "../types";
import { taskMenu } from "./board-view";
import { taskRow } from "./task-card";
import { emptyState, guard } from "./toast";
import { dropdown } from "./ui/controls";

export interface ListViewHandlers {
  onOpenTask: (task: Task) => void;
  onChanged: () => void;
}

/** Filters that persist while the user moves between views. */
interface Filters {
  boardId: string;
  labelId: string;
  minPriority: number;
}

const filters: Filters = { boardId: "", labelId: "", minPriority: 0 };

const SCOPE_TITLES: Record<Scope, { title: string; empty: string }> = {
  today: { title: "Today", empty: "Nothing is due today. Enjoy it." },
  upcoming: { title: "Upcoming", empty: "Nothing scheduled beyond today." },
  overdue: { title: "Overdue", empty: "Nothing is overdue." },
  completed: { title: "Completed", empty: "No completed tasks yet." },
  all: { title: "All tasks", empty: "No open tasks anywhere." },
  nodue: { title: "No due date", empty: "Every task has a due date." },
};

export function scopeTitle(scope: Scope): string {
  return SCOPE_TITLES[scope].title;
}

export async function renderGlobalView(
  container: HTMLElement,
  scope: Scope,
  handlers: ListViewHandlers,
): Promise<void> {
  const query: TaskQuery = { scope, ...appliedFilters() };
  const tasks = await guard(() => api.queryTasks(query));
  if (!tasks) return;

  const view = el("div", { class: "view-padded" });
  view.appendChild(filterBar(() => void renderGlobalView(container, scope, handlers)));

  if (tasks.length === 0) {
    const empty = el("div");
    view.appendChild(empty);
    render(container, view);
    emptyState(empty, SCOPE_TITLES[scope].title, SCOPE_TITLES[scope].empty);
    return;
  }

  // Completed work reads better newest-first than grouped by a due date that
  // has already passed.
  if (scope === "completed" || scope === "nodue") {
    view.appendChild(taskListBlock(tasks, handlers));
  } else {
    for (const [heading, group] of groupByDue(tasks)) {
      view.append(
        el(
          "div",
          { class: "group-heading" },
          el("h3", { text: heading }),
          el("span", { class: "meta", text: String(group.length) }),
        ),
        taskListBlock(group, handlers),
      );
    }
  }

  render(container, view);
}

export async function renderSearchView(
  container: HTMLElement,
  text: string,
  handlers: ListViewHandlers,
): Promise<void> {
  const trimmed = text.trim();
  const view = el("div", { class: "view-padded" });
  view.appendChild(filterBar(() => void renderSearchView(container, text, handlers)));

  if (!trimmed) {
    const empty = el("div");
    view.appendChild(empty);
    render(container, view);
    emptyState(empty, "Search", "Type to search titles, descriptions, notes and checklist items.");
    return;
  }

  const hits = await guard(() =>
    api.searchTasks({ text: trimmed, includeArchived: true, ...appliedFilters() }),
  );
  if (!hits) return;

  view.append(
    el(
      "div",
      { class: "group-heading" },
      el("h3", { text: `${hits.length} result${hits.length === 1 ? "" : "s"} for “${trimmed}”` }),
    ),
  );

  if (hits.length === 0) {
    const empty = el("div");
    view.appendChild(empty);
    render(container, view);
    emptyState(empty, "No matches", "Try a shorter phrase, or clear the filters above.");
    return;
  }

  view.appendChild(searchResults(hits, trimmed, handlers));
  render(container, view);
}

function appliedFilters(): Partial<TaskQuery> {
  return {
    boardIds: filters.boardId ? [filters.boardId] : [],
    labelIds: filters.labelId ? [filters.labelId] : [],
    minPriority: filters.minPriority || null,
  };
}

function filterBar(rerender: () => void): HTMLElement {
  const { boards, labels } = state.data;

  return el(
    "div",
    { class: "filter-bar" },
    dropdown({
      value: filters.boardId,
      ariaLabel: "Filter by board",
      options: [
        { value: "", label: "All boards" },
        ...boards.map((board) => ({ value: board.id, label: board.name })),
      ],
      onChange: (boardId) => {
        filters.boardId = boardId;
        rerender();
      },
    }),
    dropdown({
      value: filters.labelId,
      ariaLabel: "Filter by label",
      options: [
        { value: "", label: "All labels" },
        ...labels.map((label) => ({ value: label.id, label: label.name, color: label.color })),
      ],
      onChange: (labelId) => {
        filters.labelId = labelId;
        rerender();
      },
    }),
    dropdown({
      value: filters.minPriority,
      ariaLabel: "Filter by priority",
      options: [
        { value: 0, label: "Any priority" },
        { value: 1, label: "Low and above" },
        { value: 2, label: "Medium and above" },
        { value: 3, label: "High and above" },
        { value: 4, label: "Urgent only" },
      ],
      onChange: (minPriority) => {
        filters.minPriority = minPriority;
        rerender();
      },
    }),
  );
}

function rowHandlers(handlers: ListViewHandlers) {
  return {
    onOpen: handlers.onOpenTask,
    onToggleComplete: (task: Task, completed: boolean) => {
      void guard(async () => {
        await api.setTaskCompleted(task.id, completed);
        handlers.onChanged();
      });
    },
    onContextMenu: (task: Task, ev: MouseEvent) =>
      taskMenu(ev, task, null, { onOpenTask: handlers.onOpenTask, onChanged: handlers.onChanged }),
  };
}

function taskListBlock(tasks: Task[], handlers: ListViewHandlers): HTMLElement {
  const block = el("div", { class: "task-list" });
  const shared = rowHandlers(handlers);
  const boardNames = new Map(state.data.boards.map((board) => [board.id, board.name]));

  for (const task of tasks) {
    block.appendChild(taskRow(task, shared, { boardName: boardNames.get(task.boardId) }));
  }
  return block;
}

function searchResults(hits: SearchHit[], needle: string, handlers: ListViewHandlers): HTMLElement {
  const block = el("div", { class: "task-list" });
  const shared = rowHandlers(handlers);

  for (const hit of hits) {
    const snippet = hit.snippet
      ? (() => {
          const node = el("div", { class: "search-snippet" });
          node.appendChild(highlight(hit.snippet, needle));
          return node;
        })()
      : undefined;

    block.appendChild(
      taskRow(hit.task, shared, {
        boardName: hit.boardName,
        listName: hit.listName,
        extra: snippet,
      }),
    );
  }
  return block;
}
