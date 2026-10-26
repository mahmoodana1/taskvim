//! Yedion iCalendar feed and shared task/event agenda helpers.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use chrono::{Datelike, NaiveDate};

use crate::model::{CalendarEvent, Data, EventRepeat, RepeatUnit};

#[derive(Clone)]
pub struct Class {
    pub id: String,
    pub title: String,
    pub date: String,
    pub start: String,
    pub end: String,
    /// Shown beside the title: the room code, unless edited locally.
    pub note: String,
}

#[derive(Clone)]
pub enum AgendaItem {
    Class(Class),
    Task {
        id: u64,
        text: String,
        /// The task's course name, or empty for a loose task.
        course: String,
        start: String,
        end: String,
    },
    /// One occurrence: `date` is the day it falls on, not the series start.
    Event(CalendarEvent),
}

impl AgendaItem {
    pub fn start(&self) -> &str {
        match self {
            Self::Class(item) => &item.start,
            Self::Task { start, .. } => start,
            Self::Event(item) => &item.start_time,
        }
    }
    pub fn end(&self) -> &str {
        match self {
            Self::Class(item) => &item.end,
            Self::Task { end, .. } => end,
            Self::Event(item) => &item.end_time,
        }
    }
    pub fn title(&self) -> &str {
        match self {
            Self::Class(item) => &item.title,
            Self::Task { text, .. } => text,
            Self::Event(item) => &item.title,
        }
    }
    pub fn note(&self) -> &str {
        match self {
            Self::Class(item) => &item.note,
            Self::Task { course, .. } => course,
            Self::Event(item) => &item.note,
        }
    }
    pub fn task_id(&self) -> Option<u64> {
        match self {
            Self::Task { id, .. } => Some(*id),
            _ => None,
        }
    }
}

fn cache_path() -> Option<std::path::PathBuf> {
    Some(dirs::data_dir()?.join("taskvim").join("yedion.ics"))
}

fn url_path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("taskvim").join("yedion.ics-url"))
}

fn fetch_feed(url: &str) -> Option<String> {
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "8",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
    {
        let mut stdin = child.stdin.take()?;
        writeln!(stdin, "url = \"{}\"", escaped).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

fn save_cache(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    if let Ok(mut file) = options.open(path) {
        let _ = file.write_all(content.as_bytes());
        #[cfg(unix)]
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

/// Feed entries dated before the current semester are dropped.
const FEED_START: &str = "2026-10-11";

/// Short room label for a feed LOCATION, e.g. "כיתה C-104 (מינוס) קמפוס המכללה"
/// becomes "C-104"; rooms without a code keep their text minus the boilerplate.
pub fn room_code(room: &str) -> String {
    let code = room.split_whitespace().find(|word| {
        word.starts_with(|c: char| c.is_ascii_alphabetic()) && word.contains(|c: char| c.is_ascii_digit())
    });
    match code {
        Some(code) => code.to_string(),
        None => room
            .replace("חדר לימוד :", "")
            .replace("קמפוס המכללה", "")
            .trim()
            .to_string(),
    }
}

fn current(mut feed: Vec<Class>) -> Vec<Class> {
    feed.retain(|class| class.date.as_str() >= FEED_START);
    feed
}

/// The last cached feed, read from disk only — never touches the network, so
/// startup can draw right away.
pub fn load_cached() -> Vec<Class> {
    let cached = cache_path().and_then(|path| fs::read_to_string(path).ok());
    current(cached.as_deref().map(parse_ical).unwrap_or_default())
}

/// Fetch the feed from the private URL on a background thread. The receiver
/// yields the fresh feed once it arrives, or nothing if there is no URL or
/// the fetch fails (the cached feed then stays in place).
pub fn refresh_in_background() -> Receiver<Vec<Class>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Some(url) = url_path().and_then(|path| fs::read_to_string(path).ok()) else {
            return;
        };
        let Some(content) = fetch_feed(url.trim()) else {
            return;
        };
        if content.contains("BEGIN:VCALENDAR") && content.contains("END:VCALENDAR") {
            if let Some(path) = cache_path() {
                save_cache(&path, &content);
            }
            let _ = tx.send(current(parse_ical(&content)));
        }
    });
    rx
}

fn unescape_ical(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn event_from(fields: &HashMap<String, String>) -> Option<Class> {
    if fields.get("STATUS").is_some_and(|s| s == "CANCELLED") {
        return None;
    }
    let (date, start) = parse_ical_datetime(fields.get("DTSTART")?)?;
    let (_, end) = fields
        .get("DTEND")
        .and_then(|value| parse_ical_datetime(value))
        .unwrap_or_else(|| (date.clone(), start.clone()));
    let room = fields
        .get("LOCATION")
        .map(|v| unescape_ical(v))
        .unwrap_or_default();
    Some(Class {
        id: fields.get("UID").cloned().unwrap_or_default(),
        title: fields
            .get("SUMMARY")
            .map(|v| unescape_ical(v))
            .unwrap_or_else(|| "אירוע".to_string()),
        date,
        start,
        end,
        note: room_code(&room),
    })
}

fn parse_ical_datetime(value: &str) -> Option<(String, String)> {
    let value = value.trim().trim_end_matches('Z');
    let date = NaiveDate::parse_from_str(value.get(..8)?, "%Y%m%d").ok()?;
    let time = if value.len() >= 13 {
        format!("{}:{}", value.get(9..11)?, value.get(11..13)?)
    } else {
        String::from("00:00")
    };
    Some((date.format("%Y-%m-%d").to_string(), time))
}

pub fn parse_ical(input: &str) -> Vec<Class> {
    let mut unfolded: Vec<String> = Vec::new();
    for line in input.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(previous) = unfolded.last_mut() {
                previous.push_str(line.trim_start());
            }
        } else {
            unfolded.push(line.trim_end_matches('\r').to_string());
        }
    }
    let mut events = Vec::new();
    let mut fields = None::<HashMap<String, String>>;
    for line in unfolded {
        if line == "BEGIN:VEVENT" {
            fields = Some(HashMap::new());
            continue;
        }
        if line == "END:VEVENT" {
            if let Some(record) = fields.take().and_then(|fields| event_from(&fields)) {
                events.push(record);
            }
            continue;
        }
        let Some(fields) = fields.as_mut() else {
            continue;
        };
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.split(';').next().unwrap_or(name).to_uppercase();
        fields.insert(name, value.to_string());
    }
    events.sort_by(|a, b| a.date.cmp(&b.date).then(a.start.cmp(&b.start)));
    events
}

pub fn classes_on(date: NaiveDate, data: &Data, schedule: &[Class]) -> Vec<Class> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut found: Vec<_> = schedule
        .iter()
        .filter(|event| event.date == key && !data.calendar_hidden_feed.contains(&event.id))
        .map(|class| {
            let mut class = class.clone();
            if let Some(edit) = data.calendar_class_edits.iter().find(|e| e.id == class.id) {
                let set = |field: &mut String, value: &Option<String>| {
                    if let Some(value) = value {
                        *field = value.clone();
                    }
                };
                set(&mut class.title, &edit.title);
                set(&mut class.note, &edit.note);
                set(&mut class.start, &edit.start_time);
                set(&mut class.end, &edit.end_time);
            }
            class
        })
        .collect();
    found.sort_by(|a, b| a.start.cmp(&b.start));
    found
}

pub fn all_day_tasks_on<'a>(
    tasks: &'a [crate::model::Task],
    date: NaiveDate,
) -> Vec<&'a crate::model::Task> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut found: Vec<_> = tasks
        .iter()
        .filter(|task| {
            !task.completed
                && task.due.as_deref() == Some(key.as_str())
                && (task.start_time.is_none() || task.end_time.is_none())
        })
        .collect();
    found.sort_by(|a, b| a.text.to_lowercase().cmp(&b.text.to_lowercase()));
    found
}

pub fn agenda_on(data: &Data, schedule: &[Class], date: NaiveDate) -> Vec<AgendaItem> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut items = Vec::new();
    items.extend(
        classes_on(date, data, schedule)
            .into_iter()
            .map(AgendaItem::Class),
    );
    items.extend(data.tasks.iter().filter_map(|task| {
        if task.completed || task.due.as_deref() != Some(key.as_str()) {
            return None;
        }
        Some(AgendaItem::Task {
            id: task.id,
            text: task.text.clone(),
            course: course_name(data, task.project),
            start: task.start_time.clone()?,
            end: task.end_time.clone()?,
        })
    }));
    items.extend(events_on(&data.calendar_events, date).into_iter().map(AgendaItem::Event));
    items.sort_by(|a, b| a.start().cmp(b.start()));
    items
}

/// The name of the course a task belongs to, or empty for a loose task.
pub fn course_name(data: &Data, project: Option<u64>) -> String {
    project
        .and_then(|id| data.projects.iter().find(|p| p.id == id))
        .map(|p| p.name.clone())
        .unwrap_or_default()
}

/// Does `event` (or one of its repeats) fall on `date`?
pub fn occurs_on(event: &CalendarEvent, date: NaiveDate) -> bool {
    let Ok(first) = NaiveDate::parse_from_str(&event.date, "%Y-%m-%d") else {
        return false;
    };
    let key = date.format("%Y-%m-%d").to_string();
    let Some(repeat) = &event.repeat else {
        return date == first;
    };
    if date < first || event.skip.contains(&key) {
        return false;
    }
    if repeat.until.as_deref().is_some_and(|until| key.as_str() > until) {
        return false;
    }
    let every = repeat.every.max(1) as i64;
    match repeat.unit {
        RepeatUnit::Day => (date - first).num_days() % every == 0,
        RepeatUnit::Week => {
            let sunday = |d: NaiveDate| d - chrono::Duration::days(d.weekday().num_days_from_sunday() as i64);
            let weeks = (sunday(date) - sunday(first)).num_days() / 7;
            let weekday = date.weekday().num_days_from_sunday();
            let on_day = if repeat.days.is_empty() {
                weekday == first.weekday().num_days_from_sunday()
            } else {
                repeat.days.contains(&weekday)
            };
            on_day && weeks % every == 0
        }
        // Months without that day (the 31st in April) are skipped, as
        // Google Calendar does.
        RepeatUnit::Month => {
            let months = (date.year() - first.year()) as i64 * 12 + date.month() as i64
                - first.month() as i64;
            date.day() == first.day() && months % every == 0
        }
    }
}

/// The occurrences on `date`, each dated that day.
pub fn events_on(events: &[CalendarEvent], date: NaiveDate) -> Vec<CalendarEvent> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut found: Vec<_> = events
        .iter()
        .filter(|event| occurs_on(event, date))
        .map(|event| CalendarEvent {
            date: key.clone(),
            ..event.clone()
        })
        .collect();
    found.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    found
}

const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// Parse a repeat spec: `none` · `daily` · `weekly` · `monthly` · `yearly`
/// · `3d` · `2w` · `1mo` · `sun,tue` (weekly on those days) · `2w sun,tue`.
pub fn parse_event_repeat(spec: &str) -> Result<Option<EventRepeat>, ()> {
    let s = spec.trim().to_lowercase();
    if s.is_empty() || s == "none" || s == "never" || s == "-" {
        return Ok(None);
    }
    let mut every = 1;
    let mut unit = None;
    let mut days = Vec::new();
    for word in s.split(|c: char| c == ',' || c == ':' || c.is_whitespace()) {
        if word.is_empty() || word == "on" || word == "every" {
            continue;
        }
        // Any prefix of a day name from two letters: "tu", "tue", "tuesday".
        const NAMES: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
        if let Some(day) = NAMES.iter().position(|d| word.len() >= 2 && d.starts_with(word)) {
            if !days.contains(&(day as u32)) {
                days.push(day as u32);
            }
            continue;
        }
        if unit.is_some() {
            return Err(());
        }
        let (n, u) = match word {
            "daily" | "day" => (1, RepeatUnit::Day),
            "weekly" | "week" => (1, RepeatUnit::Week),
            "monthly" | "month" => (1, RepeatUnit::Month),
            "yearly" | "year" | "annually" => (12, RepeatUnit::Month),
            _ => {
                let (num, u, scale) = if let Some(rest) = word.strip_suffix("mo") {
                    (rest, RepeatUnit::Month, 1)
                } else if let Some(rest) = word.strip_suffix('y') {
                    (rest, RepeatUnit::Month, 12)
                } else if let Some(rest) = word.strip_suffix('w') {
                    (rest, RepeatUnit::Week, 1)
                } else if let Some(rest) = word.strip_suffix('d') {
                    (rest, RepeatUnit::Day, 1)
                } else {
                    return Err(());
                };
                let n: u32 = num.parse().map_err(|_| ())?;
                (n * scale, u)
            }
        };
        if n == 0 {
            return Err(());
        }
        every = n;
        unit = Some(u);
    }
    let unit = match unit {
        Some(RepeatUnit::Week) | None if !days.is_empty() => RepeatUnit::Week,
        Some(_) if !days.is_empty() => return Err(()),
        Some(unit) => unit,
        None => return Err(()),
    };
    days.sort();
    Ok(Some(EventRepeat {
        every,
        unit,
        days,
        until: None,
    }))
}

/// The spec `parse_event_repeat` reads back: "weekly" · "2w sun,tue" · …
pub fn repeat_spec(repeat: &EventRepeat) -> String {
    let base = match (repeat.every.max(1), repeat.unit) {
        (1, RepeatUnit::Day) => "daily".to_string(),
        (1, RepeatUnit::Week) => "weekly".to_string(),
        (1, RepeatUnit::Month) => "monthly".to_string(),
        (12, RepeatUnit::Month) => "yearly".to_string(),
        (n, RepeatUnit::Day) => format!("{}d", n),
        (n, RepeatUnit::Week) => format!("{}w", n),
        (n, RepeatUnit::Month) => format!("{}mo", n),
    };
    if repeat.days.is_empty() {
        return base;
    }
    let days: Vec<_> = repeat.days.iter().filter_map(|&d| WEEKDAYS.get(d as usize).copied()).collect();
    if repeat.every.max(1) == 1 {
        days.join(",")
    } else {
        format!("{} {}", base, days.join(","))
    }
}

pub fn parse_time_range(input: &str) -> Option<(String, String)> {
    let (start, end) = input.trim().trim_start_matches('@').split_once('-')?;
    let valid = |time: &str| {
        let (hour, minute) = time.split_once(':')?;
        if hour.len() != 2 || minute.len() != 2 {
            return None;
        }
        let h: u8 = hour.parse().ok()?;
        let m: u8 = minute.parse().ok()?;
        if h > 23 || m > 59 {
            return None;
        }
        Some((h as u16) * 60 + m as u16)
    };
    let start_minutes = valid(start)?;
    let end_minutes = valid(end)?;
    if end_minutes <= start_minutes {
        return None;
    }
    Some((start.to_string(), end.to_string()))
}

/// Pull an optional trailing `HH:MM-HH:MM` (or `@HH:MM-HH:MM`) from a task.
pub fn strip_time_range(input: &str) -> (String, Option<(String, String)>) {
    let trimmed = input.trim();
    let Some((text, token)) = trimmed.rsplit_once(char::is_whitespace) else {
        return (trimmed.to_string(), None);
    };
    match parse_time_range(token) {
        Some(range) => (text.trim().to_string(), Some(range)),
        None => (trimmed.to_string(), None),
    }
}
