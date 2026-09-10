// Theme and skin application.
//
// `data-theme` and `data-skin` are orthogonal: the theme is light/dark (and may
// follow the OS), the skin swaps the token set. Skins that only exist in one
// mode pin it, so a cyberpunk board never renders on a white background.

export type ThemePreference = "system" | "light" | "dark";
export type Skin = "apple" | "cyberpunk" | "xp";

/** Skins with no counterpart in the other mode. */
const FIXED_MODE: Partial<Record<Skin, "light" | "dark">> = {
  cyberpunk: "dark",
  xp: "light",
};

const media = window.matchMedia("(prefers-color-scheme: dark)");
let current: { theme: ThemePreference; skin: Skin } = { theme: "system", skin: "apple" };

export function applyTheme(theme: ThemePreference, skin: Skin): void {
  current = { theme, skin };
  const root = document.documentElement;
  const resolved = FIXED_MODE[skin] ?? (theme === "system" ? systemMode() : theme);

  root.setAttribute("data-skin", skin);
  root.setAttribute("data-theme", resolved);
}

export function resolvedMode(): "light" | "dark" {
  return (document.documentElement.getAttribute("data-theme") as "light" | "dark") ?? "light";
}

function systemMode(): "light" | "dark" {
  return media.matches ? "dark" : "light";
}

// Follow the OS while the preference is "system"; a fixed skin ignores it.
media.addEventListener("change", () => {
  if (current.theme === "system") applyTheme(current.theme, current.skin);
});
