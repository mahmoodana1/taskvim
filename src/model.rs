//! The persistent on-disk data model: the types, plus load/save and
//! invariant repair (`sanitize`).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Unknown JSON keys of a record, carried through load and save untouched.
pub type Extra = serde_json::Map<String, serde_json::Value>;

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

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
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

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
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

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
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

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
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

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CalendarEvent {
    pub id: u64,
    /// The first (or only) day the event happens on.
    pub date: String,
    pub title: String,
    pub start_time: String,
    pub end_time: String,
    /// A short line shown beside the title, e.g. a room.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub repeat: Option<EventRepeat>,
    /// Occurrences of a repeating event deleted one by one (YYYY-MM-DD).
    #[serde(default)]
    pub skip: Vec<String>,

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
}

/// How a calendar event repeats from its `date`, Google Calendar style.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct EventRepeat {
    pub every: u32,
    pub unit: RepeatUnit,
    /// Weekly only: the weekdays it falls on, 0 = Sunday. Empty means the
    /// weekday of the event's `date`.
    #[serde(default)]
    pub days: Vec<u32>,
    /// Last day an occurrence may fall on (YYYY-MM-DD); `None` repeats forever.
    #[serde(default)]
    pub until: Option<String>,

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Local changes to one entry of the read-only Yedion feed, keyed by its UID.
/// `None` keeps whatever the feed says.
#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct ClassEdit {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
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
    /// Local edits to Yedion feed entries.
    #[serde(default)]
    pub calendar_class_edits: Vec<ClassEdit>,
    pub daily_goal: u32,
    pub next_id: u64,
    pub next_color: usize,

    /// Fields this build does not know (newer builds, other programs, sync
    /// stamps). Kept so a save never deletes them.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Data {
    pub fn path() -> PathBuf {
        // Tests must never touch the real file: it is the user's only copy,
        // and Syncthing carries any overwrite to every other machine.
        if cfg!(test) {
            return std::env::temp_dir()
                // one file per test thread: saves merge with what is on disk
                .join(format!("taskvim-test-{}-{:?}", std::process::id(), std::thread::current().id()))
                .join("data.json");
        }
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("taskvim")
            .join("data.json")
    }

    /// The file as it is now. `Ok(None)` only when there is no file yet; a
    /// file that does not parse is an error, never an empty board: treating
    /// it as empty and saving once would wipe every machine through Syncthing.
    pub fn try_load() -> io::Result<Option<Data>> {
        let s = match fs::read_to_string(Self::path()) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut d: Data = serde_json::from_str(&s)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        d.sanitize();
        Ok(Some(d))
    }

    /// Startup load. A file caught mid-write gets a few retries; one that
    /// stays unreadable stops taskvim instead of opening (and later saving)
    /// an empty board over it.
    pub fn load() -> Data {
        for _ in 0..10 {
            match Self::try_load() {
                Ok(Some(d)) => return d,
                Ok(None) => {
                    let mut d = Data::default();
                    d.sanitize();
                    return d;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
        eprintln!(
            "taskvim: {} does not parse; not opening it so nothing overwrites it",
            Self::path().display()
        );
        std::process::exit(1);
    }

    /// Write through a temp file and a rename, so no reader (Syncthing, the
    /// subscreen agent, dojo) ever sees half a file.
    pub fn save(&self) -> io::Result<()> {
        let p = Self::path();
        let dir = p.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = dir.join(format!(".data.json.taskvim-{}", std::process::id()));
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &p)
    }

    /// Three-way merge: the changes `ours` made since `base`, applied on top
    /// of `theirs` (the file as another program left it). Records are matched
    /// by `id` in every top-level list of objects, so lists added later merge
    /// too; any other top-level value takes ours only where we changed it.
    pub fn merge(base: &Data, ours: &Data, theirs: &Data) -> Data {
        use serde_json::Value;
        let v = |d: &Data| serde_json::to_value(d).unwrap_or(Value::Null);
        let (b, o, mut t) = (v(base), v(ours), v(theirs));
        let (Some(b), Some(o), Some(tm)) = (b.as_object(), o.as_object(), t.as_object_mut())
        else {
            return ours.clone();
        };
        let keys: Vec<String> = o.keys().chain(b.keys()).cloned().collect();
        for k in keys {
            let (bk, ok) = (b.get(&k), o.get(&k));
            if bk == ok {
                continue;
            }
            let lists = (
                bk.and_then(record_list),
                ok.and_then(record_list),
                tm.get(&k).and_then(record_list),
            );
            match lists {
                (Some(bl), Some(ol), Some(mut tl)) => {
                    for (id, rec) in &bl {
                        match ol.iter().find(|(i, _)| i == id) {
                            None => tl.retain(|(i, _)| i != id),
                            Some((_, orec)) if orec != rec => replace_or_push(&mut tl, id, orec),
                            Some(_) => {}
                        }
                    }
                    for (id, orec) in &ol {
                        if !bl.iter().any(|(i, _)| i == id) {
                            replace_or_push(&mut tl, id, orec);
                        }
                    }
                    tm.insert(k, Value::Array(tl.into_iter().map(|(_, r)| r).collect()));
                }
                _ => match (bk, ok, tm.get(&k)) {
                    // A plain list both sides edited (the dashboard order):
                    // ours, plus what they added, minus what they removed.
                    (Some(Value::Array(bl)), Some(Value::Array(ol)), Some(Value::Array(tl))) => {
                        let mut out: Vec<Value> =
                            ol.iter().filter(|x| !bl.contains(x) || tl.contains(x)).cloned().collect();
                        for x in tl {
                            if !bl.contains(x) && !out.contains(x) {
                                out.push(x.clone());
                            }
                        }
                        tm.insert(k, Value::Array(out));
                    }
                    (_, Some(ov), _) => {
                        tm.insert(k, ov.clone());
                    }
                    _ => {}
                },
            }
        }
        let mut d: Data = serde_json::from_value(t).unwrap_or_else(|_| ours.clone());
        d.sanitize();
        d
    }

    /// This board without the stamps taskvim-sync adds (`_m`/`_c` on
    /// records, `_sync` on top), to tell its bookkeeping from real edits.
    pub fn without_sync_stamps(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(top) = v.as_object_mut() {
            top.remove("_sync");
            for list in top.values_mut().filter_map(|x| x.as_array_mut()) {
                for r in list.iter_mut().filter_map(|x| x.as_object_mut()) {
                    r.remove("_m");
                    r.remove("_c");
                }
            }
        }
        v
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

        // v7: events gained a note, a repeat rule and skipped dates; feed
        // entries can carry local edits. All default, so just the bump.
        if self.version < 7 {
            self.version = 7;
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

/// A list of objects that all carry an `id`, as (id, record) pairs.
fn record_list(v: &serde_json::Value) -> Option<Vec<(serde_json::Value, serde_json::Value)>> {
    let a = v.as_array()?;
    if a.is_empty() {
        return Some(Vec::new());
    }
    a.iter()
        .map(|r| Some((r.as_object()?.get("id")?.clone(), r.clone())))
        .collect()
}

fn replace_or_push(
    list: &mut Vec<(serde_json::Value, serde_json::Value)>,
    id: &serde_json::Value,
    rec: &serde_json::Value,
) {
    match list.iter_mut().find(|(i, _)| i == id) {
        Some(slot) => slot.1 = rec.clone(),
        None => list.push((id.clone(), rec.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(j: &str) -> Data {
        serde_json::from_str(j).unwrap()
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let d = data(
            r#"{"tasks":[{"id":1,"text":"a","_m":5,"future":{"x":1}}],"projects":[],"log":[],
                "daily_goal":2,"next_id":2,"next_color":0,"_sync":{"del":{}}}"#,
        );
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"_m\":5") && s.contains("\"future\"") && s.contains("\"_sync\""), "{s}");
    }

    #[test]
    fn merge_keeps_both_sides_edits() {
        let base = data(r#"{"version":7,"tasks":[{"id":1,"text":"a"},{"id":2,"text":"b"}],"projects":[],"log":[],
            "dashboard":[1,2],"daily_goal":2,"next_id":3,"next_color":0}"#);
        // we ticked 1, added 3 and dropped 2 from the dashboard
        let mut ours = base.clone();
        ours.tasks[0].completed = true;
        ours.tasks.push(data(r#"{"version":7,"tasks":[{"id":3,"text":"c"}],"projects":[],"log":[],"daily_goal":2,"next_id":4,"next_color":0}"#).tasks[0].clone());
        ours.dashboard = vec![1, 3];
        ours.next_id = 4;
        // meanwhile the phone renamed 2, added 4 and put it on the dashboard
        let theirs = data(r#"{"version":7,"tasks":[{"id":1,"text":"a"},{"id":2,"text":"B"},{"id":4,"text":"d"}],
            "projects":[],"log":[],"dashboard":[1,2,4],"daily_goal":2,"next_id":5,"next_color":0}"#);
        let m = Data::merge(&base, &ours, &theirs);
        let t = |id| m.tasks.iter().find(|t| t.id == id).unwrap();
        assert!(t(1).completed);
        assert_eq!(t(2).text, "B");
        assert_eq!(t(3).text, "c");
        assert_eq!(t(4).text, "d");
        assert_eq!(m.dashboard, vec![1, 3, 4]);
        assert!(m.next_id >= 5);
    }

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
            d.version, 7,
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
        assert_eq!(back.version, 7);
    }

    #[test]
    fn migrates_to_v4_with_archive_fields_defaulted() {
        let mut d: Data = serde_json::from_str(v2_json()).expect("v2 json parses");
        d.sanitize();

        assert_eq!(d.version, 7);
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
            extra: Default::default(),
        });
        d.tasks[0].relink = true;

        let s = serde_json::to_string(&d).unwrap();
        let back: Data = serde_json::from_str(&s).unwrap();
        assert!(back.projects[0].archived);
        assert!(back.tasks[0].relink);
        assert_eq!(back.version, 7);
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
