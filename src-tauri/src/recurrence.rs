//! Recurrence maths for repeating tasks and repeating reminders.
//!
//! Steps are computed in the user's local timezone and converted back to UTC,
//! so "every day at 09:00" stays at 09:00 across a daylight-saving change
//! instead of drifting by an hour.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};

use crate::models::{Frequency, Recurrence};
use crate::util::parse_ts;

/// The next occurrence strictly after `from`, or `None` once the rule has run
/// out of occurrences (`until` passed, or `count` reached).
pub fn next_after(rec: &Recurrence, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Some(count) = rec.count
        && rec.occurrences + 1 >= count
    {
        return None;
    }

    let local = from.with_timezone(&Local).naive_local();
    let interval = rec.interval.max(1) as i64;

    let next_local = match rec.freq {
        Frequency::Daily => local + Duration::days(interval),
        Frequency::Weekdays => next_weekday(local),
        Frequency::Weekly => next_weekly(local, &rec.weekdays, interval),
        Frequency::Monthly => add_months(local, interval, rec.day_of_month),
        Frequency::Yearly => add_months(local, interval * 12, rec.day_of_month),
    };

    let next = to_utc(next_local);

    if let Some(until) = rec.until.as_deref().and_then(parse_ts)
        && next > until
    {
        return None;
    }
    Some(next)
}

/// Converts a local wall-clock time back to UTC, stepping past the gap that a
/// spring-forward transition leaves in the local calendar.
fn to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    match Local.from_local_datetime(&local).earliest() {
        Some(dt) => dt.with_timezone(&Utc),
        // Inside a DST gap this wall-clock time does not exist; nudge forward.
        None => Local
            .from_local_datetime(&(local + Duration::hours(1)))
            .earliest()
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| Utc.from_utc_datetime(&local)),
    }
}

/// Monday-to-Friday: the next day that is not a weekend.
fn next_weekday(from: NaiveDateTime) -> NaiveDateTime {
    let mut next = from + Duration::days(1);
    while matches!(next.weekday(), Weekday::Sat | Weekday::Sun) {
        next += Duration::days(1);
    }
    next
}

/// Weekly recurrence, optionally restricted to specific weekdays.
///
/// With weekdays set, this walks to the next selected day; when that wraps past
/// Sunday it also skips `interval - 1` whole weeks.
fn next_weekly(from: NaiveDateTime, weekdays: &[u8], interval: i64) -> NaiveDateTime {
    if weekdays.is_empty() {
        return from + Duration::weeks(interval);
    }
    let mut selected: Vec<i64> = weekdays
        .iter()
        .filter(|d| **d <= 6)
        .map(|d| i64::from(*d))
        .collect();
    if selected.is_empty() {
        return from + Duration::weeks(interval);
    }
    selected.sort_unstable();
    selected.dedup();

    let current = from.weekday().num_days_from_monday() as i64;
    match selected.iter().find(|d| **d > current) {
        Some(next) => from + Duration::days(next - current),
        // Wrapped into a new week: jump to the first selected day of the
        // week `interval` weeks ahead.
        None => {
            let days_to_monday = 7 - current;
            from + Duration::days(days_to_monday + (interval - 1) * 7 + selected[0])
        }
    }
}

/// Adds whole months, clamping to the length of the target month so 31 January
/// plus one month lands on 28/29 February rather than overflowing.
fn add_months(from: NaiveDateTime, months: i64, day_of_month: Option<u32>) -> NaiveDateTime {
    let target_day = day_of_month.unwrap_or_else(|| from.day());
    let total = from.year() as i64 * 12 + (from.month() as i64 - 1) + months;
    let year = (total.div_euclid(12)) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    let day = target_day.min(days_in_month(year, month));

    NaiveDate::from_ymd_opt(year, month, day)
        .map(|d| d.and_time(from.time()))
        .unwrap_or(from)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let first = NaiveDate::from_ymd_opt(year, month, 1);
    let next_first = NaiveDate::from_ymd_opt(next_year, next_month, 1);
    match (first, next_first) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 28,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(freq: Frequency) -> Recurrence {
        Recurrence {
            freq,
            interval: 1,
            weekdays: vec![],
            day_of_month: None,
            until: None,
            count: None,
            occurrences: 0,
        }
    }

    fn at(s: &str) -> DateTime<Utc> {
        parse_ts(s).expect("test timestamp parses")
    }

    #[test]
    fn daily_advances_one_day() {
        let next = next_after(&rec(Frequency::Daily), at("2026-03-10T09:00:00Z")).unwrap();
        assert_eq!(next - at("2026-03-10T09:00:00Z"), Duration::days(1));
    }

    #[test]
    fn every_third_day_advances_three() {
        let mut r = rec(Frequency::Daily);
        r.interval = 3;
        let next = next_after(&r, at("2026-03-10T09:00:00Z")).unwrap();
        assert_eq!(next - at("2026-03-10T09:00:00Z"), Duration::days(3));
    }

    #[test]
    fn weekdays_skips_the_weekend() {
        // 2026-03-13 is a Friday, so the next weekday is Monday the 16th.
        let next = next_after(&rec(Frequency::Weekdays), at("2026-03-13T09:00:00Z")).unwrap();
        assert_eq!(next.with_timezone(&Local).weekday(), Weekday::Mon);
    }

    #[test]
    fn monthly_clamps_to_short_months() {
        // 31 January + 1 month has no 31st to land on.
        let from = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(add_months(from, 1, None).day(), 28);
    }

    #[test]
    fn count_limit_ends_the_series() {
        let mut r = rec(Frequency::Daily);
        r.count = Some(3);
        r.occurrences = 2;
        assert!(next_after(&r, at("2026-03-10T09:00:00Z")).is_none());
    }

    #[test]
    fn until_bound_ends_the_series() {
        let mut r = rec(Frequency::Daily);
        r.until = Some("2026-03-10T12:00:00Z".into());
        assert!(next_after(&r, at("2026-03-10T09:00:00Z")).is_none());
    }

    #[test]
    fn weekly_with_days_picks_the_next_selected_day() {
        let mut r = rec(Frequency::Weekly);
        r.weekdays = vec![0, 2, 4]; // Mon, Wed, Fri
        // 2026-03-09 is a Monday; the next selected day is Wednesday.
        let next = next_after(&r, at("2026-03-09T09:00:00Z")).unwrap();
        assert_eq!(next.with_timezone(&Local).weekday(), Weekday::Wed);
    }
}
