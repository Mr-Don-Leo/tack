// DOM helpers.
//
// Everything is built from elements and text nodes — there is no `innerHTML`
// anywhere in the app, so a task title containing markup is displayed, never
// executed. The one exception is `icon()`, which parses a fixed set of SVG
// paths defined in this file and never touches user input.

type Attrs = Record<string, string | number | boolean | null | undefined>;
type Child = Node | string | number | null | undefined | false;

/**
 * Creates an element. `class`, `text` and `on*` keys are handled specially;
 * everything else becomes an attribute.
 */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);

  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === "class") node.className = String(value);
    else if (key === "text") node.textContent = String(value);
    else if (key === "html") throw new Error("raw HTML is not allowed");
    else node.setAttribute(key, String(value));
  }

  append(node, children);
  return node;
}

export function append(parent: Node, children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    parent.appendChild(typeof child === "object" ? child : document.createTextNode(String(child)));
  }
}

export function clear(node: Element): void {
  node.replaceChildren();
}

/** Replaces a node's children in one operation, avoiding intermediate paints. */
export function render(node: Element, ...children: Child[]): void {
  const fragment = document.createDocumentFragment();
  append(fragment, children);
  node.replaceChildren(fragment);
}

export function on<K extends keyof HTMLElementEventMap>(
  node: HTMLElement,
  event: K,
  handler: (ev: HTMLElementEventMap[K]) => void,
  options?: AddEventListenerOptions,
): void {
  node.addEventListener(event, handler as EventListener, options);
}

export interface ButtonOptions {
  class?: string;
  style?: string;
  title?: string;
  disabled?: boolean;
  "aria-label"?: string;
  onClick?: (ev: MouseEvent) => void;
}

/** A button with the shared `.btn` styling. */
export function button(label: Child, options: ButtonOptions = {}): HTMLButtonElement {
  const { onClick, ...rest } = options;
  const node = el("button", { type: "button", class: "btn", ...rest }, label);
  if (onClick) on(node, "click", (ev) => onClick(ev));
  return node;
}

export function iconButton(
  name: IconName,
  title: string,
  onClick: (ev: MouseEvent) => void,
  extraClass = "",
): HTMLButtonElement {
  const node = el(
    "button",
    { type: "button", class: `btn btn-icon ${extraClass}`.trim(), title, "aria-label": title },
    icon(name),
  );
  on(node, "click", onClick);
  return node;
}

// Icons -------------------------------------------------------------------

/** Path data for each icon, on a 24×24 grid, stroked rather than filled. */
const ICONS = {
  plus: "M12 5v14M5 12h14",
  check: "M4.5 12.5l5 5 10-11",
  close: "M6 6l12 12M18 6L6 18",
  chevronDown: "M6 9.5l6 6 6-6",
  chevronLeft: "M14.5 6l-6 6 6 6",
  chevronRight: "M9.5 6l6 6-6 6",
  search: "M11 4a7 7 0 100 14 7 7 0 000-14zM20 20l-4.2-4.2",
  calendar: "M7 3v3M17 3v3M4 9h16M5 5h14a1 1 0 011 1v13a1 1 0 01-1 1H5a1 1 0 01-1-1V6a1 1 0 011-1z",
  clock: "M12 3a9 9 0 100 18 9 9 0 000-18zM12 7v5.2l3.4 2",
  bell: "M18 9a6 6 0 10-12 0c0 5-2 6-2 6h16s-2-1-2-6M13.7 20a2 2 0 01-3.4 0",
  repeat: "M4 10V8a3 3 0 013-3h10l-2.5-2.5M20 14v2a3 3 0 01-3 3H7l2.5 2.5",
  flag: "M5 21V4h11l-1.5 3.5L16 11H5",
  tag: "M3 12.5V5a2 2 0 012-2h7.5L21 11.5 12.5 20 3 12.5zM8 8h.01",
  trash: "M4 7h16M9 7V5a1 1 0 011-1h4a1 1 0 011 1v2M6 7l1 13h10l1-13",
  copy: "M9 9h10v10a2 2 0 01-2 2H9a2 2 0 01-2-2V9zM5 15V5a2 2 0 012-2h10",
  archive: "M3 6h18v4H3zM5 10v9a1 1 0 001 1h12a1 1 0 001-1v-9M10 14h4",
  settings:
    "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 14a1.6 1.6 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.6 1.6 0 00-2.7 1.1V20a2 2 0 11-4 0v-.1A1.6 1.6 0 006 18.8l-.1.1a2 2 0 11-2.8-2.8l.1-.1A1.6 1.6 0 004 13.3H4a2 2 0 110-4h.1A1.6 1.6 0 005.2 6.6l-.1-.1a2 2 0 112.8-2.8l.1.1A1.6 1.6 0 0011 3.7V4a2 2 0 114 0v.1a1.6 1.6 0 002.7 1.1l.1-.1a2 2 0 112.8 2.8l-.1.1a1.6 1.6 0 00-.3 1.8v.1a1.6 1.6 0 001.5 1H20a2 2 0 110 4h-.1a1.6 1.6 0 00-1.5 1z",
  bolt: "M13 2L4.5 13.5H11L10 22l8.5-11.5H12L13 2z",
  board: "M4 4h16v16H4zM9.5 4v16M15 4v16",
  inbox: "M4 13h4l1.5 3h5L16 13h4M4 13l2.5-8h11L20 13v6a1 1 0 01-1 1H5a1 1 0 01-1-1z",
  list: "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01",
  sun: "M12 5V3M12 21v-2M5 12H3M21 12h-2M6.3 6.3L4.9 4.9M19.1 19.1l-1.4-1.4M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4M12 8a4 4 0 100 8 4 4 0 000-8z",
  moon: "M20 13.5A8.5 8.5 0 1110.5 4a6.6 6.6 0 009.5 9.5z",
  paperclip: "M20 11l-8.4 8.4a5 5 0 01-7.1-7.1l9-9a3.5 3.5 0 015 5l-9 9a2 2 0 01-2.8-2.8L14 7",
  note: "M5 4h9l5 5v11a1 1 0 01-1 1H5a1 1 0 01-1-1V5a1 1 0 011-1zM14 4v5h5",
  alert: "M12 8v5M12 16.5h.01M10.3 3.9L2.6 17.4A2 2 0 004.3 20.4h15.4a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z",
  drag: "M9 5h.01M9 12h.01M9 19h.01M15 5h.01M15 12h.01M15 19h.01",
  more: "M12 5h.01M12 12h.01M12 19h.01",
  filter: "M3 5h18l-7 8v5l-4 2v-7L3 5z",
  download: "M12 3v12M7.5 10.5L12 15l4.5-4.5M4 20h16",
  upload: "M12 15V3M7.5 7.5L12 3l4.5 4.5M4 20h16",
  folder: "M3 6a1 1 0 011-1h5l2 2.5h9a1 1 0 011 1V19a1 1 0 01-1 1H4a1 1 0 01-1-1V6z",
  snooze: "M12 3a9 9 0 100 18 9 9 0 000-18zM9 9h6l-6 6h6",
  refresh: "M20 11a8 8 0 10-1.5 6M20 5v6h-6",
} as const;

export type IconName = keyof typeof ICONS;

const SVG_NS = "http://www.w3.org/2000/svg";

/** An inline SVG icon. Path data comes from the table above, never from data. */
export function icon(name: IconName, size = 16, strokeWidth = 1.8): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", String(strokeWidth));
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");

  const path = document.createElementNS(SVG_NS, "path");
  path.setAttribute("d", ICONS[name]);
  svg.appendChild(path);
  return svg;
}

/** Highlights every occurrence of `needle` in `text` using `<mark>` elements. */
export function highlight(text: string, needle: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const query = needle.trim().toLowerCase();
  if (!query) {
    fragment.appendChild(document.createTextNode(text));
    return fragment;
  }

  const haystack = text.toLowerCase();
  let cursor = 0;
  for (;;) {
    const hit = haystack.indexOf(query, cursor);
    if (hit === -1) break;
    if (hit > cursor) fragment.appendChild(document.createTextNode(text.slice(cursor, hit)));
    fragment.appendChild(el("mark", { text: text.slice(hit, hit + query.length) }));
    cursor = hit + query.length;
  }
  fragment.appendChild(document.createTextNode(text.slice(cursor)));
  return fragment;
}

/** Runs `handler` on the next click outside `node`, then unsubscribes. */
export function onClickOutside(node: HTMLElement, handler: () => void): () => void {
  const listener = (ev: MouseEvent) => {
    if (!node.contains(ev.target as Node)) handler();
  };
  // Deferred so the click that opened the panel does not immediately close it.
  const timer = window.setTimeout(() => document.addEventListener("mousedown", listener), 0);
  return () => {
    window.clearTimeout(timer);
    document.removeEventListener("mousedown", listener);
  };
}

/** Focuses `node` and, for text inputs, places the caret at the end. */
export function focusEnd(node: HTMLInputElement | HTMLTextAreaElement): void {
  node.focus();
  const end = node.value.length;
  node.setSelectionRange(end, end);
}
