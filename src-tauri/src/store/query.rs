//! The global task view and search.
//!
//! Both are the same query with different filters, so "Overdue" and a text
//! search can be combined without a second code path. Text matching uses
//! escaped `LIKE` over a lowercased haystack, which keeps the schema simple and
//! stays well under a millisecond at personal-task volumes.

use chrono::{Duration, Utc};
use rusqlite::{Connection, ToSql, params_from_iter};

use crate::error::Result;
use crate::models::{GlobalCounts, SearchHit, Task, TaskQuery};
use crate::store::tasks;
use crate::util::{escape_like, local_day_end, local_day_start, normalize_search, to_ts};

struct Filters {
    clauses: Vec<String>,
    args: Vec<Box<dyn ToSql>>,
}

impl Filters {
    fn new() -> Self {
        Self { clauses: Vec::new(), args: Vec::new() }
    }

    /// Adds a clause whose `?` placeholders are numbered as they are pushed.
    fn push(&mut self, clause: impl Into<String>, args: Vec<Box<dyn ToSql>>) {
        self.clauses.push(clause.into());
        self.args.extend(args);
    }

    fn where_sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

fn build(query: &TaskQuery) -> Filters {
    let mut f = Filters::new();
    let now = Utc::now();

    if !query.include_archived {
        f.push("t.archived = 0", vec![]);
    }

    match query.scope.as_deref() {
        // Today is strictly today's local day; anything earlier belongs to
        // Overdue, which has its own view and its own badge.
        Some("today") => {
            f.push("t.completed_at IS NULL", vec![]);
            f.push(
                "t.due_at IS NOT NULL AND t.due_at >= ? AND t.due_at < ?",
                vec![
                    Box::new(to_ts(local_day_start(now))),
                    Box::new(to_ts(local_day_end(now))),
                ],
            );
        }
        Some("upcoming") => {
            f.push("t.completed_at IS NULL", vec![]);
            f.push(
                "t.due_at IS NOT NULL AND t.due_at >= ?",
                vec![Box::new(to_ts(local_day_end(now)))],
            );
        }
        Some("overdue") => {
            f.push("t.completed_at IS NULL", vec![]);
            f.push("t.due_at IS NOT NULL AND t.due_at < ?", vec![Box::new(to_ts(now))]);
        }
        Some("completed") => f.push("t.completed_at IS NOT NULL", vec![]),
        Some("nodue") => {
            f.push("t.completed_at IS NULL", vec![]);
            f.push("t.due_at IS NULL", vec![]);
        }
        // "all" and anything unrecognised fall through to the explicit filters.
        _ => {}
    }

    if let Some(completed) = query.completed {
        f.push(
            if completed { "t.completed_at IS NOT NULL" } else { "t.completed_at IS NULL" },
            vec![],
        );
    }

    if !query.board_ids.is_empty() {
        let holes = vec!["?"; query.board_ids.len()].join(", ");
        f.push(
            format!("t.board_id IN ({holes})"),
            query
                .board_ids
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn ToSql>)
                .collect(),
        );
    }

    if !query.label_ids.is_empty() {
        let holes = vec!["?"; query.label_ids.len()].join(", ");
        f.push(
            format!(
                "EXISTS (SELECT 1 FROM task_labels tl WHERE tl.task_id = t.id AND tl.label_id IN ({holes}))"
            ),
            query
                .label_ids
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn ToSql>)
                .collect(),
        );
    }

    if let Some(min) = query.min_priority {
        f.push("t.priority >= ?", vec![Box::new(min)]);
    }
    if let Some(before) = &query.due_before {
        f.push("t.due_at IS NOT NULL AND t.due_at < ?", vec![Box::new(before.clone())]);
    }
    if let Some(after) = &query.due_after {
        f.push("t.due_at IS NOT NULL AND t.due_at >= ?", vec![Box::new(after.clone())]);
    }

    if let Some(text) = query.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let needle = format!("%{}%", escape_like(&normalize_search(text)));
        f.push(
            r"(LOWER(t.title) LIKE ? ESCAPE '\'
               OR LOWER(t.description) LIKE ? ESCAPE '\'
               OR LOWER(t.notes) LIKE ? ESCAPE '\'
               OR EXISTS (SELECT 1 FROM checklist_items ci
                          WHERE ci.task_id = t.id AND LOWER(ci.text) LIKE ? ESCAPE '\'))",
            // Four placeholders, so the needle is bound four times.
            vec![
                Box::new(needle.clone()),
                Box::new(needle.clone()),
                Box::new(needle.clone()),
                Box::new(needle),
            ],
        );
    }

    f
}

/// Tasks matching `query`, ordered so the most urgent work surfaces first:
/// dated before undated, then by due date, then by priority.
pub fn tasks(conn: &Connection, query: &TaskQuery) -> Result<Vec<Task>> {
    let f = build(query);
    let limit = query.limit.filter(|l| *l > 0).unwrap_or(2000);
    let sql = format!(
        "SELECT t.id, t.board_id, t.list_id, t.title, t.description, t.notes, t.priority, t.due_at,
                t.due_has_time, t.completed_at, t.archived, t.position, t.recurrence, t.created_at, t.updated_at
         FROM tasks t {}
         ORDER BY t.due_at IS NULL, t.due_at ASC, t.priority DESC, t.created_at DESC
         LIMIT {limit}",
        f.where_sql()
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(f.args.iter()), tasks::map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    tasks::hydrate_all(conn, rows)
}

/// Search results annotated with the field that matched, for result context.
pub fn search(conn: &Connection, query: &TaskQuery) -> Result<Vec<SearchHit>> {
    let needle = query
        .text
        .as_deref()
        .map(normalize_search)
        .unwrap_or_default();
    let matches = tasks(conn, query)?;

    let mut hits = Vec::with_capacity(matches.len());
    for task in matches {
        let board_name: String = conn.query_row(
            "SELECT name FROM boards WHERE id = ?1",
            [&task.board_id],
            |r| r.get(0),
        )?;
        let list_name: String = conn.query_row(
            "SELECT name FROM lists WHERE id = ?1",
            [&task.list_id],
            |r| r.get(0),
        )?;
        let (matched_field, snippet) = classify(&task, &needle);
        hits.push(SearchHit { task, board_name, list_name, matched_field, snippet });
    }
    Ok(hits)
}

/// Picks the field to quote in the result row, preferring the most specific
/// match over the title so the user can see *why* a task matched.
fn classify(task: &Task, needle: &str) -> (String, String) {
    if needle.is_empty() {
        return ("title".into(), String::new());
    }
    let contains = |text: &str| text.to_lowercase().contains(needle);

    if contains(&task.title) {
        return ("title".into(), String::new());
    }
    if contains(&task.description) {
        return ("description".into(), excerpt(&task.description, needle));
    }
    if contains(&task.notes) {
        return ("notes".into(), excerpt(&task.notes, needle));
    }
    if let Some(item) = task.checklist.iter().find(|i| contains(&i.text)) {
        return ("checklist".into(), item.text.clone());
    }
    ("title".into(), String::new())
}

/// ~120 characters of context around the first match, on character boundaries.
fn excerpt(text: &str, needle: &str) -> String {
    let lower = text.to_lowercase();
    let Some(byte_pos) = lower.find(needle) else {
        return text.chars().take(120).collect();
    };
    let char_pos = text[..byte_pos].chars().count();
    let start = char_pos.saturating_sub(40);
    let body: String = text.chars().skip(start).take(120).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if start + 120 < text.chars().count() { "…" } else { "" };
    format!("{prefix}{body}{suffix}")
}

/// Badge counts for the global-view sidebar, in one pass over the table.
pub fn counts(conn: &Connection) -> Result<GlobalCounts> {
    let now = Utc::now();
    let today_end = to_ts(local_day_end(now));
    let today_start = to_ts(local_day_start(now));
    let now_ts = to_ts(now);

    conn.query_row(
        "SELECT
           SUM(CASE WHEN completed_at IS NULL AND due_at IS NOT NULL
                     AND due_at >= ?1 AND due_at < ?2 THEN 1 ELSE 0 END) AS today,
           SUM(CASE WHEN completed_at IS NULL AND due_at IS NOT NULL AND due_at >= ?2 THEN 1 ELSE 0 END) AS upcoming,
           SUM(CASE WHEN completed_at IS NULL AND due_at IS NOT NULL AND due_at < ?3 THEN 1 ELSE 0 END) AS overdue,
           SUM(CASE WHEN completed_at IS NOT NULL THEN 1 ELSE 0 END) AS completed,
           SUM(CASE WHEN completed_at IS NULL THEN 1 ELSE 0 END) AS all_open,
           SUM(CASE WHEN completed_at IS NULL AND due_at IS NULL THEN 1 ELSE 0 END) AS no_due
         FROM tasks WHERE archived = 0",
        rusqlite::params![today_start, today_end, now_ts],
        |row| {
            Ok(GlobalCounts {
                today: row.get::<_, Option<i64>>("today")?.unwrap_or(0),
                upcoming: row.get::<_, Option<i64>>("upcoming")?.unwrap_or(0),
                overdue: row.get::<_, Option<i64>>("overdue")?.unwrap_or(0),
                completed: row.get::<_, Option<i64>>("completed")?.unwrap_or(0),
                all: row.get::<_, Option<i64>>("all_open")?.unwrap_or(0),
                no_due_date: row.get::<_, Option<i64>>("no_due")?.unwrap_or(0),
            })
        },
    )
    .map_err(Into::into)
}

/// Open tasks due within the next `days`, for the tray's "Upcoming" submenu.
pub fn upcoming_for_tray(conn: &Connection, days: i64, limit: i64) -> Result<Vec<Task>> {
    let query = TaskQuery {
        completed: Some(false),
        due_before: Some(to_ts(local_day_end(Utc::now()) + Duration::days(days - 1))),
        limit: Some(limit),
        ..Default::default()
    };
    tasks(conn, &query)
}
