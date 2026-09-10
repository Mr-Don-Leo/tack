//! Small shared helpers: identifiers, timestamps and list ordering.

use chrono::{DateTime, Local, TimeZone, Utc};

use crate::models::Timestamp;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Canonical "now" for every write path, so ordering stays consistent.
pub fn now() -> Timestamp {
    to_ts(Utc::now())
}

pub fn to_ts(dt: DateTime<Utc>) -> Timestamp {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parses a stored timestamp. Returns `None` for malformed values rather than
/// panicking — a corrupt row should never take down the reminder loop.
pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Start of the local day containing `dt`, expressed in UTC.
pub fn local_day_start(dt: DateTime<Utc>) -> DateTime<Utc> {
    let local = dt.with_timezone(&Local).date_naive();
    Local
        .from_local_datetime(&local.and_hms_opt(0, 0, 0).expect("midnight is valid"))
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or(dt)
}

/// Exclusive end of the local day containing `dt`, expressed in UTC.
pub fn local_day_end(dt: DateTime<Utc>) -> DateTime<Utc> {
    local_day_start(dt) + chrono::Duration::days(1)
}

/// Gap between adjacent `position` values. Cards are ordered by a sparse float
/// so a drag only rewrites the moved row instead of renumbering the column.
pub const POSITION_STEP: f64 = 1024.0;

/// Midpoint between two neighbours, or a new end-of-list slot.
///
/// Floats lose precision after roughly 50 consecutive drops into the same gap;
/// `needs_rebalance` detects that so the caller can renumber the column.
pub fn position_between(before: Option<f64>, after: Option<f64>) -> f64 {
    match (before, after) {
        (None, None) => POSITION_STEP,
        (Some(b), None) => b + POSITION_STEP,
        (None, Some(a)) => a - POSITION_STEP,
        (Some(b), Some(a)) => (b + a) / 2.0,
    }
}

/// True when neighbours have collapsed too close to subdivide safely.
pub fn needs_rebalance(before: Option<f64>, after: Option<f64>) -> bool {
    match (before, after) {
        (Some(b), Some(a)) => (a - b).abs() < 0.0001,
        _ => false,
    }
}

/// Lowercases and collapses whitespace for case-insensitive `LIKE` matching.
pub fn normalize_search(text: &str) -> String {
    text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Escapes `%`, `_` and `\` so user text is matched literally by `LIKE`.
/// Pair with `ESCAPE '\'` in the SQL.
pub fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
