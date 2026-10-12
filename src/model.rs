//! The persistent on-disk data model: the types, plus load/save and
//! invariant repair (`sanitize`).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Task {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub project: Option<u64>,
    #[serde(default)]
    pub section: Option<u64>,
    #[serde(default)]
    pub due: Option<String>, // YYYY-MM-DD
    /// Optional calendar time range for tasks with a scheduled slot.
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub dash_section: DashSection,
    /// Set on habits. A recurring task never carries a due date.
    #[serde(default)]
    pub repeat: Option<Repeat>,
    /// Start date of the period `completed` refers to. `None` until the first
    /// tick stamps it.
    #[serde(default)]
    pub period: Option<String>,
    /// This task was on the dashboard when its course was archived, so
    /// un-archiving should put it back. Lives on the task rather than as a
    /// list of ids on the project, so it cannot dangle — it dies with the
    /// task it belongs to.
    #[serde(default)]
    pub relink: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RepeatUnit {
    Day,
    Week,
    Month,
}

/// A habit's cadence. `anchor` is the date the repeat was set and defines the
/// grid the ticks land on — see `due::period_start`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Repeat {
    pub every: u32,
    pub unit: RepeatUnit,
    pub anchor: String, // YYYY-MM-DD
}

impl Repeat {
    /// "daily" · "weekly" · "every 3d" · "every 2w" · "monthly"
    pub fn label(&self) -> String {
        let n = self.every.max(1);
        match (n, self.unit) {
            (1, RepeatUnit::Day) => "daily".to_string(),
            (1, RepeatUnit::Week) => "weekly".to_string(),
            (1, RepeatUnit::Month) => "monthly".to_string(),
            (n, RepeatUnit::Day) => format!("every {}d", n),
            (n, RepeatUnit::Week) => format!("every {}w", n),
            (n, RepeatUnit::Month) => format!("every {}mo", n),
        }
    }
}

/// The three fixed dashboard sections. Deliberately an enum and not a
/// `Section` record: these are fixed, non-renameable, and course-independent,
/// so an enum cannot dangle, duplicate, or need sanitizing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum DashSection {
    Important,
    #[default]
    General,
    Agent,
}

impl DashSection {
    /// Display order on the dashboard. Fixed.
    pub const ALL: [DashSection; 3] = [
        DashSection::Important,
        DashSection::General,
        DashSection::Agent,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            DashSection::Important => "Important",
            DashSection::General => "General",
            DashSection::Agent => "Agent",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Section {
    pub id: u64,
    pub project: u64,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub color: usize,
    /// Archived courses are hidden from the COURSES panel and from Tab
    /// cycling. Their tasks still exist and still count toward this course's
    /// own progress.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct LogEntry {
    pub id: u64,
    pub text: String,
    pub project: Option<u64>,
    pub project_name: String,
    pub color: usize,
    #[serde(default)]
    pub section: Option<u64>,
    #[serde(default)]
    pub section_name: String,
    pub date: String, // YYYY-MM-DD
    pub time: String, // HH:MM
    /// Back-link to the task that produced this entry. `None` on entries
    /// migrated from v2, which predate the link — see `sanitize`.
    #[serde(default)]
    pub task: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CalendarEvent {
    pub id: u64,
    pub date: String,
    pub title: String,
    pub start_time: String,
    pub end_time: String,
}

/// How many colours courses cycle through when assigned automatically.
/// Must match the length of the UI palette in `ui::theme::PALETTE`.
pub const COLOR_COUNT: usize = 6;

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Data {
    #[serde(default)]
    pub version: u32,
    pub tasks: Vec<Task>,
    pub projects: Vec<Project>,
    #[serde(default)]
    pub sections: Vec<Section>,
    pub log: Vec<LogEntry>,
    /// Ordered ids of tasks shown on the main dashboard ("pinned"/linked tasks).
    #[serde(default)]
    pub dashboard: Vec<u64>,
    #[serde(default)]
    pub calendar_events: Vec<CalendarEvent>,
    /// Yedion feed entries hidden locally by the user.
    #[serde(default)]
    pub calendar_hidden_feed: Vec<String>,
    pub daily_goal: u32,
    pub next_id: u64,
    pub next_color: usize,
}

impl Data {
    pub fn path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("taskvim")
            .join("data.json")
    }

    pub fn load() -> Data {
        let mut d: Data = fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        d.sanitize();
        d
    }

    pub fn save(&self) -> io::Result<()> {
        let p = Self::path();
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    pub fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Repair any invariant violations before the app touches this Data:
    ///   • drop projects/sections/tasks with duplicate ids (keep first);
    ///   • push `next_id` past every id actually in use so `new_id()` can
    ///     never hand out a value that already belongs to some record;
    ///   • drop task refs to projects/sections that don't exist;
    ///   • drop dangling / duplicate dashboard entries.
    /// Root cause: an earlier cloud-sync path could leave `next_id` stale,
    /// after which `new_id()` returned an id already used by an existing
    /// task — every lookup then returned the wrong (usually completed) task.
    pub fn sanitize(&mut self) {
        if self.daily_goal == 0 {
            self.daily_goal = 2;
        }
        if self.version < 2 {
            self.dashboard = self.tasks.iter().map(|t| t.id).collect();
            self.version = 2;
        }

        // v3: LogEntry gained `task`, Task gained `dash_section` + `repeat` +
        // `period`. All are `#[serde(default)]`, so migration is just the
        // version bump — no field needs touching and nothing is dropped.
        if self.version < 3 {
            self.version = 3;
        }

        // v4: Project gained `archived`, Task gained `relink`. Both are
        // `#[serde(default)]`, so migration is just the version bump.
        if self.version < 4 {
            self.version = 4;
        }

        // v5: scheduled task times and standalone calendar events are optional,
        // so existing task files gain them without a destructive migration.
        if self.version < 5 {
            self.version = 5;
        }

        // v6: locally hidden entries from the read-only Yedion calendar feed.
        if self.version < 6 {
            self.version = 6;
        }

        let mut seen = HashSet::new();
        self.projects.retain(|p| seen.insert(p.id));
        seen.clear();
        self.sections.retain(|s| seen.insert(s.id));
        seen.clear();
        self.tasks.retain(|t| seen.insert(t.id));

        let project_ids: HashSet<u64> = self.projects.iter().map(|p| p.id).collect();
        self.sections.retain(|s| project_ids.contains(&s.project));
        let section_ids: HashSet<u64> = self.sections.iter().map(|s| s.id).collect();
        for t in self.tasks.iter_mut() {
            if let Some(pid) = t.project {
                if !project_ids.contains(&pid) {
                    t.project = None;
                    t.section = None;
                }
            }
            if let Some(sid) = t.section {
                if !section_ids.contains(&sid) {
                    t.section = None;
                }
            }
        }

        let task_ids: HashSet<u64> = self.tasks.iter().map(|t| t.id).collect();
        let mut seen_dash = HashSet::new();
        self.dashboard
            .retain(|id| task_ids.contains(id) && seen_dash.insert(*id));

        let max_id = self
            .projects
            .iter()
            .map(|p| p.id)
            .chain(self.sections.iter().map(|s| s.id))
            .chain(self.tasks.iter().map(|t| t.id))
            .chain(self.log.iter().map(|l| l.id))
            .chain(self.calendar_events.iter().map(|e| e.id))
            .max()
            .unwrap_or(0);
        if self.next_id <= max_id {
            self.next_id = max_id + 1;
        }
    }

    pub fn reset() -> io::Result<()> {
        let p = Self::path();
        if p.exists() {
            let bak = p.with_extension("json.bak");
            let _ = fs::rename(&p, &bak);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_json() -> &'static str {
        r#"{
            "version": 2,
            "tasks": [{"id": 1, "text": "old task", "project": null, "section": null,
                       "due": null, "completed": false}],
            "projects": [],
            "sections": [],
            "log": [{"id": 2, "text": "old task", "project": null, "project_name": "",
                     "color": 0, "section": null, "section_name": "",
                     "date": "2026-08-01", "time": "10:00"}],
            "dashboard": [1],
            "daily_goal": 2,
            "next_id": 3,
            "next_color": 0
        }"#
    }

    #[test]
    fn v2_file_migrates_without_losing_data() {
        let mut d: Data = serde_json::from_str(v2_json()).expect("v2 json parses");
        d.sanitize();

        assert_eq!(
            d.version, 6,
            "migration runs all the way to the current version"
        );
        assert_eq!(d.tasks.len(), 1);
        assert_eq!(d.log.len(), 1);
        assert_eq!(d.dashboard, vec![1]);
        assert_eq!(d.log[0].task, None, "migrated entries have no back-link");
    }

    #[test]
    fn log_task_backlink_roundtrips_through_json() {
        let mut d: Data = serde_json::from_str(v2_json()).unwrap();
        d.sanitize();
        d.log[0].task = Some(1);

        let s = serde_json::to_string(&d).unwrap();
        let back: Data = serde_json::from_str(&s).unwrap();
        assert_eq!(back.log[0].task, Some(1));
        assert_eq!(back.version, 6);
    }

    #[test]
    fn migrates_to_v4_with_archive_fields_defaulted() {
        let mut d: Data = serde_json::from_str(v2_json()).expect("v2 json parses");
        d.sanitize();

        assert_eq!(d.version, 6);
        assert_eq!(d.tasks.len(), 1);
        assert!(!d.tasks[0].relink, "nothing is pending a re-link");
    }

    #[test]
    fn archived_and_relink_roundtrip_through_json() {
        let mut d: Data = serde_json::from_str(v2_json()).unwrap();
        d.sanitize();
        d.projects.push(Project {
            id: 9,
            name: "Statistics".into(),
            color: 0,
            archived: true,
        });
        d.tasks[0].relink = true;

        let s = serde_json::to_string(&d).unwrap();
        let back: Data = serde_json::from_str(&s).unwrap();
        assert!(back.projects[0].archived);
        assert!(back.tasks[0].relink);
        assert_eq!(back.version, 6);
    }

    #[test]
    fn dash_section_defaults_to_general_and_roundtrips() {
        let mut d: Data = serde_json::from_str(v2_json()).unwrap();
        d.sanitize();
        assert_eq!(d.tasks[0].dash_section, DashSection::General);

        d.tasks[0].dash_section = DashSection::Important;
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"important\""), "serializes lowercase: {}", s);

        let back: Data = serde_json::from_str(&s).unwrap();
        assert_eq!(back.tasks[0].dash_section, DashSection::Important);
    }

    #[test]
    fn dash_section_all_is_in_display_order() {
        assert_eq!(
            DashSection::ALL,
            [
                DashSection::Important,
                DashSection::General,
                DashSection::Agent
            ]
        );
    }
}
