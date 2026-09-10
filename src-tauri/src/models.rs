//! Serializable domain types shared with the frontend.
//!
//! Everything that crosses the IPC boundary is defined here so the TypeScript
//! mirror in `src/types.ts` has a single Rust counterpart to track.

use serde::{Deserialize, Serialize};

/// RFC 3339 timestamp in UTC. Stored as TEXT so the database stays diffable
/// and portable, and so exports need no conversion.
pub type Timestamp = String;
pub type Id = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: Id,
    pub name: String,
    /// Accent override in `#rrggbb`, or `None` to inherit the theme accent.
    pub color: Option<String>,
    /// Single emoji/glyph shown in the sidebar.
    pub icon: Option<String>,
    pub position: f64,
    pub is_main: bool,
    pub archived: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub id: Id,
    pub board_id: Id,
    pub name: String,
    pub position: f64,
    /// Dropping a card here completes it; completing a card moves it here.
    pub is_done_list: bool,
    pub wip_limit: Option<i64>,
    pub archived: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// `None` makes the label available on every board.
    pub board_id: Option<Id>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub id: Id,
    pub task_id: Id,
    pub text: String,
    pub done: bool,
    pub position: f64,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: Id,
    pub task_id: Id,
    /// Display name. The on-disk name is derived and sanitized separately.
    pub name: String,
    /// Absolute path inside the app's managed attachment store.
    pub path: String,
    pub size: i64,
    pub mime: Option<String>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReminderKind {
    /// Fires at a fixed instant regardless of the task's due date.
    Absolute,
    /// Fires `offset_minutes` before the task's due date, and follows it when
    /// the due date is edited.
    RelativeToDue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: Id,
    pub task_id: Id,
    pub kind: ReminderKind,
    /// Minutes before the due date, for `RelativeToDue`. 0 means "at due time".
    pub offset_minutes: Option<i64>,
    /// Resolved firing instant. Recomputed whenever the due date changes.
    pub fire_at: Option<Timestamp>,
    /// Optional repeat, independent of the task's own recurrence.
    pub recurrence: Option<Recurrence>,
    pub snoozed_until: Option<Timestamp>,
    pub fired_at: Option<Timestamp>,
    pub dismissed: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Frequency {
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Recurrence {
    pub freq: Frequency,
    /// Every `interval` units of `freq`. Always >= 1.
    #[serde(default = "one")]
    pub interval: u32,
    /// For `Weekly`: 0 = Monday .. 6 = Sunday. Empty means "same weekday".
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// For `Monthly`: clamped to the length of the target month.
    #[serde(default)]
    pub day_of_month: Option<u32>,
    /// Stop after this date (inclusive).
    #[serde(default)]
    pub until: Option<Timestamp>,
    /// Stop after this many occurrences.
    #[serde(default)]
    pub count: Option<u32>,
    /// Occurrences produced so far, used together with `count`.
    #[serde(default)]
    pub occurrences: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Id,
    pub board_id: Id,
    pub list_id: Id,
    pub title: String,
    pub description: String,
    pub notes: String,
    /// 0 none, 1 low, 2 medium, 3 high, 4 urgent.
    pub priority: i64,
    pub due_at: Option<Timestamp>,
    /// False when the user gave only a date, so the UI can hide "00:00".
    pub due_has_time: bool,
    pub completed_at: Option<Timestamp>,
    pub archived: bool,
    pub position: f64,
    pub recurrence: Option<Recurrence>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    // Hydrated relations. Always present on read, ignored on write.
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: Id,
    pub name: String,
    pub enabled: bool,
    /// `None` scopes the rule globally, across every board.
    pub board_id: Option<Id>,
    pub trigger: Trigger,
    pub conditions: Vec<Condition>,
    pub actions: Vec<AutomationAction>,
    pub last_run_at: Option<Timestamp>,
    pub run_count: i64,
    pub position: f64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Trigger {
    #[serde(rename_all = "camelCase")]
    TaskCreated { list_id: Option<Id> },
    #[serde(rename_all = "camelCase")]
    TaskCompleted { list_id: Option<Id> },
    TaskOverdue,
    DueDateReached,
    #[serde(rename_all = "camelCase")]
    TaskMoved {
        from_list_id: Option<Id>,
        to_list_id: Option<Id>,
    },
    #[serde(rename_all = "camelCase")]
    LabelAdded { label_id: Option<Id> },
    #[serde(rename_all = "camelCase")]
    LabelRemoved { label_id: Option<Id> },
    /// Fires on a wall-clock schedule rather than in response to a task.
    #[serde(rename_all = "camelCase")]
    Scheduled { schedule: Schedule },
}

/// A wall-clock schedule evaluated in the user's local timezone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// 0-23 local hour.
    pub hour: u32,
    /// 0-59 local minute.
    pub minute: u32,
    /// Empty = every day. Otherwise 0 = Monday .. 6 = Sunday.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// When set, only fires on this day of the month.
    #[serde(default)]
    pub day_of_month: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Condition {
    #[serde(rename_all = "camelCase")]
    HasLabel { label_id: Id },
    #[serde(rename_all = "camelCase")]
    LacksLabel { label_id: Id },
    #[serde(rename_all = "camelCase")]
    PriorityAtLeast { priority: i64 },
    #[serde(rename_all = "camelCase")]
    PriorityEquals { priority: i64 },
    #[serde(rename_all = "camelCase")]
    InList { list_id: Id },
    #[serde(rename_all = "camelCase")]
    InBoard { board_id: Id },
    #[serde(rename_all = "camelCase")]
    TitleContains { text: String },
    IsOverdue,
    IsCompleted,
    IsNotCompleted,
    HasDueDate,
    HasNoDueDate,
    #[serde(rename_all = "camelCase")]
    DueWithinMinutes { minutes: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AutomationAction {
    #[serde(rename_all = "camelCase")]
    MoveTask { board_id: Option<Id>, list_id: Id },
    #[serde(rename_all = "camelCase")]
    CreateTask {
        board_id: Option<Id>,
        list_id: Option<Id>,
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        priority: i64,
        /// Due date relative to the moment the action runs.
        #[serde(default)]
        due_in_minutes: Option<i64>,
        #[serde(default)]
        label_ids: Vec<Id>,
    },
    CompleteTask,
    #[serde(rename_all = "camelCase")]
    SetPriority { priority: i64 },
    #[serde(rename_all = "camelCase")]
    AddLabel { label_id: Id },
    #[serde(rename_all = "camelCase")]
    RemoveLabel { label_id: Id },
    /// Shifts the due date by `minutes` from now (positive = future).
    #[serde(rename_all = "camelCase")]
    SetDueDate { in_minutes: i64 },
    /// Creates an absolute reminder `in_minutes` from now.
    #[serde(rename_all = "camelCase")]
    SetReminder { in_minutes: i64 },
    #[serde(rename_all = "camelCase")]
    Notify { title: String, body: String },
    DuplicateTask,
    ArchiveTask,
}

/// One line of the per-task history shown in the task detail view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: Id,
    pub task_id: Option<Id>,
    pub board_id: Option<Id>,
    pub kind: String,
    pub message: String,
    pub created_at: Timestamp,
}

/// Everything the sidebar and board view need in a single round trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardView {
    pub board: Board,
    pub lists: Vec<List>,
    pub tasks: Vec<Task>,
    pub labels: Vec<Label>,
}

/// Counts backing the global-view sidebar badges.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalCounts {
    pub today: i64,
    pub upcoming: i64,
    pub overdue: i64,
    pub completed: i64,
    pub all: i64,
    pub no_due_date: i64,
}

/// Filter accepted by the global task view and by search.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskQuery {
    pub text: Option<String>,
    pub board_ids: Vec<Id>,
    pub label_ids: Vec<Id>,
    /// Inclusive lower bound on priority.
    pub min_priority: Option<i64>,
    /// `None` = any, `Some(true)` = completed only, `Some(false)` = open only.
    pub completed: Option<bool>,
    /// One of: today, upcoming, overdue, completed, all, nodue.
    pub scope: Option<String>,
    pub due_before: Option<Timestamp>,
    pub due_after: Option<Timestamp>,
    pub include_archived: bool,
    pub limit: Option<i64>,
}

/// A search hit plus the field that matched, so the UI can show context.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub task: Task,
    pub board_name: String,
    pub list_name: String,
    /// "title" | "description" | "notes" | "checklist"
    pub matched_field: String,
    pub snippet: String,
}
