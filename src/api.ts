// Typed wrappers around the Rust commands.
//
// The backend returns errors as plain strings, so `call` normalises whatever
// arrives into an `Error` the UI can show verbatim.

import { invoke } from "@tauri-apps/api/core";

import type {
  ActivityEntry,
  Attachment,
  Automation,
  AutomationAction,
  BackupInfo,
  Board,
  BoardView,
  Bootstrap,
  ChecklistItem,
  Condition,
  GlobalCounts,
  Id,
  ImportMode,
  ImportSummary,
  Label,
  List,
  ParsedQuickAdd,
  Recurrence,
  Reminder,
  SearchHit,
  Settings,
  Task,
  TaskPatch,
  TaskQuery,
  Trigger,
} from "./types";

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw new Error(typeof error === "string" ? error : String(error));
  }
}

// Startup -----------------------------------------------------------------

export const bootstrap = () => call<Bootstrap>("bootstrap");
export const dataDirectory = () => call<string>("data_directory");

// Boards ------------------------------------------------------------------

export const listBoards = (includeArchived = false) =>
  call<Board[]>("list_boards", { includeArchived });

export const boardView = (boardId: Id) => call<BoardView>("board_view", { boardId });

export const createBoard = (name: string, color?: string | null, icon?: string | null) =>
  call<Board>("create_board", { name, color: color ?? null, icon: icon ?? null });

export const updateBoard = (
  boardId: Id,
  patch: { name?: string; color?: string | null; icon?: string | null; archived?: boolean },
) => call<Board>("update_board", { boardId, patch });

export const deleteBoard = (boardId: Id) => call<void>("delete_board", { boardId });

export const reorderBoard = (boardId: Id, index: number) =>
  call<Board[]>("reorder_board", { boardId, index });

// Lists -------------------------------------------------------------------

export const createList = (boardId: Id, name: string, isDoneList = false) =>
  call<List>("create_list", { boardId, name, isDoneList });

export const updateList = (
  listId: Id,
  patch: { name?: string; isDoneList?: boolean; wipLimit?: number | null; archived?: boolean },
) => call<List>("update_list", { listId, patch });

export const deleteList = (listId: Id) => call<void>("delete_list", { listId });

export const reorderList = (listId: Id, index: number) =>
  call<List[]>("reorder_list", { listId, index });

// Tasks -------------------------------------------------------------------

export interface TaskInput {
  boardId?: Id | null;
  listId?: Id | null;
  title: string;
  description?: string;
  notes?: string;
  priority?: number;
  dueAt?: string | null;
  dueHasTime?: boolean;
  recurrence?: Recurrence | null;
  labelIds?: Id[];
  index?: number | null;
}

export const createTask = (input: TaskInput) => call<Task>("create_task", { input });
export const updateTask = (taskId: Id, patch: TaskPatch) =>
  call<Task>("update_task", { taskId, patch });
export const moveTask = (taskId: Id, listId: Id, index?: number) =>
  call<Task>("move_task", { taskId, listId, index: index ?? null });
export const setTaskCompleted = (taskId: Id, completed: boolean) =>
  call<Task>("set_task_completed", { taskId, completed });
export const deleteTask = (taskId: Id) => call<void>("delete_task", { taskId });
export const duplicateTask = (taskId: Id) => call<Task>("duplicate_task", { taskId });
export const getTask = (taskId: Id) => call<Task>("get_task", { taskId });
export const taskActivity = (taskId: Id) => call<ActivityEntry[]>("task_activity", { taskId });

// Checklists --------------------------------------------------------------

export const addChecklistItem = (taskId: Id, text: string) =>
  call<ChecklistItem>("add_checklist_item", { taskId, text });
export const updateChecklistItem = (itemId: Id, text: string | null, done: boolean | null) =>
  call<void>("update_checklist_item", { itemId, text, done });
export const deleteChecklistItem = (itemId: Id) => call<void>("delete_checklist_item", { itemId });

// Labels ------------------------------------------------------------------

export const listLabels = () => call<Label[]>("list_labels");
export const createLabel = (name: string, color: string, boardId: Id | null = null) =>
  call<Label>("create_label", { name, color, boardId });
export const updateLabel = (labelId: Id, name: string | null, color: string | null) =>
  call<Label>("update_label", { labelId, name, color });
export const deleteLabel = (labelId: Id) => call<void>("delete_label", { labelId });
export const setTaskLabel = (taskId: Id, labelId: Id, attached: boolean) =>
  call<Task>("set_task_label", { taskId, labelId, attached });

// Reminders ---------------------------------------------------------------

export const addRelativeReminder = (taskId: Id, offsetMinutes: number) =>
  call<Reminder>("add_reminder", { taskId, offsetMinutes, fireAt: null, recurrence: null });

export const addAbsoluteReminder = (
  taskId: Id,
  fireAt: string,
  recurrence: Recurrence | null = null,
) => call<Reminder>("add_reminder", { taskId, offsetMinutes: null, fireAt, recurrence });

export const deleteReminder = (reminderId: Id) => call<void>("delete_reminder", { reminderId });
export const snoozeReminder = (reminderId: Id, minutes?: number) =>
  call<Reminder>("snooze_reminder", { reminderId, minutes: minutes ?? null });
export const dismissReminder = (reminderId: Id) => call<void>("dismiss_reminder", { reminderId });

// Views and search --------------------------------------------------------

export const queryTasks = (query: TaskQuery) => call<Task[]>("query_tasks", { query });
export const searchTasks = (query: TaskQuery) => call<SearchHit[]>("search_tasks", { query });
export const globalCounts = () => call<GlobalCounts>("global_counts");

// Automations -------------------------------------------------------------

export const listAutomations = () => call<Automation[]>("list_automations");

export const createAutomation = (input: {
  name: string;
  boardId: Id | null;
  trigger: Trigger;
  conditions: Condition[];
  actions: AutomationAction[];
  enabled: boolean;
}) => call<Automation>("create_automation", { input });

export const updateAutomation = (
  automationId: Id,
  patch: {
    name?: string;
    enabled?: boolean;
    boardId?: Id | null;
    trigger?: Trigger;
    conditions?: Condition[];
    actions?: AutomationAction[];
  },
) => call<Automation>("update_automation", { automationId, patch });

export const deleteAutomation = (automationId: Id) =>
  call<void>("delete_automation", { automationId });

// Settings ----------------------------------------------------------------

export const getSettings = () => call<Settings>("get_settings");
export const saveSettings = (values: Partial<Settings>) =>
  call<Settings>("save_settings", { values });

// Quick Add ---------------------------------------------------------------

export const previewQuickAdd = (text: string) => call<ParsedQuickAdd>("preview_quick_add", { text });
export const quickAdd = (text: string, boardId: Id | null = null, listId: Id | null = null) =>
  call<Task>("quick_add", { text, boardId, listId });
export const hideQuickAdd = () => call<void>("hide_quick_add");
export const showMainWindow = () => call<void>("show_main_window");

// Attachments -------------------------------------------------------------

export const addAttachment = (taskId: Id, path: string) =>
  call<Attachment>("add_attachment", { taskId, path });
export const deleteAttachment = (attachmentId: Id) =>
  call<void>("delete_attachment", { attachmentId });
export const openAttachment = (attachmentId: Id) => call<void>("open_attachment", { attachmentId });

// Import / export and backups ---------------------------------------------

export const exportData = (path: string) => call<string>("export_data", { path });
export const importData = (path: string, mode: ImportMode) =>
  call<ImportSummary>("import_data", { path, mode });
export const listBackups = () => call<BackupInfo[]>("list_backups");
export const createBackupNow = () => call<string>("create_backup_now");
