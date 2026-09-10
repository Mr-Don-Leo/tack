// Settings: appearance, reminders, quick add, labels, and data management.

import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import * as api from "../api";
import { button, el, icon, on, render } from "../dom";
import { formatBytes, formatRelative } from "../format";
import { setSettings, state } from "../store";
import { applyTheme } from "../theme";
import type { Label, Settings } from "../types";
import { guard, toast } from "./toast";
import { dropdown, toggleSwitch } from "./ui/controls";
import { confirmDialog, openModal } from "./ui/overlays";

const SHORTCUT_PATTERN = /^(?:(?:Control|Command|CommandOrControl|Alt|Option|Shift|Super)\+)+[A-Za-z0-9]+$|^(?:(?:Control|Command|CommandOrControl|Alt|Option|Shift|Super)\+)+(?:Space|Enter|Tab|Backspace|Escape|Up|Down|Left|Right|F\d{1,2})$/;

export async function renderSettings(container: HTMLElement, onChanged: () => void): Promise<void> {
  const settings = state.data.settings;

  const save = async (values: Partial<Settings>) => {
    const saved = await guard(() => api.saveSettings(values));
    if (!saved) return false;
    setSettings(saved);
    applyTheme(saved.theme, saved.skin);
    onChanged();
    return true;
  };

  const view = el(
    "div",
    { class: "view-padded view-narrow" },
    el("h1", { text: "Settings", style: "margin-bottom:16px" }),
    appearancePanel(settings, save),
    behaviourPanel(settings, save),
    remindersPanel(settings, save),
    labelsPanel(onChanged),
    await dataPanel(onChanged),
  );

  render(container, view);
}

function panel(title: string, description: string, ...rows: Node[]): HTMLElement {
  return el(
    "section",
    { class: "panel" },
    el("div", { class: "panel-header" }, el("h2", { text: title }), el("p", { text: description })),
    el("div", { class: "panel-body" }, ...rows),
  );
}

function settingRow(title: string, help: string, control: Node): HTMLElement {
  return el(
    "div",
    { class: "setting-row" },
    el(
      "div",
      { class: "setting-row-text" },
      el("div", { class: "setting-row-title", text: title }),
      el("div", { class: "setting-row-help", text: help }),
    ),
    el("div", { class: "setting-row-control" }, control),
  );
}

type Save = (values: Partial<Settings>) => Promise<boolean>;

function appearancePanel(settings: Settings, save: Save): HTMLElement {
  return panel(
    "Appearance",
    "Themes follow the system by default. Skins that only exist in one mode pin it.",
    settingRow(
      "Skin",
      "Cyberpunk is always dark; XP is always light.",
      dropdown({
        value: settings.skin,
        ariaLabel: "Skin",
        options: [
          { value: "apple" as const, label: "Apple", detail: "Light and dark" },
          { value: "cyberpunk" as const, label: "Cyberpunk", detail: "Always dark" },
          { value: "xp" as const, label: "Windows XP", detail: "Always light" },
        ],
        onChange: (skin) => void save({ skin }),
      }),
    ),
    settingRow(
      "Theme",
      "Only applies to skins that support both modes.",
      dropdown({
        value: settings.theme,
        ariaLabel: "Theme",
        options: [
          { value: "system" as const, label: "Follow the system" },
          { value: "light" as const, label: "Light" },
          { value: "dark" as const, label: "Dark" },
        ],
        onChange: (theme) => void save({ theme }),
      }),
    ),
    settingRow(
      "Week starts on",
      "Used by the date picker.",
      dropdown({
        value: settings.weekStartsOn ?? 0,
        ariaLabel: "Week starts on",
        options: [
          { value: 0, label: "Monday" },
          { value: 6, label: "Sunday" },
        ],
        onChange: (weekStartsOn) => void save({ weekStartsOn }),
      }),
    ),
  );
}

function behaviourPanel(settings: Settings, save: Save): HTMLElement {
  const shortcut = el("input", {
    class: "input",
    type: "text",
    placeholder: "Control+Shift+Space",
    "aria-label": "Quick Add shortcut",
  });
  shortcut.value = settings.quickAddShortcut ?? "";
  on(shortcut, "change", () => {
    const value = shortcut.value.trim();
    // Validated here as well as in Rust so a typo is caught before the OS-level
    // registration fails with a less friendly message.
    if (value && !SHORTCUT_PATTERN.test(value)) {
      shortcut.classList.add("input-invalid");
      toast("Use a combination like Control+Shift+Space", "error");
      return;
    }
    shortcut.classList.remove("input-invalid");
    void save({ quickAddShortcut: value });
  });

  return panel(
    "Behaviour",
    "Tack keeps running in the background so reminders and automations still fire.",
    settingRow(
      "Quick Add shortcut",
      "A global shortcut that opens the capture bar from anywhere. Leave empty to disable.",
      shortcut,
    ),
    settingRow(
      "Close to the tray",
      "Closing the window hides it instead of quitting.",
      toggleSwitch(settings.closeToTray ?? true, (closeToTray) => void save({ closeToTray }), "Close to the tray"),
    ),
    settingRow(
      "Start minimised",
      "Launch straight into the background with no window.",
      toggleSwitch(settings.startMinimized ?? false, (startMinimized) => void save({ startMinimized }), "Start minimised"),
    ),
  );
}

function remindersPanel(settings: Settings, save: Save): HTMLElement {
  return panel(
    "Reminders",
    "Notifications are delivered by your operating system.",
    settingRow(
      "Desktop notifications",
      "Turn off to keep reminders in-app only.",
      toggleSwitch(
        settings.notificationsEnabled ?? true,
        (notificationsEnabled) => void save({ notificationsEnabled }),
        "Desktop notifications",
      ),
    ),
    settingRow(
      "Snooze for",
      "Used by the Snooze button on a reminder.",
      dropdown({
        value: settings.snoozeMinutes ?? 10,
        ariaLabel: "Snooze duration",
        options: [
          { value: 5, label: "5 minutes" },
          { value: 10, label: "10 minutes" },
          { value: 30, label: "30 minutes" },
          { value: 60, label: "1 hour" },
          { value: 240, label: "4 hours" },
          { value: 1440, label: "1 day" },
        ],
        onChange: (snoozeMinutes) => void save({ snoozeMinutes }),
      }),
    ),
    settingRow(
      "Back up every",
      "Tack keeps the ten most recent snapshots.",
      dropdown({
        value: settings.backupIntervalHours ?? 6,
        ariaLabel: "Backup interval",
        options: [
          { value: 1, label: "Hour" },
          { value: 6, label: "6 hours" },
          { value: 12, label: "12 hours" },
          { value: 24, label: "Day" },
        ],
        onChange: (backupIntervalHours) => void save({ backupIntervalHours }),
      }),
    ),
  );
}

function labelsPanel(onChanged: () => void): HTMLElement {
  const list = el("div", { class: "chip-row" });

  const paint = (labels: Label[]) => {
    const chips = labels.map((label) => {
      const chip = el(
        "span",
        { class: "chip" },
        el("span", { text: label.name }),
        (() => {
          const remove = el("button", {
            type: "button",
            class: "chip-remove",
            "aria-label": `Delete ${label.name}`,
          }, icon("close", 11, 2.4));
          on(remove, "click", () => {
            void (async () => {
              const ok = await confirmDialog({
                title: `Delete “${label.name}”?`,
                message: "It will be removed from every task that uses it.",
                confirmLabel: "Delete",
                danger: true,
              });
              if (!ok) return;
              await guard(async () => {
                await api.deleteLabel(label.id);
                onChanged();
              });
            })();
          });
          return remove;
        })(),
      );
      chip.style.background = `color-mix(in srgb, ${label.color} 16%, transparent)`;
      chip.style.color = label.color;
      return chip;
    });
    render(list, ...chips);
  };

  paint(state.data.labels);

  return panel(
    "Labels",
    "Labels created here are available on every board.",
    list,
    button("New label", { class: "btn btn-sm", onClick: () => void promptNewLabel(onChanged) }),
  );
}

async function promptNewLabel(onChanged: () => void): Promise<void> {
  const modal = openModal({ title: "New label", width: "sm" });
  const name = el("input", { class: "input", type: "text", placeholder: "Label name", "aria-label": "Label name" });

  const palette = ["#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#007AFF", "#5856D6", "#AF52DE", "#8E8E93"];
  let color = palette[4];

  const swatches = el("div", { class: "chip-row" });
  const paintSwatches = () => {
    render(
      swatches,
      ...palette.map((value) => {
        const swatch = el("button", {
          type: "button",
          class: "pill",
          "aria-label": value,
          "aria-pressed": String(value === color),
          style: "width:28px;height:24px;padding:0",
        });
        swatch.style.background = value;
        swatch.style.outline = value === color ? "2px solid var(--text)" : "none";
        swatch.style.outlineOffset = "2px";
        on(swatch, "click", () => {
          color = value;
          paintSwatches();
        });
        return swatch;
      }),
    );
  };
  paintSwatches();

  modal.body.append(
    el("label", { class: "field" }, el("span", { class: "field-label", text: "Name" }), name),
    el("div", { class: "field" }, el("span", { class: "field-label", text: "Colour" }), swatches),
  );
  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Create", {
      class: "btn btn-primary",
      onClick: () => {
        const value = name.value.trim();
        if (!value) {
          name.classList.add("input-invalid");
          return;
        }
        modal.close();
        void guard(async () => {
          await api.createLabel(value, color, null);
          onChanged();
        });
      },
    }),
  );
  name.focus();
}

async function dataPanel(onChanged: () => void): Promise<HTMLElement> {
  const backups = (await guard(() => api.listBackups())) ?? [];
  const directory = (await guard(() => api.dataDirectory())) ?? "";

  const backupList = backups.length
    ? el(
        "div",
        { class: "col", style: "gap:4px" },
        ...backups.slice(0, 5).map((backup) =>
          el(
            "div",
            { class: "row meta" },
            icon("archive", 12),
            el("span", { text: backup.name }),
            el("span", { class: "spacer" }),
            el("span", { text: formatBytes(backup.size) }),
            backup.modified ? el("span", { class: "tertiary", text: formatRelative(backup.modified) }) : null,
          ),
        ),
      )
    : el("div", { class: "meta", text: "No backups yet — the first one is written shortly after launch." });

  return panel(
    "Your data",
    "Everything lives on this machine. Nothing is sent anywhere.",
    settingRow(
      "Export",
      "Write every board, task, label and automation to a JSON file.",
      button("Export…", {
        class: "btn btn-sm",
        onClick: () => {
          void (async () => {
            const path = await saveFileDialog({
              title: "Export Tack data",
              defaultPath: `tack-export-${new Date().toISOString().slice(0, 10)}.json`,
              filters: [{ name: "JSON", extensions: ["json"] }],
            });
            if (!path) return;
            await guard(async () => {
              await api.exportData(path);
              toast("Export written", "success");
            });
          })();
        },
      }),
    ),
    settingRow(
      "Import",
      "Merge an export into this app, or replace everything with it.",
      button("Import…", {
        class: "btn btn-sm",
        onClick: () => void promptImport(onChanged),
      }),
    ),
    settingRow(
      "Back up now",
      "Takes a consistent snapshot of the database immediately.",
      button("Back up", {
        class: "btn btn-sm",
        onClick: () =>
          void guard(async () => {
            await api.createBackupNow();
            toast("Backup written", "success");
            onChanged();
          }),
      }),
    ),
    el(
      "div",
      { class: "col", style: "gap:6px" },
      el("span", { class: "field-label", text: "Recent backups" }),
      backupList,
      el("div", { class: "meta selectable", style: "margin-top:6px" }, icon("folder", 12), el("span", { text: directory })),
    ),
  );
}

async function promptImport(onChanged: () => void): Promise<void> {
  const picked = await openFileDialog({
    multiple: false,
    title: "Import Tack data",
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof picked !== "string") return;

  const modal = openModal({ title: "Import", width: "sm" });
  let mode: "merge" | "replace" = "merge";

  modal.body.append(
    el("p", { class: "meta selectable", text: picked }),
    el(
      "div",
      { class: "field" },
      el("span", { class: "field-label", text: "How should this import be applied?" }),
      dropdown({
        value: mode,
        ariaLabel: "Import mode",
        options: [
          { value: "merge" as const, label: "Merge", detail: "Add the imported boards alongside yours" },
          { value: "replace" as const, label: "Replace", detail: "Delete everything here first" },
        ],
        onChange: (next) => {
          mode = next;
        },
      }),
    ),
  );

  modal.footer.append(
    el("span", { class: "spacer" }),
    button("Cancel", { class: "btn", onClick: modal.close }),
    button("Import", {
      class: "btn btn-primary",
      onClick: () => {
        void (async () => {
          if (mode === "replace") {
            const ok = await confirmDialog({
              title: "Replace everything?",
              message:
                "Every board, task, label and automation currently in Tack will be deleted and replaced by the file's contents. A backup is written automatically, but this cannot be undone from the app.",
              confirmLabel: "Replace",
              danger: true,
            });
            if (!ok) return;
          }
          modal.close();
          await guard(async () => {
            const summary = await api.importData(picked, mode);
            toast(
              `Imported ${summary.tasks} task${summary.tasks === 1 ? "" : "s"} across ${summary.boards} board${summary.boards === 1 ? "" : "s"}`,
              "success",
            );
            onChanged();
          });
        })();
      },
    }),
  );
}
