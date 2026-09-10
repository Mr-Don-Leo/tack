// Overlays: modals, context menus and the custom date picker.

import { button, el, icon, on, onClickOutside, render } from "../../dom";
import { WEEKDAY_SHORT, formatDay, startOfDay, toTimeValue } from "../../format";
import { iconButton } from "../../dom";

// Modal -------------------------------------------------------------------

export interface ModalHandle {
  root: HTMLElement;
  body: HTMLElement;
  footer: HTMLElement;
  close: () => void;
}

export interface ModalConfig {
  title: string;
  width?: "sm" | "md";
  onClose?: () => void;
  /** Extra header controls, rendered to the right of the title. */
  headerExtra?: Node[];
}

/** Opens a modal over a blurred scrim. Escape and a scrim click both close it. */
export function openModal(config: ModalConfig): ModalHandle {
  const body = el("div", { class: "modal-body" });
  const footer = el("div", { class: "modal-footer" });

  const closeButton = iconButton("close", "Close", () => close());
  const modal = el(
    "div",
    { class: `modal ${config.width === "sm" ? "modal-sm" : ""}`.trim(), role: "dialog", "aria-modal": "true" },
    el(
      "div",
      { class: "modal-header" },
      el("h2", { class: "truncate", text: config.title }),
      el("span", { class: "spacer" }),
      ...(config.headerExtra ?? []),
      closeButton,
    ),
    body,
    footer,
  );
  const scrim = el("div", { class: "scrim" }, modal);

  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    document.removeEventListener("keydown", onKey);
    scrim.remove();
    config.onClose?.();
  };

  const onKey = (ev: KeyboardEvent) => {
    // Let a nested overlay (a dropdown panel, a picker) take Escape first.
    if (ev.key === "Escape" && !document.querySelector(".dropdown-panel, .menu")) {
      ev.preventDefault();
      close();
    }
  };

  on(scrim, "mousedown", (ev) => {
    if (ev.target === scrim) close();
  });
  document.addEventListener("keydown", onKey);
  document.body.appendChild(scrim);

  return { root: modal, body, footer, close };
}

/** A yes/no modal. Resolves true only when the confirming button is pressed. */
export function confirmDialog(config: {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
}): Promise<boolean> {
  return new Promise((resolve) => {
    let answered = false;
    const settle = (value: boolean) => {
      if (answered) return;
      answered = true;
      resolve(value);
    };

    const modal = openModal({
      title: config.title,
      width: "sm",
      onClose: () => settle(false),
    });
    modal.body.appendChild(el("p", { class: "selectable", text: config.message }));
    modal.footer.append(
      el("span", { class: "spacer" }),
      button("Cancel", {
        class: "btn",
        onClick: () => {
          settle(false);
          modal.close();
        },
      }),
      button(config.confirmLabel ?? "Confirm", {
        class: config.danger ? "btn btn-primary btn-confirm-danger" : "btn btn-primary",
        onClick: () => {
          settle(true);
          modal.close();
        },
      }),
    );
  });
}

// Context menu ------------------------------------------------------------

export interface MenuEntry {
  label: string;
  icon?: Parameters<typeof icon>[0];
  danger?: boolean;
  shortcut?: string;
  onSelect: () => void;
}

/** Opens a menu anchored to the pointer or to an element's bottom-left corner. */
export function openMenu(anchor: MouseEvent | HTMLElement, entries: MenuEntry[]): void {
  const menu = el("div", { class: "menu", role: "menu" });
  let release: (() => void) | null = null;

  const close = () => {
    release?.();
    menu.remove();
    document.removeEventListener("keydown", onKey);
  };
  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") close();
  };

  for (const entry of entries) {
    const item = el(
      "button",
      { type: "button", class: `menu-item ${entry.danger ? "menu-item-danger" : ""}`.trim(), role: "menuitem" },
      entry.icon ? icon(entry.icon, 15) : null,
      el("span", { text: entry.label }),
      entry.shortcut ? el("span", { class: "menu-item-shortcut", text: entry.shortcut }) : null,
    );
    on(item, "click", () => {
      close();
      entry.onSelect();
    });
    menu.appendChild(item);
  }

  document.body.appendChild(menu);

  const point =
    anchor instanceof MouseEvent
      ? { x: anchor.clientX, y: anchor.clientY }
      : (() => {
          const rect = anchor.getBoundingClientRect();
          return { x: rect.left, y: rect.bottom + 4 };
        })();

  // Keep the menu on screen when opened near an edge.
  const rect = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(point.x, window.innerWidth - rect.width - 8)}px`;
  menu.style.top = `${Math.min(point.y, window.innerHeight - rect.height - 8)}px`;

  release = onClickOutside(menu, close);
  document.addEventListener("keydown", onKey);
}

// Date picker -------------------------------------------------------------

export interface DatePickerConfig {
  /** Currently selected instant, or null for "no due date". */
  value: string | null;
  hasTime: boolean;
  weekStartsOn: number;
  onChange: (iso: string | null, hasTime: boolean) => void;
}

/**
 * A calendar panel with an optional time field.
 *
 * Written by hand rather than using `<input type="date">`, whose popup is drawn
 * by the platform and ignores the app's theme entirely.
 */
export function datePicker(config: DatePickerConfig): HTMLElement {
  const selected = config.value ? new Date(config.value) : null;
  let cursor = startOfDay(selected ?? new Date());
  cursor.setDate(1);
  let time = config.hasTime && config.value ? toTimeValue(config.value) : "";

  const root = el("div", { class: "datepicker" });
  const monthLabel = el("div", { class: "datepicker-month" });
  const grid = el("div", { class: "datepicker-grid" });

  const timeInput = el("input", {
    class: "input time-input",
    type: "text",
    placeholder: "--:--",
    "aria-label": "Time",
    maxlength: "5",
  });
  timeInput.value = time;

  const commit = (date: Date | null) => {
    if (!date) return config.onChange(null, false);
    const withTime = /^\d{1,2}:\d{2}$/.test(time);
    const combined = new Date(date);
    if (withTime) {
      const [h, m] = time.split(":").map(Number);
      combined.setHours(h, m, 0, 0);
    } else {
      combined.setHours(23, 59, 0, 0);
    }
    config.onChange(combined.toISOString(), withTime);
  };

  const paint = () => {
    monthLabel.textContent = cursor.toLocaleDateString(undefined, { month: "long", year: "numeric" });

    const cells: Node[] = [];
    for (let i = 0; i < 7; i += 1) {
      cells.push(
        el("div", { class: "datepicker-weekday", text: WEEKDAY_SHORT[(config.weekStartsOn + i) % 7][0] }),
      );
    }

    // Back up to the first cell of the week containing the 1st.
    const firstWeekday = (cursor.getDay() + 6) % 7; // 0 = Monday
    const lead = (firstWeekday - config.weekStartsOn + 7) % 7;
    const start = new Date(cursor);
    start.setDate(1 - lead);

    const today = startOfDay(new Date());
    for (let i = 0; i < 42; i += 1) {
      const day = new Date(start);
      day.setDate(start.getDate() + i);
      const isOutside = day.getMonth() !== cursor.getMonth();
      const isSelected = selected !== null && startOfDay(day).getTime() === startOfDay(selected).getTime();

      const cell = el("button", {
        type: "button",
        class: [
          "datepicker-day",
          isOutside ? "is-outside" : "",
          startOfDay(day).getTime() === today.getTime() ? "is-today" : "",
          isSelected ? "is-selected" : "",
        ]
          .filter(Boolean)
          .join(" "),
        text: String(day.getDate()),
      });
      on(cell, "click", () => commit(day));
      cells.push(cell);
    }
    render(grid, ...cells);
  };

  const step = (months: number) => {
    cursor = new Date(cursor.getFullYear(), cursor.getMonth() + months, 1);
    paint();
  };

  on(timeInput, "input", () => {
    time = timeInput.value.trim();
  });

  root.append(
    el(
      "div",
      { class: "datepicker-header" },
      iconButton("chevronLeft", "Previous month", () => step(-1)),
      monthLabel,
      iconButton("chevronRight", "Next month", () => step(1)),
    ),
    grid,
    el(
      "div",
      { class: "datepicker-footer" },
      timeInput,
      el("span", { class: "spacer" }),
      button("Today", {
        class: "btn btn-ghost btn-sm",
        onClick: () => commit(new Date()),
      }),
      button("Clear", {
        class: "btn btn-ghost btn-sm",
        onClick: () => commit(null),
      }),
    ),
  );

  paint();
  return root;
}

/** A trigger that opens the date picker in a floating panel. */
export function dueDateControl(config: DatePickerConfig & { label?: string }): HTMLElement {
  const root = el("div", { class: "dropdown" });
  const labelSpan = el("span", { class: "truncate" });
  const trigger = el(
    "button",
    { type: "button", class: "dropdown-trigger", "aria-haspopup": "dialog", "aria-expanded": "false" },
    icon("calendar", 14),
    labelSpan,
    el("span", { class: "spacer" }),
    el("span", { class: "dropdown-chevron" }, icon("chevronDown", 14)),
  );

  let panel: HTMLElement | null = null;
  let release: (() => void) | null = null;
  let value = config.value;
  let hasTime = config.hasTime;

  const paint = () => {
    labelSpan.textContent = value
      ? `${formatDay(new Date(value))}${hasTime ? `, ${toTimeValue(value)}` : ""}`
      : (config.label ?? "No due date");
    labelSpan.classList.toggle("tertiary", !value);
  };

  const close = () => {
    release?.();
    release = null;
    panel?.remove();
    panel = null;
    trigger.setAttribute("aria-expanded", "false");
  };

  on(trigger, "click", () => {
    if (panel) return close();
    panel = el(
      "div",
      { class: "dropdown-panel" },
      datePicker({
        value,
        hasTime,
        weekStartsOn: config.weekStartsOn,
        onChange: (iso, withTime) => {
          value = iso;
          hasTime = withTime;
          paint();
          close();
          config.onChange(iso, withTime);
        },
      }),
    );
    root.appendChild(panel);
    trigger.setAttribute("aria-expanded", "true");
    if (panel.getBoundingClientRect().bottom > window.innerHeight - 8) {
      panel.classList.add("dropdown-panel-up");
    }
    release = onClickOutside(root, close);
  });

  root.appendChild(trigger);
  paint();
  return root;
}
