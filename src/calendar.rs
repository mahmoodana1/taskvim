//! Yedion iCalendar feed and shared task/event agenda helpers.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use chrono::NaiveDate;

#[derive(Clone)]
pub struct Class {
    pub id: String,
    pub title: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub room: String,
}

#[derive(Clone)]
pub enum AgendaItem {
    Class(Class),
    Task {
        id: u64,
        text: String,
        start: String,
        end: String,
    },
    Event(crate::model::CalendarEvent),
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
    Some(Class {
        id: fields.get("UID").cloned().unwrap_or_default(),
        title: fields
            .get("SUMMARY")
            .map(|v| unescape_ical(v))
            .unwrap_or_else(|| "אירוע".to_string()),
        date,
        start,
        end,
        room: fields
            .get("LOCATION")
            .map(|v| unescape_ical(v))
            .unwrap_or_default(),
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

pub fn classes_on(date: NaiveDate, schedule: &[Class], hidden: &[String]) -> Vec<Class> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut found: Vec<_> = schedule
        .iter()
        .filter(|event| event.date == key && !hidden.contains(&event.id))
        .cloned()
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

pub fn agenda_on(
    tasks: &[crate::model::Task],
    events: &[crate::model::CalendarEvent],
    schedule: &[Class],
    hidden: &[String],
    date: NaiveDate,
) -> Vec<AgendaItem> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut items = Vec::new();
    items.extend(
        classes_on(date, schedule, hidden)
            .into_iter()
            .map(AgendaItem::Class),
    );
    items.extend(tasks.iter().filter_map(|task| {
        if task.completed || task.due.as_deref() != Some(key.as_str()) {
            return None;
        }
        Some(AgendaItem::Task {
            id: task.id,
            text: task.text.clone(),
            start: task.start_time.clone()?,
            end: task.end_time.clone()?,
        })
    }));
    items.extend(
        events_on(events, date)
            .into_iter()
            .cloned()
            .map(AgendaItem::Event),
    );
    items.sort_by(|a, b| a.start().cmp(b.start()));
    items
}

pub fn events_on<'a>(
    events: &'a [crate::model::CalendarEvent],
    date: NaiveDate,
) -> Vec<&'a crate::model::CalendarEvent> {
    let key = date.format("%Y-%m-%d").to_string();
    let mut found: Vec<_> = events.iter().filter(|event| event.date == key).collect();
    found.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    found
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

pub fn parse_event_input(input: &str) -> Option<(String, String, String)> {
    let (title, token) = input.trim().rsplit_once(char::is_whitespace)?;
    if title.trim().is_empty() {
        return None;
    }
    let (start, end) = parse_time_range(token)?;
    Some((title.trim().to_string(), start, end))
}
