//! Natural-language parsing for Quick Add.
//!
//! Turns `Fix checkout invoice bug tomorrow 3pm !high #work` into a title, a
//! due date, a priority and a set of labels. The parser is deliberately
//! conservative: anything it does not confidently recognise stays in the title,
//! because silently eating a word out of a task name is far worse than missing
//! a date.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone};

use crate::models::{Frequency, Recurrence};

/// What Quick Add extracted from a line of text.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    pub title: String,
    /// RFC 3339, UTC.
    pub due_at: Option<String>,
    pub due_has_time: bool,
    pub priority: i64,
    /// Label names as typed; the caller resolves them against the board.
    pub labels: Vec<String>,
    /// Board name from an `@board` token, resolved by the caller.
    pub board: Option<String>,
    pub recurrence: Option<Recurrence>,
}

/// Date-only tasks are due at the end of their day, so a task due "today" is
/// not instantly overdue at midnight.
const END_OF_DAY: (u32, u32) = (23, 59);

/// Sentence punctuation stripped from the end of a token before matching.
/// `!` is excluded because it is also the priority marker.
const TRAILING: [char; 5] = [',', '.', ';', ':', '?'];

struct Scanner {
    /// Tokens exactly as typed, used to rebuild the title.
    raw: Vec<String>,
    /// Lowercased, punctuation-trimmed tokens, used for matching.
    lower: Vec<String>,
    consumed: Vec<bool>,
}

impl Scanner {
    fn new(input: &str) -> Self {
        let raw: Vec<String> = input.split_whitespace().map(str::to_string).collect();
        // Only trailing punctuation is trimmed: a leading `!`, `#` or `@` is
        // the marker that makes a token a priority, label or board.
        let lower = raw.iter().map(|t| t.trim_end_matches(TRAILING).to_lowercase()).collect();
        let consumed = vec![false; raw.len()];
        Self { raw, lower, consumed }
    }

    fn len(&self) -> usize {
        self.raw.len()
    }

    fn at(&self, i: usize) -> &str {
        self.lower.get(i).map(String::as_str).unwrap_or("")
    }

    fn free(&self, i: usize) -> bool {
        self.consumed.get(i).is_some_and(|c| !c)
    }

    fn take(&mut self, range: std::ops::Range<usize>) {
        for i in range {
            if let Some(slot) = self.consumed.get_mut(i) {
                *slot = true;
            }
        }
    }

    /// Also swallows a leading preposition, so "due on friday" leaves no "due
    /// on" behind in the title.
    fn take_with_preposition(&mut self, range: std::ops::Range<usize>) {
        let start = range.start;
        self.take(range);
        if start > 0 && matches!(self.at(start - 1), "on" | "by" | "at" | "due" | "for" | "@") {
            self.take(start - 1..start);
            if start > 1 && matches!(self.at(start - 2), "due") {
                self.take(start - 2..start - 1);
            }
        }
    }

    fn title(&self) -> String {
        self.raw
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.consumed[*i])
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    }
}

/// Parses `input` relative to `now`, which is injected so the behaviour is
/// testable and so "tomorrow" always means the user's local tomorrow.
pub fn parse(input: &str, now: DateTime<Local>) -> Parsed {
    let mut scanner = Scanner::new(input);
    let mut out = Parsed::default();

    scan_tags(&mut scanner, &mut out);
    out.recurrence = scan_recurrence(&mut scanner);
    let date = scan_date(&mut scanner, now);
    let time = scan_time(&mut scanner, now);

    out.title = scanner.title();
    // Nothing recognisable left means the user typed only modifiers; keep the
    // original text rather than creating a task with an empty name.
    if out.title.is_empty() {
        out.title = input.trim().to_string();
        out.due_at = None;
        return out;
    }

    if let Some(due) = resolve_due(date, time, now) {
        out.due_at = Some(crate::util::to_ts(due.0.with_timezone(&chrono::Utc)));
        out.due_has_time = due.1;
    }
    out
}

/// Combines an optional date and an optional time into a due instant.
///
/// A bare time means today, or tomorrow if that time has already passed —
/// typing "3pm" at 6pm means tomorrow afternoon, not three hours ago.
fn resolve_due(
    date: Option<NaiveDate>,
    time: Option<NaiveTime>,
    now: DateTime<Local>,
) -> Option<(DateTime<Local>, bool)> {
    let has_time = time.is_some();
    let (date, time) = match (date, time) {
        (Some(d), Some(t)) => (d, t),
        (Some(d), None) => (
            d,
            NaiveTime::from_hms_opt(END_OF_DAY.0, END_OF_DAY.1, 0)?,
        ),
        (None, Some(t)) => {
            let today = now.date_naive();
            let candidate = Local.from_local_datetime(&today.and_time(t)).earliest();
            match candidate {
                Some(dt) if dt > now => (today, t),
                _ => (today + Duration::days(1), t),
            }
        }
        (None, None) => return None,
    };

    let naive = date.and_time(time);
    let resolved = Local
        .from_local_datetime(&naive)
        .earliest()
        // Inside a daylight-saving gap, step forward to a time that exists.
        .or_else(|| Local.from_local_datetime(&(naive + Duration::hours(1))).earliest())?;
    Some((resolved, has_time))
}

// ------------------------------------------------------------ labels, boards

fn scan_tags(scanner: &mut Scanner, out: &mut Parsed) {
    for i in 0..scanner.len() {
        let token = scanner.raw[i].clone();

        if let Some(name) = token.strip_prefix('#')
            && !name.is_empty()
        {
            out.labels.push(clean_tag(name));
            scanner.take(i..i + 1);
            continue;
        }
        if let Some(name) = token.strip_prefix('@')
            && !name.is_empty()
        {
            out.board = Some(clean_tag(name));
            scanner.take(i..i + 1);
            continue;
        }
        if let Some(priority) = priority_token(&scanner.lower[i]) {
            out.priority = priority;
            scanner.take(i..i + 1);
        }
    }
}

fn clean_tag(name: &str) -> String {
    name.trim_end_matches(TRAILING).replace('_', " ")
}

/// `!high`, `!!`, `p1` and friends. `p1` is the highest, matching the
/// convention used by most trackers.
fn priority_token(token: &str) -> Option<i64> {
    match token {
        "!urgent" | "!!!" | "p1" => Some(4),
        "!high" | "!!" | "p2" => Some(3),
        "!medium" | "!med" | "!" | "p3" => Some(2),
        "!low" | "p4" => Some(1),
        _ => None,
    }
}

// -------------------------------------------------------------- recurrence

fn scan_recurrence(scanner: &mut Scanner) -> Option<Recurrence> {
    for i in 0..scanner.len() {
        if !scanner.free(i) {
            continue;
        }
        // Single-word forms.
        let single = match scanner.at(i) {
            "daily" => Some((Frequency::Daily, 1, vec![])),
            "weekly" => Some((Frequency::Weekly, 1, vec![])),
            "fortnightly" | "biweekly" => Some((Frequency::Weekly, 2, vec![])),
            "monthly" => Some((Frequency::Monthly, 1, vec![])),
            "yearly" | "annually" => Some((Frequency::Yearly, 1, vec![])),
            _ => None,
        };
        if let Some((freq, interval, weekdays)) = single {
            scanner.take(i..i + 1);
            return Some(build_recurrence(freq, interval, weekdays));
        }

        if scanner.at(i) != "every" {
            continue;
        }
        let next = scanner.at(i + 1).to_string();

        // "every weekday" / "every day" / "every week" / "every monday"
        if let Some((freq, weekdays)) = match next.as_str() {
            "day" => Some((Frequency::Daily, vec![])),
            "weekday" | "weekdays" => Some((Frequency::Weekdays, vec![])),
            "week" => Some((Frequency::Weekly, vec![])),
            "month" => Some((Frequency::Monthly, vec![])),
            "year" => Some((Frequency::Yearly, vec![])),
            other => weekday_index(other).map(|d| (Frequency::Weekly, vec![d])),
        } {
            scanner.take(i..i + 2);
            return Some(build_recurrence(freq, 1, weekdays));
        }

        // "every 2 weeks"
        if let Ok(interval) = next.parse::<u32>()
            && interval > 0
            && let Some(freq) = match scanner.at(i + 2) {
                "day" | "days" => Some(Frequency::Daily),
                "week" | "weeks" => Some(Frequency::Weekly),
                "month" | "months" => Some(Frequency::Monthly),
                "year" | "years" => Some(Frequency::Yearly),
                _ => None,
            }
        {
            scanner.take(i..i + 3);
            return Some(build_recurrence(freq, interval, vec![]));
        }
    }
    None
}

fn build_recurrence(freq: Frequency, interval: u32, weekdays: Vec<u8>) -> Recurrence {
    Recurrence {
        freq,
        interval,
        weekdays,
        day_of_month: None,
        until: None,
        count: None,
        occurrences: 0,
    }
}

// -------------------------------------------------------------------- dates

fn scan_date(scanner: &mut Scanner, now: DateTime<Local>) -> Option<NaiveDate> {
    let today = now.date_naive();

    for i in 0..scanner.len() {
        if !scanner.free(i) {
            continue;
        }
        let token = scanner.at(i).to_string();

        // Relative words.
        // "tonight" is left for the time scanner, which knows it means 8pm.
        let relative = match token.as_str() {
            "today" => Some(today),
            "tomorrow" | "tmr" | "tmrw" => Some(today + Duration::days(1)),
            "yesterday" => Some(today - Duration::days(1)),
            _ => None,
        };
        if let Some(date) = relative {
            scanner.take_with_preposition(i..i + 1);
            return Some(date);
        }

        // "next week" / "next month" / "next friday"
        if token == "next" && scanner.free(i + 1) {
            let follow = scanner.at(i + 1).to_string();
            let date = match follow.as_str() {
                "week" => Some(today + Duration::days(7)),
                "month" => Some(add_month(today)),
                "year" => Some(NaiveDate::from_ymd_opt(today.year() + 1, today.month(), today.day())
                    .unwrap_or(today)),
                other => weekday_index(other).map(|d| next_weekday(today, d)),
            };
            if let Some(date) = date {
                scanner.take_with_preposition(i..i + 2);
                return Some(date);
            }
        }

        // "this friday" reads the same as a bare weekday.
        if token == "this"
            && let Some(day) = weekday_index(scanner.at(i + 1))
        {
            scanner.take_with_preposition(i..i + 2);
            return Some(next_weekday(today, day));
        }

        // "in 3 days"
        if token == "in"
            && let Ok(count) = scanner.at(i + 1).parse::<i64>()
            && count >= 0
            && let Some(date) = match scanner.at(i + 2) {
                "day" | "days" => Some(today + Duration::days(count)),
                "week" | "weeks" => Some(today + Duration::weeks(count)),
                "month" | "months" => Some((0..count).fold(today, |d, _| add_month(d))),
                _ => None,
            }
        {
            scanner.take(i..i + 3);
            return Some(date);
        }

        // A bare weekday name.
        if let Some(day) = weekday_index(&token) {
            scanner.take_with_preposition(i..i + 1);
            return Some(next_weekday(today, day));
        }

        // "5 jan" / "jan 5" / "january 5th"
        if let Some(month) = month_index(&token)
            && let Some(day) = day_number(scanner.at(i + 1))
        {
            scanner.take_with_preposition(i..i + 2);
            return Some(on_or_after(today, month, day));
        }
        if let Some(day) = day_number(&token)
            && let Some(month) = month_index(scanner.at(i + 1))
        {
            scanner.take_with_preposition(i..i + 2);
            return Some(on_or_after(today, month, day));
        }

        // Numeric forms: 2026-09-12, 12/09, 12/09/2026.
        if let Some(date) = numeric_date(&token, today) {
            scanner.take_with_preposition(i..i + 1);
            return Some(date);
        }
    }
    None
}

/// The next occurrence of `weekday`, never today — "friday" typed on a Friday
/// means the Friday coming up.
fn next_weekday(today: NaiveDate, weekday: u8) -> NaiveDate {
    let current = today.weekday().num_days_from_monday() as i64;
    let target = i64::from(weekday);
    let ahead = (target - current).rem_euclid(7);
    today + Duration::days(if ahead == 0 { 7 } else { ahead })
}

/// The next date with this month and day, this year or next.
fn on_or_after(today: NaiveDate, month: u32, day: u32) -> NaiveDate {
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day);
    match this_year {
        Some(date) if date >= today => date,
        _ => NaiveDate::from_ymd_opt(today.year() + 1, month, day).unwrap_or(today),
    }
}

fn add_month(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    // Clamp so 31 January + 1 month lands on the last day of February.
    (0..=3)
        .filter_map(|back| NaiveDate::from_ymd_opt(year, month, date.day().saturating_sub(back)))
        .next()
        .unwrap_or(date)
}

/// `12`, `12th`, `3rd` — a plain day-of-month.
fn day_number(token: &str) -> Option<u32> {
    let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
    let suffix = &token[digits.len()..];
    if digits.is_empty() || !matches!(suffix, "" | "st" | "nd" | "rd" | "th") {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

fn numeric_date(token: &str, today: NaiveDate) -> Option<NaiveDate> {
    let separator = if token.contains('/') {
        '/'
    } else if token.matches('-').count() >= 2 {
        '-'
    } else {
        return None;
    };
    let parts: Vec<&str> = token.split(separator).collect();

    // ISO first: an unambiguous four-digit year leads.
    if parts.len() == 3
        && parts[0].len() == 4
        && let (Ok(y), Ok(m), Ok(d)) = (parts[0].parse(), parts[1].parse(), parts[2].parse())
    {
        return NaiveDate::from_ymd_opt(y, m, d);
    }

    // Otherwise day-first, which is what "12/09" means outside the US. This is
    // the one genuinely ambiguous case; the task editor shows the resolved date
    // so a misread is visible immediately.
    let day: u32 = parts.first()?.parse().ok()?;
    let month: u32 = parts.get(1)?.parse().ok()?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    match parts.get(2) {
        Some(year) => {
            let year: i32 = year.parse().ok()?;
            let year = if year < 100 { 2000 + year } else { year };
            NaiveDate::from_ymd_opt(year, month, day)
        }
        None => Some(on_or_after(today, month, day)),
    }
}

fn weekday_index(token: &str) -> Option<u8> {
    Some(match token {
        "monday" | "mon" => 0,
        "tuesday" | "tue" | "tues" => 1,
        "wednesday" | "wed" => 2,
        "thursday" | "thu" | "thurs" => 3,
        "friday" | "fri" => 4,
        "saturday" | "sat" => 5,
        "sunday" | "sun" => 6,
        _ => return None,
    })
}

fn month_index(token: &str) -> Option<u32> {
    Some(match token {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sep" | "sept" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    })
}

// -------------------------------------------------------------------- times

fn scan_time(scanner: &mut Scanner, _now: DateTime<Local>) -> Option<NaiveTime> {
    for i in 0..scanner.len() {
        if !scanner.free(i) {
            continue;
        }
        let token = scanner.at(i).to_string();

        if let Some(time) = match token.as_str() {
            "noon" | "midday" => NaiveTime::from_hms_opt(12, 0, 0),
            "midnight" => NaiveTime::from_hms_opt(23, 59, 0),
            "tonight" => NaiveTime::from_hms_opt(20, 0, 0),
            // These are ordinary nouns as often as they are times, so they only
            // count when they follow a date we already matched — "tomorrow
            // morning" yes, "Plan the morning routine" no.
            "morning" | "afternoon" | "evening" if i > 0 && !scanner.free(i - 1) => {
                match token.as_str() {
                    "morning" => NaiveTime::from_hms_opt(9, 0, 0),
                    "afternoon" => NaiveTime::from_hms_opt(14, 0, 0),
                    _ => NaiveTime::from_hms_opt(18, 0, 0),
                }
            }
            _ => None,
        } {
            scanner.take_with_preposition(i..i + 1);
            return Some(time);
        }

        if let Some(time) = clock_time(&token) {
            scanner.take_with_preposition(i..i + 1);
            return Some(time);
        }

        // "3 pm" written with a space.
        if matches!(scanner.at(i + 1), "am" | "pm")
            && let Some(time) = clock_time(&format!("{token}{}", scanner.at(i + 1)))
        {
            scanner.take_with_preposition(i..i + 2);
            return Some(time);
        }
    }
    None
}

/// `3pm`, `3:30pm`, `15:00`, `0930`. Bare numbers are only read as a time when
/// they carry a colon or a meridiem, so "buy 2 apples" keeps its 2.
fn clock_time(token: &str) -> Option<NaiveTime> {
    let (body, meridiem) = if let Some(rest) = token.strip_suffix("am") {
        (rest, Some(false))
    } else if let Some(rest) = token.strip_suffix("pm") {
        (rest, Some(true))
    } else {
        (token, None)
    };
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let (hour, minute) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if meridiem.is_some() => (body.parse::<u32>().ok()?, 0),
        // No colon and no am/pm: not a time.
        None => return None,
    };
    if minute > 59 {
        return None;
    }

    let hour = match meridiem {
        Some(true) if hour < 12 => hour + 12,
        Some(false) if hour == 12 => 0,
        Some(_) => hour,
        None => hour,
    };
    if hour > 23 {
        return None;
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        // Tuesday 8 September 2026, 10:00 local.
        Local.with_ymd_and_hms(2026, 9, 8, 10, 0, 0).unwrap()
    }

    fn due_local(parsed: &Parsed) -> DateTime<Local> {
        crate::util::parse_ts(parsed.due_at.as_deref().expect("a due date was parsed"))
            .expect("due date is valid")
            .with_timezone(&Local)
    }

    #[test]
    fn parses_the_example_from_the_brief() {
        let parsed = parse("Fix checkout invoice bug tomorrow 3pm", now());
        assert_eq!(parsed.title, "Fix checkout invoice bug");
        assert!(parsed.due_has_time);
        assert_eq!(due_local(&parsed), Local.with_ymd_and_hms(2026, 9, 9, 15, 0, 0).unwrap());
    }

    #[test]
    fn a_plain_title_stays_untouched() {
        let parsed = parse("Buy 2 apples", now());
        assert_eq!(parsed.title, "Buy 2 apples");
        assert!(parsed.due_at.is_none());
        assert_eq!(parsed.priority, 0);
    }

    #[test]
    fn date_without_time_is_due_at_the_end_of_its_day() {
        let parsed = parse("File the tax return friday", now());
        assert_eq!(parsed.title, "File the tax return");
        assert!(!parsed.due_has_time);
        let due = due_local(&parsed);
        assert_eq!(due.date_naive(), NaiveDate::from_ymd_opt(2026, 9, 11).unwrap());
        assert_eq!(due.time(), NaiveTime::from_hms_opt(23, 59, 0).unwrap());
    }

    #[test]
    fn a_bare_past_time_rolls_to_tomorrow() {
        // 09:00 has already gone at 10:00, so the user means tomorrow morning.
        let parsed = parse("Standup 9am", now());
        assert_eq!(parsed.title, "Standup");
        assert_eq!(due_local(&parsed), Local.with_ymd_and_hms(2026, 9, 9, 9, 0, 0).unwrap());
    }

    #[test]
    fn extracts_priority_labels_and_board() {
        let parsed = parse("Ship release !high #work #ops @Development", now());
        assert_eq!(parsed.title, "Ship release");
        assert_eq!(parsed.priority, 3);
        assert_eq!(parsed.labels, vec!["work", "ops"]);
        assert_eq!(parsed.board.as_deref(), Some("Development"));
    }

    #[test]
    fn recognises_recurrence() {
        let parsed = parse("Water the plants every 2 weeks", now());
        assert_eq!(parsed.title, "Water the plants");
        let rec = parsed.recurrence.expect("recurrence parsed");
        assert_eq!(rec.freq, Frequency::Weekly);
        assert_eq!(rec.interval, 2);
    }

    #[test]
    fn recognises_a_weekly_weekday_recurrence() {
        let parsed = parse("Team sync every monday", now());
        assert_eq!(parsed.title, "Team sync");
        let rec = parsed.recurrence.expect("recurrence parsed");
        assert_eq!(rec.freq, Frequency::Weekly);
        assert_eq!(rec.weekdays, vec![0]);
    }

    #[test]
    fn swallows_the_preposition_with_the_date() {
        let parsed = parse("Call the bank on tuesday at 2:30pm", now());
        assert_eq!(parsed.title, "Call the bank");
        assert_eq!(due_local(&parsed), Local.with_ymd_and_hms(2026, 9, 15, 14, 30, 0).unwrap());
    }

    #[test]
    fn reads_in_n_days() {
        let parsed = parse("Chase the invoice in 3 days", now());
        assert_eq!(parsed.title, "Chase the invoice");
        assert_eq!(due_local(&parsed).date_naive(), NaiveDate::from_ymd_opt(2026, 9, 11).unwrap());
    }

    #[test]
    fn reads_a_month_and_day() {
        let parsed = parse("Renew the domain 5 jan", now());
        assert_eq!(parsed.title, "Renew the domain");
        assert_eq!(due_local(&parsed).date_naive(), NaiveDate::from_ymd_opt(2027, 1, 5).unwrap());
    }

    #[test]
    fn reads_an_iso_date() {
        let parsed = parse("Audit 2026-12-01", now());
        assert_eq!(parsed.title, "Audit");
        assert_eq!(due_local(&parsed).date_naive(), NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
    }

    #[test]
    fn a_modifier_only_line_keeps_its_text() {
        let parsed = parse("tomorrow", now());
        assert_eq!(parsed.title, "tomorrow");
        assert!(parsed.due_at.is_none());
    }

    #[test]
    fn numbers_in_a_title_are_not_times() {
        let parsed = parse("Order 12 chairs", now());
        assert_eq!(parsed.title, "Order 12 chairs");
        assert!(parsed.due_at.is_none());
    }
}
