// Formatting helpers. All display strings for dates, priorities, recurrence and
// sizes live here so the same task reads identically on a card, in a list row
// and in the editor.

import type { Recurrence, Reminder, Task } from "./types";

const DAY = 24 * 60 * 60 * 1000;

export const WEEKDAY_NAMES = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
export const WEEKDAY_SHORT = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export const PRIORITY_NAMES = ["None", "Low", "Medium", "High", "Urgent"];

export function priorityName(priority: number): string {
  return PRIORITY_NAMES[priority] ?? "None";
}

/** Local midnight for the day containing `date`. */
export function startOfDay(date: Date): Date {
  const copy = new Date(date);
  copy.setHours(0, 0, 0, 0);
  return copy;
}

/** Whole local days from today to `date`; negative for the past. */
export function dayOffset(date: Date, now = new Date()): number {
  return Math.round((startOfDay(date).getTime() - startOfDay(now).getTime()) / DAY);
}

export function isSameDay(a: Date, b: Date): boolean {
  return startOfDay(a).getTime() === startOfDay(b).getTime();
}

const timeFormat = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const dateFormat = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" });
const dateYearFormat = new Intl.DateTimeFormat(undefined, {
  day: "numeric",
  month: "short",
  year: "numeric",
});
const fullFormat = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

export function formatTime(date: Date): string {
  return timeFormat.format(date);
}

/** "Today", "Tomorrow", "Mon 14 Sep" — whichever is shortest and unambiguous. */
export function formatDay(date: Date, now = new Date()): string {
  const offset = dayOffset(date, now);
  if (offset === 0) return "Today";
  if (offset === 1) return "Tomorrow";
  if (offset === -1) return "Yesterday";
  if (offset > 1 && offset < 7) return WEEKDAY_NAMES[(date.getDay() + 6) % 7];
  return date.getFullYear() === now.getFullYear() ? dateFormat.format(date) : dateYearFormat.format(date);
}

/** The full, unambiguous form used in tooltips. */
export function formatFull(date: Date): string {
  return fullFormat.format(date);
}

/** A due date as it appears on a card: day plus time when one was set. */
export function formatDue(iso: string, hasTime: boolean, now = new Date()): string {
  const date = new Date(iso);
  const day = formatDay(date, now);
  return hasTime ? `${day}, ${formatTime(date)}` : day;
}

export type DueTone = "none" | "today" | "overdue";

export function dueTone(task: Task, now = new Date()): DueTone {
  if (!task.dueAt || task.completedAt) return "none";
  const due = new Date(task.dueAt);
  if (due.getTime() < now.getTime()) return "overdue";
  return isSameDay(due, now) ? "today" : "none";
}

/** "in 3 days", "2 hours ago" — used for reminders and activity lines. */
export function formatRelative(iso: string, now = new Date()): string {
  const target = new Date(iso).getTime();
  const diffMinutes = Math.round((target - now.getTime()) / 60000);
  const abs = Math.abs(diffMinutes);
  const past = diffMinutes < 0;

  const say = (value: number, unit: string) => {
    const plural = value === 1 ? unit : `${unit}s`;
    return past ? `${value} ${plural} ago` : `in ${value} ${plural}`;
  };

  if (abs < 1) return "now";
  if (abs < 60) return say(abs, "minute");
  if (abs < 60 * 24) return say(Math.round(abs / 60), "hour");
  if (abs < 60 * 24 * 30) return say(Math.round(abs / (60 * 24)), "day");
  return formatDay(new Date(iso), now);
}

/** Human summary of a repeat rule, e.g. "Every 2 weeks on Mon, Wed". */
export function describeRecurrence(rec: Recurrence): string {
  const interval = Math.max(1, rec.interval);
  const every = (unit: string) => (interval === 1 ? `Every ${unit}` : `Every ${interval} ${unit}s`);

  let base: string;
  switch (rec.freq) {
    case "daily":
      base = every("day");
      break;
    case "weekdays":
      base = "Every weekday";
      break;
    case "weekly": {
      const days = rec.weekdays.map((d) => WEEKDAY_SHORT[d]).filter(Boolean);
      base = days.length ? `${every("week")} on ${days.join(", ")}` : every("week");
      break;
    }
    case "monthly":
      base = rec.dayOfMonth ? `${every("month")} on day ${rec.dayOfMonth}` : every("month");
      break;
    case "yearly":
      base = every("year");
      break;
  }

  if (rec.count) return `${base}, ${rec.count} times`;
  if (rec.until) return `${base}, until ${formatDay(new Date(rec.until))}`;
  return base;
}

/** "10 minutes before", "At the due time", or an absolute instant. */
export function describeReminder(reminder: Reminder): string {
  if (reminder.kind === "relativeToDue") {
    const minutes = reminder.offsetMinutes ?? 0;
    if (minutes === 0) return "At the due time";
    return `${describeOffset(minutes)} before`;
  }
  if (!reminder.fireAt) return "No time set";
  const suffix = reminder.recurrence ? ` · ${describeRecurrence(reminder.recurrence)}` : "";
  return `${formatDue(reminder.fireAt, true)}${suffix}`;
}

export function describeOffset(minutes: number): string {
  if (minutes % (60 * 24) === 0) {
    const days = minutes / (60 * 24);
    return `${days} ${days === 1 ? "day" : "days"}`;
  }
  if (minutes % 60 === 0) {
    const hours = minutes / 60;
    return `${hours} ${hours === 1 ? "hour" : "hours"}`;
  }
  return `${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/**
 * Combines a local date and an optional `HH:MM` into a UTC ISO string.
 *
 * With no time the due instant is the end of that local day, which keeps a task
 * due "today" from being reported as overdue the moment the day begins.
 */
export function toIso(date: Date, time: string | null): string {
  const combined = new Date(date);
  if (time) {
    const [hours, minutes] = time.split(":").map(Number);
    combined.setHours(hours || 0, minutes || 0, 0, 0);
  } else {
    combined.setHours(23, 59, 0, 0);
  }
  return combined.toISOString();
}

/** `HH:MM` in local time, for a time input. */
export function toTimeValue(iso: string): string {
  const date = new Date(iso);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function checklistProgress(task: Task): { done: number; total: number } {
  return {
    done: task.checklist.filter((item) => item.done).length,
    total: task.checklist.length,
  };
}

/** Groups tasks under Overdue / Today / Tomorrow / week / later headings. */
export function groupByDue(tasks: Task[], now = new Date()): Array<[string, Task[]]> {
  const groups = new Map<string, Task[]>();
  const push = (key: string, task: Task) => {
    const bucket = groups.get(key);
    if (bucket) bucket.push(task);
    else groups.set(key, [task]);
  };

  for (const task of tasks) {
    if (!task.dueAt) {
      push("No due date", task);
      continue;
    }
    const due = new Date(task.dueAt);
    const offset = dayOffset(due, now);
    if (!task.completedAt && due.getTime() < now.getTime()) push("Overdue", task);
    else if (offset === 0) push("Today", task);
    else if (offset === 1) push("Tomorrow", task);
    else if (offset > 1 && offset <= 7) push("This week", task);
    else if (offset > 7) push("Later", task);
    else push("Earlier", task);
  }

  const order = ["Overdue", "Today", "Tomorrow", "This week", "Later", "Earlier", "No due date"];
  return order.filter((key) => groups.has(key)).map((key) => [key, groups.get(key)!]);
}
