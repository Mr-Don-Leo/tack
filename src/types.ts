// Mirrors `src-tauri/src/models.rs`. Kept hand-written rather than generated so
// the shapes the UI actually consumes stay obvious at a glance.

export type Id = string;
/** RFC 3339 timestamp in UTC. */
export type Timestamp = string;

export interface Board {
  id: Id;
  name: string;
  color: string | null;
  icon: string | null;
  position: number;
  isMain: boolean;
  archived: boolean;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

export interface List {
  id: Id;
  boardId: Id;
  name: string;
  position: number;
  isDoneList: boolean;
  wipLimit: number | null;
  archived: boolean;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

export interface Label {
  id: Id;
  name: string;
  color: string;
  boardId: Id | null;
  createdAt: Timestamp;
}

export interface ChecklistItem {
  id: Id;
  taskId: Id;
  text: string;
  done: boolean;
  position: number;
  createdAt: Timestamp;
}

export interface Attachment {
  id: Id;
  taskId: Id;
  name: string;
  path: string;
  size: number;
  mime: string | null;
  createdAt: Timestamp;
}

export type ReminderKind = "absolute" | "relativeToDue";

export interface Reminder {
  id: Id;
  taskId: Id;
  kind: ReminderKind;
  offsetMinutes: number | null;
  fireAt: Timestamp | null;
  recurrence: Recurrence | null;
  snoozedUntil: Timestamp | null;
  firedAt: Timestamp | null;
  dismissed: boolean;
  createdAt: Timestamp;
}

export type Frequency = "daily" | "weekdays" | "weekly" | "monthly" | "yearly";

export interface Recurrence {
  freq: Frequency;
  interval: number;
  /** 0 = Monday … 6 = Sunday. */
  weekdays: number[];
  dayOfMonth: number | null;
  until: Timestamp | null;
  count: number | null;
  occurrences: number;
}

/** 0 none, 1 low, 2 medium, 3 high, 4 urgent. */
export type Priority = 0 | 1 | 2 | 3 | 4;

export interface Task {
  id: Id;
  boardId: Id;
  listId: Id;
  title: string;
  description: string;
  notes: string;
  priority: number;
  dueAt: Timestamp | null;
  dueHasTime: boolean;
  completedAt: Timestamp | null;
  archived: boolean;
  position: number;
  recurrence: Recurrence | null;
  createdAt: Timestamp;
  updatedAt: Timestamp;
  labels: Label[];
  checklist: ChecklistItem[];
  attachments: Attachment[];
  reminders: Reminder[];
}

export interface BoardView {
  board: Board;
  lists: List[];
  tasks: Task[];
  labels: Label[];
}

export interface GlobalCounts {
  today: number;
  upcoming: number;
  overdue: number;
  completed: number;
  all: number;
  noDueDate: number;
}

export type Scope = "today" | "upcoming" | "overdue" | "completed" | "all" | "nodue";

export interface TaskQuery {
  text?: string | null;
  boardIds?: Id[];
  labelIds?: Id[];
  minPriority?: number | null;
  completed?: boolean | null;
  scope?: Scope | null;
  dueBefore?: Timestamp | null;
  dueAfter?: Timestamp | null;
  includeArchived?: boolean;
  limit?: number | null;
}

export interface SearchHit {
  task: Task;
  boardName: string;
  listName: string;
  matchedField: "title" | "description" | "notes" | "checklist";
  snippet: string;
}

export interface ActivityEntry {
  id: Id;
  taskId: Id | null;
  boardId: Id | null;
  kind: string;
  message: string;
  createdAt: Timestamp;
}

// Automations -------------------------------------------------------------

export interface Schedule {
  hour: number;
  minute: number;
  weekdays: number[];
  dayOfMonth: number | null;
}

export type Trigger =
  | { type: "taskCreated"; listId: Id | null }
  | { type: "taskCompleted"; listId: Id | null }
  | { type: "taskOverdue" }
  | { type: "dueDateReached" }
  | { type: "taskMoved"; fromListId: Id | null; toListId: Id | null }
  | { type: "labelAdded"; labelId: Id | null }
  | { type: "labelRemoved"; labelId: Id | null }
  | { type: "scheduled"; schedule: Schedule };

export type TriggerType = Trigger["type"];

export type Condition =
  | { type: "hasLabel"; labelId: Id }
  | { type: "lacksLabel"; labelId: Id }
  | { type: "priorityAtLeast"; priority: number }
  | { type: "priorityEquals"; priority: number }
  | { type: "inList"; listId: Id }
  | { type: "inBoard"; boardId: Id }
  | { type: "titleContains"; text: string }
  | { type: "isOverdue" }
  | { type: "isCompleted" }
  | { type: "isNotCompleted" }
  | { type: "hasDueDate" }
  | { type: "hasNoDueDate" }
  | { type: "dueWithinMinutes"; minutes: number };

export type ConditionType = Condition["type"];

export type AutomationAction =
  | { type: "moveTask"; boardId: Id | null; listId: Id }
  | {
      type: "createTask";
      boardId: Id | null;
      listId: Id | null;
      title: string;
      description: string;
      priority: number;
      dueInMinutes: number | null;
      labelIds: Id[];
    }
  | { type: "completeTask" }
  | { type: "setPriority"; priority: number }
  | { type: "addLabel"; labelId: Id }
  | { type: "removeLabel"; labelId: Id }
  | { type: "setDueDate"; inMinutes: number }
  | { type: "setReminder"; inMinutes: number }
  | { type: "notify"; title: string; body: string }
  | { type: "duplicateTask" }
  | { type: "archiveTask" };

export type ActionType = AutomationAction["type"];

export interface Automation {
  id: Id;
  name: string;
  enabled: boolean;
  boardId: Id | null;
  trigger: Trigger;
  conditions: Condition[];
  actions: AutomationAction[];
  lastRunAt: Timestamp | null;
  runCount: number;
  position: number;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

// Settings, bootstrap and maintenance -------------------------------------

export interface Settings {
  theme: "system" | "light" | "dark";
  skin: "apple" | "cyberpunk" | "xp";
  quickAddShortcut: string;
  closeToTray: boolean;
  startMinimized: boolean;
  notificationsEnabled: boolean;
  notificationSound: boolean;
  snoozeMinutes: number;
  defaultReminderOffsets: number[];
  backupIntervalHours: number;
  weekStartsOn: number;
  lastBoardId: Id | null;
  lastBackupAt: Timestamp | null;
}

export interface Bootstrap {
  boards: Board[];
  mainBoardId: Id;
  labels: Label[];
  settings: Settings;
  counts: GlobalCounts;
  alerts: Task[];
  platform: string;
}

export interface ParsedQuickAdd {
  title: string;
  dueAt: Timestamp | null;
  dueHasTime: boolean;
  priority: number;
  labels: string[];
  board: string | null;
  recurrence: Recurrence | null;
}

export interface BackupInfo {
  path: string;
  name: string;
  size: number;
  modified: Timestamp | null;
}

export interface ImportSummary {
  boards: number;
  lists: number;
  tasks: number;
  labels: number;
  automations: number;
  skippedAttachments: number;
}

export type ImportMode = "merge" | "replace";

/** A patch where `null` clears the field and `undefined` leaves it alone. */
export interface TaskPatch {
  title?: string;
  description?: string;
  notes?: string;
  priority?: number;
  dueAt?: Timestamp | null;
  dueHasTime?: boolean;
  recurrence?: Recurrence | null;
  archived?: boolean;
}
