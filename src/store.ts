// Application state.
//
// A deliberately small store: the backend is the source of truth, so this holds
// the current route, the last snapshot of boards/labels/settings, and a
// subscription mechanism that lets views re-render when the data changes.

import * as api from "./api";
import type { Board, GlobalCounts, Id, Label, Scope, Settings, Task } from "./types";

export type Route =
  | { kind: "board"; boardId: Id }
  | { kind: "view"; scope: Scope }
  | { kind: "search"; text: string }
  | { kind: "automations" }
  | { kind: "settings" };

export interface AppData {
  boards: Board[];
  mainBoardId: Id;
  labels: Label[];
  settings: Settings;
  counts: GlobalCounts;
  platform: string;
}

type Listener = () => void;

const listeners = new Set<Listener>();
const routeListeners = new Set<Listener>();

let data: AppData = {
  boards: [],
  mainBoardId: "",
  labels: [],
  settings: {} as Settings,
  counts: { today: 0, upcoming: 0, overdue: 0, completed: 0, all: 0, noDueDate: 0 },
  platform: "",
};

let route: Route = { kind: "view", scope: "today" };
/** Reminders that fired and have not been acted on, newest last. */
let alerts: Task[] = [];

export const state = {
  get data(): AppData {
    return data;
  },
  get route(): Route {
    return route;
  },
  get alerts(): Task[] {
    return alerts;
  },
};

export function boardById(id: Id): Board | undefined {
  return data.boards.find((board) => board.id === id);
}

export function mainBoard(): Board | undefined {
  return boardById(data.mainBoardId);
}

/** Subscribes to data changes. Returns an unsubscribe function. */
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function subscribeRoute(listener: Listener): () => void {
  routeListeners.add(listener);
  return () => routeListeners.delete(listener);
}

function emit(): void {
  for (const listener of listeners) listener();
}

export function navigate(next: Route): void {
  route = next;
  if (next.kind === "board") {
    // Remember the board so the next launch opens where the user left off.
    void api.saveSettings({ lastBoardId: next.boardId }).catch(() => {});
  }
  for (const listener of routeListeners) listener();
}

export async function load(): Promise<void> {
  const snapshot = await api.bootstrap();
  data = {
    boards: snapshot.boards,
    mainBoardId: snapshot.mainBoardId,
    labels: snapshot.labels,
    settings: snapshot.settings,
    counts: snapshot.counts,
    platform: snapshot.platform,
  };
  alerts = snapshot.alerts;
  emit();
}

/** Re-reads boards, labels, settings and counts after a mutation. */
export async function refresh(): Promise<void> {
  const [boards, labels, counts, settings] = await Promise.all([
    api.listBoards(false),
    api.listLabels(),
    api.globalCounts(),
    api.getSettings(),
  ]);
  data = { ...data, boards, labels, counts, settings };
  emit();
}

export function setSettings(settings: Settings): void {
  data = { ...data, settings };
  emit();
}

export function pushAlert(task: Task): void {
  alerts = [...alerts.filter((existing) => existing.id !== task.id), task];
  emit();
}

export function dismissAlert(taskId: Id): void {
  alerts = alerts.filter((task) => task.id !== taskId);
  emit();
}

/** The board a new task should land on when the user has not chosen one. */
export function defaultBoardId(): Id {
  return route.kind === "board" ? route.boardId : data.mainBoardId;
}
