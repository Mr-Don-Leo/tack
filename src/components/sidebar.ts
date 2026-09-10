// Sidebar: global views, the board list, and the automations/settings links.

import logoUrl from "../../assets/logo-small.png";

import * as api from "../api";
import { button, el, icon, iconButton, on, render } from "../dom";
import type { IconName } from "../dom";
import { navigate, state } from "../store";
import type { Board, GlobalCounts, Scope } from "../types";
import { guard } from "./toast";
import { editableText } from "./ui/controls";
import { confirmDialog, openMenu, openModal } from "./ui/overlays";
import type { MenuEntry } from "./ui/overlays";

const VIEWS: Array<{ scope: Scope; label: string; icon: IconName; count: (c: GlobalCounts) => number }> = [
  { scope: "today", label: "Today", icon: "sun", count: (c) => c.today },
  { scope: "upcoming", label: "Upcoming", icon: "calendar", count: (c) => c.upcoming },
  { scope: "overdue", label: "Overdue", icon: "alert", count: (c) => c.overdue },
  { scope: "nodue", label: "No due date", icon: "inbox", count: (c) => c.noDueDate },
  { scope: "all", label: "All tasks", icon: "list", count: (c) => c.all },
  { scope: "completed", label: "Completed", icon: "check", count: (c) => c.completed },
];

export interface SidebarHandlers {
  onChanged: () => void;
}

export function renderSidebar(container: HTMLElement, handlers: SidebarHandlers): void {
  const { boards, counts } = state.data;
  const route = state.route;

  const viewItems = VIEWS.map((view) => {
    const value = view.count(counts);
    return navItem({
      label: view.label,
      icon: view.icon,
      selected: route.kind === "view" && route.scope === view.scope,
      badge: value > 0 ? value : null,
      badgeTone: view.scope === "overdue" && value > 0 ? "danger" : "neutral",
      onSelect: () => navigate({ kind: "view", scope: view.scope }),
    });
  });

  const boardItems = boards.map((board) => boardNavItem(board, handlers));

  render(
    container,
    el(
      "div",
      { class: "sidebar-header" },
      el("img", { class: "sidebar-mark", src: logoUrl, alt: "", width: "24", height: "24" }),
      el("span", { class: "sidebar-title", text: "Tack" }),
    ),
    el(
      "div",
      { class: "sidebar-scroll" },
      el(
        "nav",
        { class: "sidebar-section", "aria-label": "Views" },
        el("div", { class: "sidebar-section-header" }, el("span", { class: "sidebar-section-title", text: "Views" })),
        ...viewItems,
      ),
      el(
        "nav",
        { class: "sidebar-section", "aria-label": "Boards" },
        el(
          "div",
          { class: "sidebar-section-header" },
          el("span", { class: "sidebar-section-title", text: "Boards" }),
          el("span", { class: "spacer" }),
          iconButton("plus", "New board", () => void promptNewBoard(handlers), "btn-sm"),
        ),
        ...boardItems,
      ),
      el(
        "nav",
        { class: "sidebar-section", "aria-label": "More" },
        navItem({
          label: "Automations",
          icon: "bolt",
          selected: route.kind === "automations",
          onSelect: () => navigate({ kind: "automations" }),
        }),
        navItem({
          label: "Settings",
          icon: "settings",
          selected: route.kind === "settings",
          onSelect: () => navigate({ kind: "settings" }),
        }),
      ),
    ),
  );
}

function navItem(config: {
  label: string;
  icon: IconName;
  selected: boolean;
  badge?: number | null;
  badgeTone?: "neutral" | "danger";
  glyph?: string | null;
  onSelect: () => void;
}): HTMLElement {
  const node = el(
    "button",
    {
      type: "button",
      class: `nav-item ${config.selected ? "is-selected" : ""}`.trim(),
      "aria-current": config.selected ? "page" : null,
    },
    el("span", { class: "nav-item-icon" }, config.glyph ? config.glyph : icon(config.icon, 15)),
    el("span", { class: "nav-item-label", text: config.label }),
    config.badge
      ? el("span", {
          class: `badge ${config.badgeTone === "danger" ? "badge-danger" : ""}`.trim(),
          text: String(config.badge),
        })
      : null,
  );
  on(node, "click", config.onSelect);
  return node;
}

function boardNavItem(board: Board, handlers: SidebarHandlers): HTMLElement {
  const route = state.route;
  const node = navItem({
    label: board.name,
    icon: board.isMain ? "inbox" : "board",
    glyph: board.icon,
    selected: route.kind === "board" && route.boardId === board.id,
    onSelect: () => navigate({ kind: "board", boardId: board.id }),
  });

  // The Main Board is pinned at the top and cannot be reordered away from it.
  if (!board.isMain) {
    node.setAttribute("draggable", "true");
    on(node, "dragstart", (ev) => {
      ev.dataTransfer?.setData("application/x-tack-board", board.id);
      node.classList.add("is-dragging");
    });
    on(node, "dragend", () => node.classList.remove("is-dragging"));
    on(node, "dragover", (ev) => {
      if (!ev.dataTransfer?.types.includes("application/x-tack-board")) return;
      ev.preventDefault();
      node.classList.add("is-drop-target");
    });
    on(node, "dragleave", () => node.classList.remove("is-drop-target"));
    on(node, "drop", (ev) => {
      node.classList.remove("is-drop-target");
      const draggedId = ev.dataTransfer?.getData("application/x-tack-board");
      if (!draggedId || draggedId === board.id) return;
      ev.preventDefault();
      // Boards are indexed among the non-main boards only.
      const target = state.data.boards.filter((candidate) => !candidate.isMain)
        .findIndex((candidate) => candidate.id === board.id);
      void guard(async () => {
        await api.reorderBoard(draggedId, Math.max(0, target));
        handlers.onChanged();
      });
    });
  }

  on(node, "contextmenu", (ev) => {
    ev.preventDefault();
    boardMenu(ev, board, node, handlers);
  });
  return node;
}

function boardMenu(ev: MouseEvent, board: Board, node: HTMLElement, handlers: SidebarHandlers): void {
  const entries: MenuEntry[] = [
    {
      label: "Rename",
      icon: "note" as const,
      onSelect: () => {
        const label = node.querySelector<HTMLElement>(".nav-item-label");
        if (!label) return;
        editableText(label, board.name, (name) => {
          void guard(async () => {
            await api.updateBoard(board.id, { name });
            handlers.onChanged();
          });
        }, "input");
      },
    },
    {
      label: "Change icon…",
      icon: "tag" as const,
      onSelect: () => void promptBoardIcon(board, handlers),
    },
  ];

  // Archiving and deleting are what make the Main Board special; hide both.
  if (!board.isMain) {
    entries.push(
      {
        label: "Archive",
        icon: "archive" as const,
        onSelect: () =>
          void guard(async () => {
            await api.updateBoard(board.id, { archived: true });
            handlers.onChanged();
          }),
      },
      {
        label: "Delete board",
        icon: "trash" as const,
        danger: true,
        onSelect: () => {
          void (async () => {
            const ok = await confirmDialog({
              title: `Delete “${board.name}”?`,
              message: "Every column and task on this board will be deleted. This cannot be undone.",
              confirmLabel: "Delete board",
              danger: true,
            });
            if (!ok) return;
            await guard(async () => {
              await api.deleteBoard(board.id);
              navigate({ kind: "board", boardId: state.data.mainBoardId });
              handlers.onChanged();
            });
          })();
        },
      },
    );
  }

  openMenu(ev, entries);
}

async function promptNewBoard(handlers: SidebarHandlers): Promise<void> {
  const modal = openModal({ title: "New board", width: "sm" });
  const name = el("input", { class: "input", type: "text", placeholder: "Board name", "aria-label": "Board name" });
  const glyph = el("input", { class: "input", type: "text", placeholder: "📋", maxlength: "4", "aria-label": "Board icon" });

  modal.body.append(
    el("label", { class: "field" }, el("span", { class: "field-label", text: "Name" }), name),
    el("label", { class: "field" }, el("span", { class: "field-label", text: "Icon (optional)" }), glyph),
  );

  const submit = () => {
    const value = name.value.trim();
    if (!value) {
      name.classList.add("input-invalid");
      return;
    }
    modal.close();
    void guard(async () => {
      const board = await api.createBoard(value, null, glyph.value.trim() || null);
      handlers.onChanged();
      navigate({ kind: "board", boardId: board.id });
    });
  };

  on(name, "keydown", (ev) => {
    if (ev.key === "Enter") submit();
  });

  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Create board", { class: "btn btn-primary", onClick: submit }),
  );
  name.focus();
}

async function promptBoardIcon(board: Board, handlers: SidebarHandlers): Promise<void> {
  const modal = openModal({ title: `Icon for “${board.name}”`, width: "sm" });
  const input = el("input", { class: "input", type: "text", maxlength: "4", "aria-label": "Board icon" });
  input.value = board.icon ?? "";

  modal.body.append(
    el("p", { class: "meta", text: "Paste any emoji, or leave it empty to use the default." }),
    input,
  );
  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Save", {
      class: "btn btn-primary",
      onClick: () => {
        const value = input.value.trim();
        modal.close();
        void guard(async () => {
          await api.updateBoard(board.id, { icon: value || null });
          handlers.onChanged();
        });
      },
    }),
  );
  input.focus();
}
