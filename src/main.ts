// Application shell: layout, routing, global shortcuts and backend events.

import { listen } from "@tauri-apps/api/event";

import * as api from "./api";
import { renderAlerts } from "./components/alerts";
import { renderAutomations } from "./components/automations-view";
import { renderBoard } from "./components/board-view";
import { renderGlobalView, renderSearchView, scopeTitle } from "./components/list-views";
import { renderSettings } from "./components/settings-view";
import { renderSidebar } from "./components/sidebar";
import { openTaskModal } from "./components/task-modal";
import { emptyState, guard, toast, toastError } from "./components/toast";
import { openModal } from "./components/ui/overlays";
import { button, el, icon, iconButton, on, render } from "./dom";
import { PRIORITY_NAMES, formatDue } from "./format";
import { load, navigate, pushAlert, refresh, state, subscribe, subscribeRoute } from "./store";
import { applyTheme } from "./theme";
import type { Scope, Task } from "./types";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/controls.css";
import "./styles/layout.css";
import "./styles/board.css";
import "./styles/modal.css";
import "./styles/skins.css";

const root = document.getElementById("app");
if (!root) throw new Error("#app is missing from index.html");

const sidebar = el("aside", { class: "sidebar" });
const topbar = el("header", { class: "topbar" });
const viewport = el("main", { class: "view" });
const alertStack = el("div", { class: "alert-stack", "aria-live": "polite" });

render(root, sidebar, el("div", { class: "main" }, topbar, viewport));
document.body.appendChild(alertStack);

/** Search text lives outside the route so the field keeps focus while typing. */
let searchText = "";
let searchDebounce = 0;

const handlers = {
  onOpenTask: (task: Task) => void openTaskModal({ taskId: task.id, onChanged: onDataChanged }),
  onChanged: onDataChanged,
};

function onDataChanged(): void {
  void refresh().catch(toastError);
}

// Rendering ---------------------------------------------------------------

function paintChrome(): void {
  renderSidebar(sidebar, { onChanged: onDataChanged });
  renderTopbar();
  renderAlerts(alertStack, handlers);
}

function renderTopbar(): void {
  const route = state.route;
  let title = "Tack";
  let subtitle: string | null = null;

  if (route.kind === "board") {
    const board = state.data.boards.find((candidate) => candidate.id === route.boardId);
    title = board?.name ?? "Board";
    subtitle = board?.isMain ? "Your default board" : null;
  } else if (route.kind === "view") {
    title = scopeTitle(route.scope);
  } else if (route.kind === "search") {
    title = "Search";
  } else if (route.kind === "automations") {
    title = "Automations";
  } else {
    title = "Settings";
  }

  const searchInput = el("input", {
    class: "input",
    type: "search",
    placeholder: "Search all boards",
    "aria-label": "Search all boards",
    id: "global-search",
  });
  searchInput.value = searchText;

  on(searchInput, "input", () => {
    searchText = searchInput.value;
    window.clearTimeout(searchDebounce);
    // Debounced so a fast typist does not queue a query per keystroke.
    searchDebounce = window.setTimeout(() => {
      if (searchText.trim()) navigate({ kind: "search", text: searchText });
      else if (state.route.kind === "search") navigate({ kind: "view", scope: "today" });
    }, 180);
  });
  on(searchInput, "keydown", (ev) => {
    if (ev.key === "Escape") {
      searchInput.value = "";
      searchText = "";
      searchInput.blur();
      if (state.route.kind === "search") navigate({ kind: "view", scope: "today" });
    }
  });

  render(
    topbar,
    el(
      "div",
      { class: "topbar-title truncate" },
      el("h1", { class: "truncate", text: title }),
      subtitle ? el("span", { class: "meta", text: subtitle }) : null,
    ),
    el("span", { class: "spacer" }),
    el(
      "div",
      { class: "search-box" },
      el("span", { class: "search-box-icon" }, icon("search", 14)),
      searchInput,
    ),
    button(el("span", { class: "row" }, icon("plus", 15), el("span", { text: "New task" })), {
      class: "btn btn-primary btn-sm",
      onClick: () => createQuickTask(),
    }),
    iconButton("refresh", "Reload", () => void reloadEverything()),
  );
}

async function paintView(): Promise<void> {
  const route = state.route;

  try {
    switch (route.kind) {
      case "board": {
        const view = await api.boardView(route.boardId);
        renderBoard(viewport, view, { onOpenTask: handlers.onOpenTask, onChanged: onDataChanged });
        break;
      }
      case "view":
        await renderGlobalView(viewport, route.scope, handlers);
        break;
      case "search":
        await renderSearchView(viewport, route.text, handlers);
        break;
      case "automations":
        await renderAutomations(viewport, onDataChanged);
        break;
      case "settings":
        await renderSettings(viewport, onDataChanged);
        break;
    }
  } catch (error) {
    // A board deleted in another window should not leave a blank shell.
    emptyState(
      viewport,
      "Could not open this view",
      error instanceof Error ? error.message : String(error),
    );
  }
}

function paintAll(): void {
  paintChrome();
  void paintView();
}

// Task creation -----------------------------------------------------------

/** Inline capture from the top bar; lands on the board in view, else Main. */
function createQuickTask(): void {
  const modal = openModal({ title: "New task", width: "sm" });

  const input = el("input", {
    class: "input",
    type: "text",
    placeholder: "Fix checkout invoice bug tomorrow 3pm !high",
    "aria-label": "Task",
  });
  const preview = el("div", { class: "meta" });

  const updatePreview = async () => {
    const text = input.value.trim();
    if (!text) {
      render(preview);
      return;
    }
    try {
      const parsed = await api.previewQuickAdd(text);
      const parts = [`“${parsed.title}”`];
      if (parsed.dueAt) parts.push(formatDue(parsed.dueAt, parsed.dueHasTime));
      if (parsed.priority) parts.push(`${PRIORITY_NAMES[parsed.priority]} priority`);
      if (parsed.labels.length) parts.push(parsed.labels.map((name) => `#${name}`).join(" "));
      render(preview, parts.join(" · "));
    } catch {
      render(preview);
    }
  };

  let debounce = 0;
  on(input, "input", () => {
    window.clearTimeout(debounce);
    debounce = window.setTimeout(() => void updatePreview(), 140);
  });

  const submit = () => {
    const text = input.value.trim();
    if (!text) return;
    modal.close();
    void guard(async () => {
      const task = await api.quickAdd(text, targetBoardId());
      onDataChanged();
      toast(`Added “${task.title}”`, "success");
    });
  };

  on(input, "keydown", (ev) => {
    if (ev.key === "Enter") {
      ev.preventDefault();
      submit();
    }
  });

  modal.body.append(
    input,
    preview,
    el("p", {
      class: "meta",
      text: "Dates, times, #labels, @boards and !priority are read from the text.",
    }),
  );
  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Add task", { class: "btn btn-primary", onClick: submit }),
  );
  input.focus();
}

/** The board a quick task should land on: the one in view, or the Main Board. */
function targetBoardId(): string | null {
  return state.route.kind === "board" ? state.route.boardId : null;
}

// Startup -----------------------------------------------------------------

async function reloadEverything(): Promise<void> {
  await guard(async () => {
    await load();
    applyTheme(state.data.settings.theme, state.data.settings.skin);
    paintAll();
  });
}

function wireShortcuts(): void {
  on(document.body, "keydown", (ev) => {
    const meta = ev.metaKey || ev.ctrlKey;
    const target = ev.target as HTMLElement | null;
    const typing = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;

    if (meta && ev.key.toLowerCase() === "k") {
      ev.preventDefault();
      document.getElementById("global-search")?.focus();
      return;
    }
    if (meta && ev.key.toLowerCase() === "n") {
      ev.preventDefault();
      createQuickTask();
      return;
    }
    if (meta && ev.key === ",") {
      ev.preventDefault();
      navigate({ kind: "settings" });
      return;
    }
    // A bare "/" focuses search the way it does in most web apps.
    if (!meta && !typing && ev.key === "/") {
      ev.preventDefault();
      document.getElementById("global-search")?.focus();
    }
  });
}

async function wireBackendEvents(): Promise<void> {
  // The engine and the tray both mutate data without the UI knowing.
  await listen("tack://data-changed", () => {
    void refresh().then(() => void paintView()).catch(() => {});
  });

  await listen<string>("tack://open-task", (event) => {
    void openTaskModal({ taskId: event.payload, onChanged: onDataChanged });
  });

  await listen<string>("tack://open-view", (event) => {
    navigate({ kind: "view", scope: event.payload as Scope });
  });

  await listen<{ taskId: string }>("tack://reminder-fired", (event) => {
    void api
      .getTask(event.payload.taskId)
      .then((task) => {
        pushAlert(task);
        renderAlerts(alertStack, handlers);
      })
      .catch(() => {});
  });
}

async function start(): Promise<void> {
  try {
    await load();
  } catch (error) {
    document.body.appendChild(
      el(
        "div",
        { class: "empty-state" },
        el("h3", { text: "Tack could not open its database" }),
        el("p", { class: "selectable", text: error instanceof Error ? error.message : String(error) }),
      ),
    );
    return;
  }

  applyTheme(state.data.settings.theme, state.data.settings.skin);

  // Reopen wherever the user left off, falling back to the Main Board.
  const last = state.data.settings.lastBoardId;
  navigate(
    last && state.data.boards.some((board) => board.id === last)
      ? { kind: "board", boardId: last }
      : { kind: "board", boardId: state.data.mainBoardId },
  );

  subscribe(paintChrome);
  subscribeRoute(paintAll);
  wireShortcuts();
  await wireBackendEvents();

  paintAll();
}

void start();
