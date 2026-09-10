// Custom form controls.
//
// Native `<select>`, `<input type=checkbox>` and `<input type=date>` are drawn
// by the platform — WebKitGTK in particular ignores the page's colours entirely
// and renders an unreadable light popup over a dark board. Everything here is
// built from buttons and divs so all three skins stay coherent, and each
// control carries the ARIA role its native counterpart would have had.

import { el, focusEnd, icon, on, onClickOutside, render } from "../../dom";

// Dropdown ----------------------------------------------------------------

export interface DropdownOption<T> {
  value: T;
  label: string;
  /** Shown under the label in the panel; ignored on the trigger. */
  detail?: string;
  /** A swatch colour rendered as a dot before the label. */
  color?: string;
  disabled?: boolean;
  /** Starts a new titled group above this option. */
  group?: string;
}

export interface DropdownConfig<T> {
  options: DropdownOption<T>[];
  value: T;
  onChange: (value: T) => void;
  placeholder?: string;
  ariaLabel?: string;
  className?: string;
}

/** A dropdown element that can also be updated from outside. */
export type DropdownElement<T> = HTMLElement & { setValue(value: T): void };

export function dropdown<T>(config: DropdownConfig<T>): DropdownElement<T> {
  const root = el("div", { class: `dropdown ${config.className ?? ""}`.trim() });
  const triggerLabel = el("span", { class: "truncate" });
  const trigger = el(
    "button",
    {
      type: "button",
      class: "dropdown-trigger",
      "aria-haspopup": "listbox",
      "aria-expanded": "false",
      "aria-label": config.ariaLabel ?? "",
    },
    triggerLabel,
    el("span", { class: "dropdown-chevron" }, icon("chevronDown", 14)),
  );

  let value = config.value;
  let panel: HTMLElement | null = null;
  let release: (() => void) | null = null;

  const paint = () => {
    const selected = config.options.find((option) => option.value === value);
    render(
      triggerLabel,
      selected?.color ? swatch(selected.color) : null,
      selected?.label ?? config.placeholder ?? "Select…",
    );
    triggerLabel.classList.toggle("tertiary", !selected);
  };

  const close = () => {
    release?.();
    release = null;
    panel?.remove();
    panel = null;
    trigger.setAttribute("aria-expanded", "false");
  };

  const open = () => {
    if (panel) return close();
    panel = el("div", { class: "dropdown-panel", role: "listbox" });

    let lastGroup: string | undefined;
    for (const option of config.options) {
      if (option.group && option.group !== lastGroup) {
        panel.appendChild(el("div", { class: "dropdown-heading", text: option.group }));
        lastGroup = option.group;
      }
      const isSelected = option.value === value;
      const item = el(
        "button",
        {
          type: "button",
          class: "dropdown-item",
          role: "option",
          "aria-selected": String(isSelected),
          disabled: option.disabled,
        },
        option.color ? swatch(option.color) : null,
        el(
          "span",
          { class: "col truncate" },
          el("span", { class: "truncate", text: option.label }),
          option.detail ? el("span", { class: "meta truncate", text: option.detail }) : null,
        ),
        isSelected ? el("span", { class: "dropdown-item-check" }, icon("check", 14)) : null,
      );
      on(item, "click", () => {
        value = option.value;
        paint();
        close();
        config.onChange(option.value);
      });
      panel.appendChild(item);
    }

    root.appendChild(panel);
    trigger.setAttribute("aria-expanded", "true");
    // Flip above the trigger when there is not enough room below it.
    const rect = panel.getBoundingClientRect();
    if (rect.bottom > window.innerHeight - 8) panel.classList.add("dropdown-panel-up");
    release = onClickOutside(root, close);
  };

  on(trigger, "click", open);
  on(trigger, "keydown", (ev) => {
    if (ev.key === "Escape") close();
  });

  root.appendChild(trigger);
  paint();

  return Object.assign(root, {
    setValue(next: T) {
      value = next;
      paint();
    },
  });
}

function swatch(color: string): HTMLElement {
  const dot = el("span", { class: "swatch" });
  dot.style.cssText =
    "width:9px;height:9px;border-radius:999px;flex:none;display:inline-block;margin-right:2px";
  dot.style.background = color;
  return dot;
}

// Checkbox ----------------------------------------------------------------

export interface CheckboxConfig {
  checked: boolean;
  label?: string;
  onChange: (checked: boolean) => void;
  round?: boolean;
  strike?: boolean;
  ariaLabel?: string;
}

export function checkbox(config: CheckboxConfig): HTMLElement {
  const box = el("span", { class: "checkbox-box" }, icon("check", 12, 2.6));
  const root = el(
    "button",
    {
      type: "button",
      class: `checkbox ${config.round ? "checkbox-round" : ""}`.trim(),
      role: "checkbox",
      "aria-checked": String(config.checked),
      "aria-label": config.ariaLabel ?? config.label ?? "Toggle",
    },
    box,
    config.label
      ? el("span", { class: `checkbox-label ${config.strike ? "strike" : ""}`.trim(), text: config.label })
      : null,
  );

  on(root, "click", (ev) => {
    ev.stopPropagation();
    const next = root.getAttribute("aria-checked") !== "true";
    root.setAttribute("aria-checked", String(next));
    config.onChange(next);
  });
  return root;
}

// Switch ------------------------------------------------------------------

export function toggleSwitch(checked: boolean, onChange: (checked: boolean) => void, label = "Toggle"): HTMLElement {
  const root = el(
    "button",
    { type: "button", class: "switch row", role: "switch", "aria-checked": String(checked), "aria-label": label },
    el("span", { class: "switch-knob" }),
  );
  on(root, "click", () => {
    const next = root.getAttribute("aria-checked") !== "true";
    root.setAttribute("aria-checked", String(next));
    onChange(next);
  });
  return root;
}

// Segmented control -------------------------------------------------------

export function segmented<T extends string>(
  options: Array<{ value: T; label: string }>,
  value: T,
  onChange: (value: T) => void,
): HTMLElement {
  const root = el("div", { class: "segmented", role: "tablist" });
  for (const option of options) {
    const item = el(
      "button",
      {
        type: "button",
        class: "segmented-item",
        role: "tab",
        "aria-selected": String(option.value === value),
        text: option.label,
      },
    );
    on(item, "click", () => {
      for (const sibling of root.children) sibling.setAttribute("aria-selected", "false");
      item.setAttribute("aria-selected", "true");
      onChange(option.value);
    });
    root.appendChild(item);
  }
  return root;
}

// Inline text field -------------------------------------------------------

/** A labelled input; `onInput` fires per keystroke, `onCommit` on blur/Enter. */
export function field(config: {
  label?: string;
  value: string;
  placeholder?: string;
  multiline?: boolean;
  rows?: number;
  onCommit?: (value: string) => void;
  onInput?: (value: string) => void;
}): HTMLElement {
  const input = config.multiline
    ? el("textarea", { class: "textarea", rows: config.rows ?? 3, placeholder: config.placeholder ?? "" })
    : el("input", { class: "input", type: "text", placeholder: config.placeholder ?? "" });
  input.value = config.value;

  if (config.onInput) on(input, "input", () => config.onInput!(input.value));
  if (config.onCommit) {
    on(input, "blur", () => config.onCommit!(input.value));
    if (!config.multiline) {
      on(input, "keydown", (ev) => {
        if (ev.key === "Enter") {
          ev.preventDefault();
          input.blur();
        }
      });
    }
  }

  return config.label
    ? el("label", { class: "field" }, el("span", { class: "field-label", text: config.label }), input)
    : input;
}

/** Turns a label into an editable field in place, committing on blur or Enter. */
export function editableText(
  node: HTMLElement,
  value: string,
  onCommit: (value: string) => void,
  className = "input input-sm",
): void {
  const input = el("input", { class: className, type: "text" });
  input.value = value;
  node.replaceWith(input);
  focusEnd(input);

  let settled = false;
  const finish = (commit: boolean) => {
    if (settled) return;
    settled = true;
    input.replaceWith(node);
    const next = input.value.trim();
    if (commit && next && next !== value) onCommit(next);
  };

  on(input, "blur", () => finish(true));
  on(input, "keydown", (ev) => {
    if (ev.key === "Enter") finish(true);
    if (ev.key === "Escape") finish(false);
  });
}
