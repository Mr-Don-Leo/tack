// Transient messages. Errors stay long enough to read; confirmations do not.

import { el, icon, on, render } from "../dom";

type Tone = "info" | "success" | "error";

let stack: HTMLElement | null = null;

function container(): HTMLElement {
  if (!stack) {
    stack = el("div", { class: "toast-stack", role: "status", "aria-live": "polite" });
    document.body.appendChild(stack);
  }
  return stack;
}

export function toast(message: string, tone: Tone = "info"): void {
  const node = el(
    "div",
    { class: `toast toast-${tone}` },
    el("span", { class: "toast-icon" }, icon(tone === "error" ? "alert" : "check", 16)),
    el("span", { class: "selectable", text: message }),
  );

  container().appendChild(node);
  const life = tone === "error" ? 6000 : 2600;

  const dismiss = () => {
    node.classList.add("is-leaving");
    window.setTimeout(() => node.remove(), 200);
  };
  const timer = window.setTimeout(dismiss, life);
  on(node, "click", () => {
    window.clearTimeout(timer);
    dismiss();
  });
}

export const toastError = (error: unknown) =>
  toast(error instanceof Error ? error.message : String(error), "error");

/** Runs `action`, surfacing any failure as a toast instead of a dead click. */
export async function guard<T>(action: () => Promise<T>): Promise<T | undefined> {
  try {
    return await action();
  } catch (error) {
    toastError(error);
    return undefined;
  }
}

/** Empties a container and shows a centred message. */
export function emptyState(target: Element, title: string, body: string): void {
  render(target, el("div", { class: "empty-state" }, el("h3", { text: title }), el("p", { text: body })));
}
