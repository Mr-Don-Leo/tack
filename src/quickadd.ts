// The Quick Add window.
//
// A single field over a borderless panel: type a line, press Enter, keep
// working. Everything else — parsing, the destination board, the task itself —
// is inferred so capture never costs more than a few seconds.

import { listen } from "@tauri-apps/api/event";

import * as api from "./api";
import { el, icon, on, render } from "./dom";
import { formatDue, PRIORITY_NAMES } from "./format";
import { applyTheme } from "./theme";
import type { Board } from "./types";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/controls.css";
import "./styles/modal.css";
import "./styles/skins.css";

const root = document.getElementById("quickadd");
if (!root) throw new Error("#quickadd is missing from quickadd.html");

document.body.classList.add("quickadd");

const input = el("input", {
  class: "quickadd-input",
  type: "text",
  placeholder: "Add a task — “Fix checkout bug tomorrow 3pm !high #work”",
  "aria-label": "New task",
  autocomplete: "off",
  spellcheck: "false",
});

const hint = el("div", { class: "quickadd-hint" });
const boardChip = el("button", {
  type: "button",
  class: "pill",
  "aria-label": "Destination board — click to change",
});

const panel = el(
  "div",
  { class: "quickadd-panel" },
  icon("bolt", 16),
  input,
  boardChip,
  el("span", { class: "kbd", text: "⏎" }),
);

render(root, panel, hint);

let boards: Board[] = [];
let mainBoardId = "";
/** null means "the Main Board", matching the quick-add default. */
let targetBoardId: string | null = null;
let previewTimer = 0;
let submitting = false;

function boardName(id: string | null): string {
  const board = boards.find((candidate) => candidate.id === (id ?? mainBoardId));
  return board?.name ?? "Main Board";
}

function paintBoardChip(): void {
  render(boardChip, icon("board", 11), boardName(targetBoardId));
}

/** Cycles through boards, so the destination is one click away without a menu. */
on(boardChip, "click", () => {
  if (boards.length === 0) return;
  const current = targetBoardId ?? mainBoardId;
  const index = boards.findIndex((board) => board.id === current);
  const next = boards[(index + 1) % boards.length];
  targetBoardId = next.id === mainBoardId ? null : next.id;
  paintBoardChip();
  input.focus();
});

async function updatePreview(): Promise<void> {
  const text = input.value.trim();
  if (!text) {
    render(hint, el("span", { class: "tertiary", text: "Dates, #labels, @boards and !priority are understood." }));
    return;
  }

  try {
    const parsed = await api.previewQuickAdd(text);
    const chips: Node[] = [el("span", { class: "truncate", text: parsed.title })];

    if (parsed.dueAt) {
      chips.push(
        el("span", { class: "pill pill-accent" }, icon("calendar", 11), formatDue(parsed.dueAt, parsed.dueHasTime)),
      );
    }
    if (parsed.priority > 0) {
      chips.push(el("span", { class: "pill" }, icon("flag", 11), PRIORITY_NAMES[parsed.priority]));
    }
    for (const label of parsed.labels) {
      chips.push(el("span", { class: "pill" }, icon("tag", 11), label));
    }
    if (parsed.recurrence) {
      chips.push(el("span", { class: "pill" }, icon("repeat", 11), "Repeats"));
    }
    render(hint, ...chips);
  } catch {
    render(hint);
  }
}

async function submit(): Promise<void> {
  const text = input.value.trim();
  if (!text || submitting) return;
  submitting = true;

  try {
    await api.quickAdd(text, targetBoardId);
    input.value = "";
    render(hint, el("span", { class: "pill pill-success" }, icon("check", 11), "Added"));
    await api.hideQuickAdd();
  } catch (error) {
    render(
      hint,
      el("span", { class: "pill pill-danger" }, icon("alert", 11), error instanceof Error ? error.message : String(error)),
    );
  } finally {
    submitting = false;
  }
}

on(input, "input", () => {
  window.clearTimeout(previewTimer);
  previewTimer = window.setTimeout(() => void updatePreview(), 140);
});

on(input, "keydown", (ev) => {
  if (ev.key === "Enter") {
    ev.preventDefault();
    void submit();
  }
  if (ev.key === "Escape") {
    ev.preventDefault();
    void api.hideQuickAdd();
  }
  // Shift+Tab opens the full editor for the task about to be created.
  if (ev.key === "Tab" && ev.shiftKey) {
    ev.preventDefault();
    void (async () => {
      const text = input.value.trim();
      if (!text) return;
      const task = await api.quickAdd(text, targetBoardId).catch(() => null);
      input.value = "";
      await api.hideQuickAdd();
      if (task) await api.showMainWindow();
    })();
  }
});

async function start(): Promise<void> {
  try {
    const snapshot = await api.bootstrap();
    boards = snapshot.boards;
    mainBoardId = snapshot.mainBoardId;
    applyTheme(snapshot.settings.theme, snapshot.settings.skin);
  } catch {
    // Quick Add stays usable even if the snapshot fails; the backend will
    // fall back to the Main Board on its own.
  }

  paintBoardChip();
  void updatePreview();

  // Reopening should always start from an empty field.
  await listen("tack://quickadd-open", () => {
    input.value = "";
    targetBoardId = null;
    paintBoardChip();
    void updatePreview();
    window.setTimeout(() => input.focus(), 20);
  });

  input.focus();
}

void start();
