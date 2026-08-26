// taskvim v0.2 — a vim-motions-driven terminal task / project / course manager.
//
// Model:
//   Courses hold the full backlog, organized under sections ("subtitles").
//   The main dashboard's Tasks list contains only tasks you pin/link there
//   (p inside a course) or add manually. Completing a task anywhere creates
//   a "commit" that feeds the heatmap, streak, daily goal and course stats.
//
// Data lives in $XDG_DATA_HOME/taskvim/data.json (~/.local/share/taskvim/).
// Fully local — no cloud sync.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{Datelike, Duration as CDuration, Local, Months, NaiveDate, Timelike};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use rand::Rng;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Gauge, List, ListItem, ListState, Padding, Paragraph, Wrap,
};
use ratatui::{DefaultTerminal, Frame};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

const PALETTE: [Color; 6] = [
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
    Color::LightCyan,
];

const ACCENT: Color = Color::LightMagenta;
const DIM: Color = Color::DarkGray;
const SEL_BG: Color = Color::Rgb(45, 45, 65);

fn palette(idx: usize) -> Color {
    PALETTE[idx % PALETTE.len()]
}

// ---------------------------------------------------------------------------
// Persistent data model
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Task {
    id: u64,
    text: String,
    #[serde(default)]
    project: Option<u64>,
    #[serde(default)]
    section: Option<u64>,
    #[serde(default)]
    due: Option<String>, // YYYY-MM-DD
    #[serde(default)]
    completed: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Section {
    id: u64,
    project: u64,
    name: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Project {
    id: u64,
    name: String,
    color: usize,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct LogEntry {
    id: u64,
    text: String,
    project: Option<u64>,
    project_name: String,
    color: usize,
    #[serde(default)]
    section: Option<u64>,
    #[serde(default)]
    section_name: String,
    date: String, // YYYY-MM-DD
    time: String, // HH:MM
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
struct Data {
    #[serde(default)]
    version: u32,
    tasks: Vec<Task>,
    projects: Vec<Project>,
    #[serde(default)]
    sections: Vec<Section>,
    log: Vec<LogEntry>,
    /// Ordered ids of tasks shown on the main dashboard ("pinned"/linked tasks).
    #[serde(default)]
    dashboard: Vec<u64>,
    daily_goal: u32,
    next_id: u64,
    next_color: usize,
}

impl Data {
    fn path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("taskvim")
            .join("data.json")
    }

    fn load() -> Data {
        let mut d: Data = fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        d.sanitize();
        d
    }

    fn save(&self) -> io::Result<()> {
        let p = Self::path();
        if let Some(dir) = p.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    fn new_id(&mut self) -> u64 {
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
    fn sanitize(&mut self) {
        if self.daily_goal == 0 {
            self.daily_goal = 2;
        }
        if self.version < 2 {
            self.dashboard = self.tasks.iter().map(|t| t.id).collect();
            self.version = 2;
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
            .max()
            .unwrap_or(0);
        if self.next_id <= max_id {
            self.next_id = max_id + 1;
        }
    }

    fn reset() -> io::Result<()> {
        let p = Self::path();
        if p.exists() {
            let bak = p.with_extension("json.bak");
            let _ = fs::rename(&p, &bak);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Tasks,
    Courses,
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Dashboard,
    Course, // modal over the dashboard; cur_course holds the project id
    Log,
    DayLog, // view for a specific day's contributions
    Help,
}

/// A row inside the course modal (sections interleaved with their tasks).
#[derive(PartialEq, Clone, Copy)]
enum Row {
    Section(u64),
    NoSection,
    Task(u64),
}

#[derive(PartialEq, Clone)]
enum InsertTarget {
    DashTask,
    CourseTask { project: u64, section: Option<u64> },
    Course,
    Section { project: u64 },
    Due { task: u64 },
    RenameTask { task: u64 },
    RenameSection { section: u64 },
    RenameCourse { project: u64 },
}

#[derive(PartialEq, Clone)]
enum Mode {
    Normal,
    Insert(InsertTarget, String),
    Command(String),
    DueMenu { task: u64 },
}

struct App {
    data: Data,
    pane: Pane,
    view: View,
    mode: Mode,
    task_sel: usize,   // index into data.dashboard
    course_sel: usize, // index into data.projects
    cur_course: Option<u64>,
    course_row: usize, // cursor inside the course modal
    log_sel: usize,
    day_sel: usize,    // selected day in day log view
    selected_date: Option<String>, // YYYY-MM-DD format for day view
    year: i32,
    pending: Option<char>, // gg / dd
    status: String,
    quit: bool,
    /// Snapshot of `data` matching the last checkpoint; used to detect real changes.
    saved_snapshot: Data,
    /// Prior data states, most recent last.
    undo_stack: Vec<Data>,
    /// Undone data states, most recent last.
    redo_stack: Vec<Data>,
}

const UNDO_HISTORY_LIMIT: usize = 100;

impl App {
    fn new() -> App {
        let data = Data::load();
        let saved_snapshot = data.clone();
        App {
            data,
            pane: Pane::Tasks,
            view: View::Dashboard,
            mode: Mode::Normal,
            task_sel: 0,
            course_sel: 0,
            cur_course: None,
            course_row: 0,
            log_sel: 0,
            day_sel: 0,
            selected_date: None,
            year: Local::now().year(),
            pending: None,
            status: String::from("taskvim ready — ? help · ⏎ on a course opens it · : commands"),
            quit: false,
            saved_snapshot,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn save_quiet(&mut self) {
        self.checkpoint();
        let _ = self.data.save();
    }

    fn save(&mut self) {
        self.checkpoint();
        match self.data.save() {
            Ok(_) => self.status = format!("written to {}", Data::path().display()),
            Err(e) => self.status = format!("save failed: {}", e),
        }
    }

    /// Record the pre-mutation state on the undo stack if data actually changed.
    /// Clears the redo stack — any new edit forks history, like vim.
    fn checkpoint(&mut self) {
        if self.data == self.saved_snapshot {
            return;
        }
        let prev = std::mem::replace(&mut self.saved_snapshot, self.data.clone());
        self.undo_stack.push(prev);
        if self.undo_stack.len() > UNDO_HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        let Some(prev) = self.undo_stack.pop() else {
            self.status = String::from("nothing to undo");
            return;
        };
        let current = std::mem::replace(&mut self.data, prev.clone());
        self.redo_stack.push(current);
        self.saved_snapshot = prev;
        self.clamp();
        self.status = format!(
            "undone  ·  {} more undo · {} redo",
            self.undo_stack.len(),
            self.redo_stack.len()
        );
        let _ = self.data.save();
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            self.status = String::from("nothing to redo");
            return;
        };
        let current = std::mem::replace(&mut self.data, next.clone());
        self.undo_stack.push(current);
        self.saved_snapshot = next;
        self.clamp();
        self.status = format!(
            "redone  ·  {} more redo · {} undo",
            self.redo_stack.len(),
            self.undo_stack.len()
        );
        let _ = self.data.save();
    }

    // ------------------------------------------------------------------ stats

    fn counts_by_date(&self) -> HashMap<NaiveDate, u32> {
        let mut m: HashMap<NaiveDate, u32> = HashMap::new();
        for e in &self.data.log {
            if let Ok(d) = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                *m.entry(d).or_insert(0) += 1;
            }
        }
        m
    }

    /// (date, count) for every day that has at least one commit, most recent first.
    fn active_days_desc(&self) -> Vec<(NaiveDate, u32)> {
        let mut pairs: Vec<(NaiveDate, u32)> = self.counts_by_date().into_iter().collect();
        pairs.sort_by(|a, b| b.0.cmp(&a.0));
        pairs
    }

    fn today_count(&self) -> u32 {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        self.data.log.iter().filter(|e| e.date == today).count() as u32
    }

    fn logs_for_date(&self, date: &str) -> Vec<&LogEntry> {
        self.data.log.iter().filter(|e| e.date == date).collect()
    }

    fn streak(&self) -> u32 {
        let counts = self.counts_by_date();
        let today = Local::now().date_naive();
        let mut d = if counts.contains_key(&today) {
            today
        } else {
            match today.pred_opt() {
                Some(p) => p,
                None => return 0,
            }
        };
        let mut s = 0;
        while counts.get(&d).copied().unwrap_or(0) > 0 {
            s += 1;
            d = match d.pred_opt() {
                Some(p) => p,
                None => break,
            };
        }
        s
    }

    fn project(&self, id: u64) -> Option<&Project> {
        self.data.projects.iter().find(|p| p.id == id)
    }

    fn project_of(&self, id: Option<u64>) -> Option<&Project> {
        id.and_then(|pid| self.project(pid))
    }

    fn section(&self, id: u64) -> Option<&Section> {
        self.data.sections.iter().find(|s| s.id == id)
    }

    fn section_of(&self, id: Option<u64>) -> Option<&Section> {
        id.and_then(|sid| self.section(sid))
    }

    fn task(&self, id: u64) -> Option<&Task> {
        self.data.tasks.iter().find(|t| t.id == id)
    }

    /// (done, total) for a project: done = logged completions, total = done + pending
    fn project_stats(&self, pid: u64) -> (u32, u32) {
        let completed = self.data.tasks.iter().filter(|t| t.project == Some(pid) && t.completed).count() as u32;
        let pending = self.data.tasks.iter().filter(|t| t.project == Some(pid) && !t.completed).count() as u32;
        (completed, completed + pending)
    }

    fn overdue_count(&self, pid: u64) -> u32 {
        let today = Local::now().date_naive();
        self.data
            .tasks
            .iter()
            .filter(|t| t.project == Some(pid) && !t.completed)
            .filter(|t| {
                t.due
                    .as_deref()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .map(|d| d < today)
                    .unwrap_or(false)
            })
            .count() as u32
    }

    /// IDs of dashboard tasks that are currently visible (incomplete only).
    fn dash_ids(&self) -> Vec<u64> {
        self.data
            .dashboard
            .iter()
            .copied()
            .filter(|id| {
                self.data
                    .tasks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| !t.completed)
                    .unwrap_or(false)
            })
            .collect()
    }

    fn dash_completed_today(&self) -> u32 {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        self.data
            .log
            .iter()
            .filter(|e| e.date == today && self.data.dashboard.iter().any(|d| {
                self.data.tasks.iter().any(|t| t.id == *d && t.text == e.text)
            }))
            .count() as u32
    }

    fn is_pinned(&self, tid: u64) -> bool {
        self.data.dashboard.contains(&tid)
    }

    // ------------------------------------------------------------ course rows

    fn course_rows(&self, pid: u64) -> Vec<Row> {
        let mut rows = Vec::new();
        let has_sections = self.data.sections.iter().any(|s| s.project == pid);
        for s in self.data.sections.iter().filter(|s| s.project == pid) {
            rows.push(Row::Section(s.id));
            for t in self
                .data
                .tasks
                .iter()
                .filter(|t| t.project == Some(pid) && t.section == Some(s.id))
            {
                rows.push(Row::Task(t.id));
            }
        }
        let loose: Vec<&Task> = self
            .data
            .tasks
            .iter()
            .filter(|t| t.project == Some(pid) && t.section.is_none())
            .collect();
        if !loose.is_empty() {
            if has_sections {
                rows.push(Row::NoSection);
            }
            for t in loose {
                rows.push(Row::Task(t.id));
            }
        }
        rows
    }

    fn focus(&mut self, pid: u64, target: Row) {
        let rows = self.course_rows(pid);
        self.course_row = rows
            .iter()
            .position(|r| *r == target)
            .unwrap_or_else(|| self.course_row.min(rows.len().saturating_sub(1)));
    }

    fn clamp_course_row(&mut self, pid: u64) {
        let n = self.course_rows(pid).len();
        self.course_row = self.course_row.min(n.saturating_sub(1));
    }

    // ---------------------------------------------------------------- actions

    fn clamp(&mut self) {
        let visible = self.dash_ids().len();
        if self.task_sel >= visible {
            self.task_sel = visible.saturating_sub(1);
        }
        if self.course_sel >= self.data.projects.len() {
            self.course_sel = self.data.projects.len().saturating_sub(1);
        }
        if self.log_sel >= self.data.log.len() {
            self.log_sel = self.data.log.len().saturating_sub(1);
        }
    }

    /// Strip only a trailing `!due` token. Used by the course modal, where the
    /// course is already known — so `@course` must be treated as literal text.
    fn parse_due_suffix(&self, buf: &str) -> (String, Option<String>) {
        let mut text = buf.trim().to_string();
        if let Some(tok) = text.split_whitespace().last() {
            if let Some(spec) = tok.strip_prefix('!') {
                if let Ok(d) = parse_due(spec) {
                    let cut = text.rfind('!').unwrap();
                    text = text[..cut].trim().to_string();
                    return (text, d);
                }
            }
        }
        (text, None)
    }

    /// Strip trailing `!due` token and a trailing `@course`, in either order.
    /// Examples: "read ch.4 @algo !fri+3"  ·  "lab 2 !2026-07-10 @stat"
    fn parse_task_input(&self, buf: &str) -> (String, Option<u64>, Option<String>) {
        let mut text = buf.trim().to_string();
        let mut project = None;
        let mut due = None;
        for _ in 0..2 {
            // trailing !due token
            if due.is_none() {
                if let Some(tok) = text.split_whitespace().last() {
                    if let Some(spec) = tok.strip_prefix('!') {
                        if let Ok(d) = parse_due(spec) {
                            due = d;
                            let cut = text.rfind('!').unwrap();
                            text = text[..cut].trim().to_string();
                            continue;
                        }
                    }
                }
            }
            // trailing @course (may contain spaces: "@algorithms 2")
            if project.is_none() {
                if let Some(pos) = text.rfind('@') {
                    let name = text[pos + 1..].trim().to_lowercase();
                    if !name.is_empty() && pos > 0 {
                        if let Some(p) = self
                            .data
                            .projects
                            .iter()
                            .find(|p| p.name.to_lowercase().starts_with(&name))
                        {
                            project = Some(p.id);
                            text = text[..pos].trim().to_string();
                            continue;
                        }
                    }
                }
            }
            break;
        }
        (text, project, due)
    }

    fn add_task(
        &mut self,
        text: String,
        project: Option<u64>,
        section: Option<u64>,
        due: Option<String>,
        pin: bool,
    ) -> Option<u64> {
        if text.is_empty() {
            return None;
        }
        let id = self.data.new_id();
        self.data.tasks.push(Task {
            id,
            text,
            project,
            section,
            due,
           completed: false,
        });
        if pin {
           self.data.dashboard.push(id);
        }
        self.status = String::from("task added");
        self.save_quiet();
        Some(id)
    }

    fn add_course(&mut self, name: String) {
        if name.is_empty() {
            return;
        }
        let id = self.data.new_id();
        let color = self.data.next_color;
        self.data.next_color = (self.data.next_color + 1) % PALETTE.len();
        self.data.projects.push(Project { id, name, color });
        self.course_sel = self.data.projects.len() - 1;
        self.status = String::from("course added — ⏎ opens it");
        self.save_quiet();
    }

    fn add_section(&mut self, pid: u64, name: String) {
        if name.is_empty() {
            return;
        }
        let id = self.data.new_id();
        self.data.sections.push(Section {
            id,
            project: pid,
            name,
        });
        self.status = String::from("section added");
        self.focus(pid, Row::Section(id));
        self.save_quiet();
    }

    fn toggle_complete(&mut self, tid: u64) {
        let Some(idx) = self.data.tasks.iter().position(|t| t.id == tid) else {
            return;
        };
        let t = &self.data.tasks[idx];
        let text = t.text.clone();
        let project = t.project;
        let section = t.section;
        let was_completed = t.completed;
        
        let (pname, color) = match self.project_of(project) {
            Some(p) => (p.name.clone(), p.color),
            None => (String::new(), 0),
        };
        let sname = self
            .section_of(section)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        self.data.tasks[idx].completed = !was_completed;

        let now = Local::now();
        if !was_completed {
            // Task is being completed
            let id = self.data.new_id();
            self.data.log.push(LogEntry {
                id,
                text: text.clone(),
                project,
                project_name: pname,
                color,
                section,
                section_name: sname,
                date: now.format("%Y-%m-%d").to_string(),
                time: now.format("%H:%M").to_string(),
            });
            let done = self.today_count();
            self.status = if done >= self.data.daily_goal {
                format!(
                    "task completed — daily goal hit ({}/{})  ·  u: undo",
                    done, self.data.daily_goal
                )
            } else {
                format!(
                    "task completed ({}/{} today)  ·  u: undo",
                    done, self.data.daily_goal
                )
            };
        } else {
            // Task is being uncompleted
            self.data.log.retain(|e| !(e.text == text && e.project == project && e.section == section));
            self.status = String::from("task marked as pending");
        }
        self.clamp();
        self.save_quiet();
    }

    fn toggle_pin(&mut self, tid: u64) {
        if let Some(pos) = self.data.dashboard.iter().position(|id| *id == tid) {
            self.data.dashboard.remove(pos);
            self.status = String::from("removed from main dashboard");
        } else {
            self.data.dashboard.push(tid);
            self.status = String::from("linked to main dashboard ↗");
        }
        self.clamp();
        self.save_quiet();
    }

    fn make_priority(&mut self, tid: u64) {
        let was_linked = self.data.dashboard.iter().any(|id| *id == tid);
        self.data.dashboard.retain(|id| *id != tid);
        self.data.dashboard.insert(0, tid);
        self.task_sel = 0;
        self.status = if was_linked {
            String::from("moved to top → current priority")
        } else {
            String::from("linked to dashboard → current priority ↗")
        };
        self.clamp();
        self.save_quiet();
    }

    fn delete_task(&mut self, tid: u64) {
        if let Some(idx) = self.data.tasks.iter().position(|t| t.id == tid) {
            let t = self.data.tasks.remove(idx);
            self.data.dashboard.retain(|id| *id != tid);
            self.status = format!("deleted task: {}", t.text);
        }
        self.clamp();
        self.save_quiet();
    }

    fn delete_section(&mut self, sid: u64) {
        if let Some(idx) = self.data.sections.iter().position(|s| s.id == sid) {
            let s = self.data.sections.remove(idx);
            for t in self.data.tasks.iter_mut() {
                if t.section == Some(sid) {
                    t.section = None;
                }
            }
            self.status = format!("deleted section: {} (its tasks kept, unsectioned)", s.name);
        }
        self.save_quiet();
    }

    fn delete_course(&mut self, pid: u64) {
        if let Some(idx) = self.data.projects.iter().position(|p| p.id == pid) {
            let p = self.data.projects.remove(idx);
            self.data.sections.retain(|s| s.project != pid);
            for t in self.data.tasks.iter_mut() {
                if t.project == Some(pid) {
                    t.project = None;
                    t.section = None;
                }
            }
            self.status = format!("deleted course: {} (its tasks kept, unassigned)", p.name);
        }
        self.clamp();
        self.save_quiet();
    }

    fn pick_random(&mut self) {
        let pool: Vec<u64> = self
            .data
            .tasks
            .iter()
            .filter(|t| !t.completed)
            .map(|t| t.id)
            .collect();
        if pool.is_empty() {
            self.status = String::from("no incomplete tasks to pick from");
            return;
        }
        let tid = pool[rand::thread_rng().gen_range(0..pool.len())];
        self.data.dashboard.retain(|id| *id != tid);
        self.data.dashboard.insert(0, tid);
        self.task_sel = 0;
        self.pane = Pane::Tasks;
        self.status = String::from("random task pulled from the backlog → current priority");
        self.save_quiet();
    }

    fn move_dash_task(&mut self, delta: i32) {
        let visible = self.dash_ids();
        let len = visible.len();
        if len < 2 || self.task_sel >= len {
            return;
        }
        let target = self.task_sel as i32 + delta;
        if target < 0 || target as usize >= len {
            return;
        }
        let a_id = visible[self.task_sel];
        let b_id = visible[target as usize];
        let a = self.data.dashboard.iter().position(|id| *id == a_id);
        let b = self.data.dashboard.iter().position(|id| *id == b_id);
        if let (Some(a), Some(b)) = (a, b) {
            self.data.dashboard.swap(a, b);
            self.task_sel = target as usize;
            self.save_quiet();
        }
    }

    /// Reorder a task among its siblings (same project + section).
    fn move_task_in_section(&mut self, tid: u64, delta: i32) {
        let Some(t) = self.task(tid) else { return };
        let (pid, sid) = (t.project, t.section);
        let idxs: Vec<usize> = self
            .data
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.project == pid && t.section == sid)
            .map(|(i, _)| i)
            .collect();
        let Some(pos) = idxs.iter().position(|i| self.data.tasks[*i].id == tid) else {
            return;
        };
        let neigh = pos as i32 + delta;
        if neigh < 0 || neigh as usize >= idxs.len() {
            return;
        }
        self.data.tasks.swap(idxs[pos], idxs[neigh as usize]);
        self.save_quiet();
    }

    /// Reorder a section among the sections of the same project.
    fn move_section(&mut self, sid: u64, delta: i32) {
        let Some(pid) = self
            .data
            .sections
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.project)
        else {
            return;
        };
        let idxs: Vec<usize> = self
            .data
            .sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.project == pid)
            .map(|(i, _)| i)
            .collect();
        let Some(pos) = idxs.iter().position(|i| self.data.sections[*i].id == sid) else {
            return;
        };
        let neigh = pos as i32 + delta;
        if neigh < 0 || neigh as usize >= idxs.len() {
            return;
        }
        self.data.sections.swap(idxs[pos], idxs[neigh as usize]);
        self.save_quiet();
    }

    /// Cycle a task through: (no section) -> s1 -> s2 -> ... -> (no section)
    fn cycle_task_section(&mut self, tid: u64, delta: i32) {
        let Some(t) = self.task(tid) else { return };
        let Some(pid) = t.project else {
            self.status = String::from("task has no course — sections live inside courses");
            return;
        };
        let cur = t.section;
        let mut options: Vec<Option<u64>> = vec![None];
        options.extend(
            self.data
                .sections
                .iter()
                .filter(|s| s.project == pid)
                .map(|s| Some(s.id)),
        );
        if options.len() < 2 {
            self.status = String::from("no sections yet — press S to create one");
            return;
        }
        let pos = options.iter().position(|o| *o == cur).unwrap_or(0) as i32;
        let next =
            ((pos + delta).rem_euclid(options.len() as i32)) as usize;
        let new = options[next];
        if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) {
            t.section = new;
        }
        let name = match new {
            None => String::from("(no section)"),
            Some(sid) => self
                .data
                .sections
                .iter()
                .find(|s| s.id == sid)
                .map(|s| s.name.clone())
                .unwrap_or_default(),
        };
        self.status = format!("moved to {}", name);
        self.save_quiet();
    }

    // ---------------------------------------------------------------- insert

    fn confirm_insert(&mut self, target: InsertTarget, buf: String) {
        match target {
            InsertTarget::DashTask => {
                let (text, project, due) = self.parse_task_input(&buf);
                self.add_task(text, project, None, due, true);
            }
            InsertTarget::CourseTask { project, section } => {
                let (text, due) = self.parse_due_suffix(&buf);
                if let Some(id) = self.add_task(text, Some(project), section, due, false) {
                    self.focus(project, Row::Task(id));
                }
            }
            InsertTarget::Course => self.add_course(buf.trim().to_string()),
            InsertTarget::Section { project } => {
                self.add_section(project, buf.trim().to_string())
            }
            InsertTarget::Due { task } => match parse_due(buf.trim()) {
                Ok(due) => {
                    if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == task) {
                        t.due = due.clone();
                        self.status = match due {
                            Some(d) => format!("due date set: {}", d),
                            None => String::from("due date cleared"),
                        };
                        self.save_quiet();
                    }
                }
                Err(_) => {
                    self.status =
                        String::from("bad date — try 2026-07-10, 7-10, +3, tomorrow, or none")
                }
            },
            InsertTarget::RenameTask { task } => {
                let name = buf.trim();
                if !name.is_empty() {
                    if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == task) {
                        t.text = name.to_string();
                        self.save_quiet();
                    }
                }
            }
            InsertTarget::RenameSection { section } => {
                let name = buf.trim();
                if !name.is_empty() {
                    if let Some(s) = self.data.sections.iter_mut().find(|s| s.id == section) {
                        s.name = name.to_string();
                        self.save_quiet();
                    }
                }
            }
            InsertTarget::RenameCourse { project } => {
                let name = buf.trim();
                if !name.is_empty() {
                    if let Some(p) = self.data.projects.iter_mut().find(|p| p.id == project) {
                        p.name = name.to_string();
                        self.save_quiet();
                    }
                }
            }
        }
    }

    // ----------------------------------------------------------------- input

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('r') {
            if matches!(self.mode, Mode::Normal) {
                self.redo();
            }
            return;
        }
        match self.mode.clone() {
            Mode::Insert(target, mut buf) => match code {
                KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    self.status = String::from("insert aborted");
                }
                KeyCode::Enter => {
                    self.mode = Mode::Normal;
                    self.confirm_insert(target, buf);
                }
                KeyCode::Backspace => {
                    buf.pop();
                    self.mode = Mode::Insert(target, buf);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Insert(target, buf);
                }
                _ => {}
            },
            Mode::Command(mut buf) => match code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    self.mode = Mode::Normal;
                    self.run_command(&buf);
                }
                KeyCode::Backspace => {
                    if buf.is_empty() {
                        self.mode = Mode::Normal;
                    } else {
                        buf.pop();
                        self.mode = Mode::Command(buf);
                    }
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.mode = Mode::Command(buf);
                }
                _ => {}
            },
            Mode::DueMenu { task } => self.handle_due_menu(task, code),
            Mode::Normal => {
                let pend = self.pending.take();
                match self.view {
                    View::Help => self.view = View::Dashboard,
                    View::Log => self.handle_log(code, pend),
                    View::DayLog => self.handle_daylog(code, pend),
                    View::Course => self.handle_course(code, pend),
                    View::Dashboard => self.handle_dashboard(code, pend),
                }
            }
        }
    }

    fn handle_due_menu(&mut self, task: u64, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
                self.status = String::from("due date unchanged");
            }
            KeyCode::Char('1') | KeyCode::Char('t') => self.set_due_days(task, 0),
            KeyCode::Char('2') => self.set_due_days(task, 1),
            KeyCode::Char('3') => self.set_due_days(task, 3),
            KeyCode::Char('4') | KeyCode::Char('w') => self.set_due_days(task, 7),
            KeyCode::Char('5') => self.set_due_days(task, 14),
            KeyCode::Char('6') => self.set_due_months(task, 1),
            KeyCode::Char('7') => self.set_due_months(task, 3),
            KeyCode::Char('0') | KeyCode::Char('n') | KeyCode::Char('-') => {
                self.clear_due(task)
            }
            KeyCode::Char('c') | KeyCode::Char('i') | KeyCode::Char('/') => {
                let pre = self.task(task).and_then(|t| t.due.clone()).unwrap_or_default();
                self.mode = Mode::Insert(InsertTarget::Due { task }, pre);
            }
            _ => {}
        }
    }

    fn set_due_days(&mut self, task: u64, days: i64) {
        let d = Local::now().date_naive() + CDuration::days(days);
        self.apply_due(task, Some(d.format("%Y-%m-%d").to_string()));
    }

    fn set_due_months(&mut self, task: u64, months: u32) {
        let today = Local::now().date_naive();
        let d = today
            .checked_add_months(Months::new(months))
            .unwrap_or(today);
        self.apply_due(task, Some(d.format("%Y-%m-%d").to_string()));
    }

    fn clear_due(&mut self, task: u64) {
        self.apply_due(task, None);
    }

    fn apply_due(&mut self, task: u64, due: Option<String>) {
        if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == task) {
            t.due = due.clone();
            self.status = match due {
                Some(d) => format!("due date set: {}", d),
                None => String::from("due date cleared"),
            };
            self.save_quiet();
        }
        self.mode = Mode::Normal;
    }

    fn handle_log(&mut self, code: KeyCode, pend: Option<char>) {
        let n_days = self.active_days_desc().len();
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.log_sel + 1 < n_days {
                    self.log_sel += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => self.log_sel = self.log_sel.saturating_sub(1),
            KeyCode::Char('g') => {
                if pend == Some('g') {
                    self.log_sel = 0;
                } else {
                    self.pending = Some('g');
                }
            }
            KeyCode::Char('G') => self.log_sel = n_days.saturating_sub(1),
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char('d') | KeyCode::Enter | KeyCode::Char(' ') => {
                let days = self.active_days_desc();
                if days.is_empty() {
                    self.status = String::from("no logs to view");
                } else if let Some((date, _)) = days.get(self.log_sel.min(days.len() - 1)) {
                    self.selected_date = Some(date.format("%Y-%m-%d").to_string());
                    self.day_sel = 0;
                    self.view = View::DayLog;
                }
            }
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('o') => self.view = View::Dashboard,
            KeyCode::Char(':') => self.mode = Mode::Command(String::new()),
            KeyCode::Char('?') => self.view = View::Help,
            _ => {}
        }
    }

    fn open_course(&mut self, pid: u64) {
        self.cur_course = Some(pid);
        self.course_row = 0;
        self.view = View::Course;
        self.status = String::from(
            "a:task S:section p:link→dashboard c:priority t:due e:rename m:move dd:delete q:close",
        );
    }

    fn handle_daylog(&mut self, code: KeyCode, _pend: Option<char>) {
        let Some(date) = &self.selected_date else {
            self.view = View::Log;
            return;
        };
        let logs = self.logs_for_date(date);
        let n = logs.len();
        if n == 0 {
            return;
        }
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.day_sel + 1 < n {
                    self.day_sel += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => self.day_sel = self.day_sel.saturating_sub(1),
            KeyCode::Char('g') => {
                self.day_sel = 0;
            }
            KeyCode::Char('G') => self.day_sel = n.saturating_sub(1),
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = View::Log;
                self.selected_date = None;
            }
            KeyCode::Char('?') => self.view = View::Help,
            _ => {}
        }
    }

    fn handle_course(&mut self, code: KeyCode, pend: Option<char>) {
        let Some(pid) = self.cur_course else {
            self.view = View::Dashboard;
            return;
        };
        if self.project(pid).is_none() {
            self.view = View::Dashboard;
            return;
        }
        let rows = self.course_rows(pid);
        let cur = rows.get(self.course_row).copied();

        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = View::Dashboard;
                self.pane = Pane::Courses;
                self.status = String::from("back to dashboard");
            }
            KeyCode::Char('?') => self.view = View::Help,
            KeyCode::Char(':') => self.mode = Mode::Command(String::new()),

            // motions
            KeyCode::Char('j') | KeyCode::Down => {
                if self.course_row + 1 < rows.len() {
                    self.course_row += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.course_row = self.course_row.saturating_sub(1)
            }
            KeyCode::Char('g') => {
                if pend == Some('g') {
                    self.course_row = 0;
                } else {
                    self.pending = Some('g');
                }
            }
            KeyCode::Char('G') => self.course_row = rows.len().saturating_sub(1),
            KeyCode::Tab => {
                // cycle to the next course
                if let Some(i) = self.data.projects.iter().position(|p| p.id == pid) {
                    let next = (i + 1) % self.data.projects.len();
                    let nid = self.data.projects[next].id;
                    self.course_sel = next;
                    self.open_course(nid);
                }
            }

            // create
            KeyCode::Char('a') | KeyCode::Char('i') => {
                let section = match cur {
                    Some(Row::Section(sid)) => Some(sid),
                    Some(Row::Task(tid)) => self.task(tid).and_then(|t| t.section),
                    _ => None,
                };
                self.mode = Mode::Insert(
                    InsertTarget::CourseTask {
                        project: pid,
                        section,
                    },
                    String::new(),
                );
            }
            KeyCode::Char('S') => {
                self.mode = Mode::Insert(InsertTarget::Section { project: pid }, String::new())
            }

            // act on the current row
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(Row::Task(tid)) = cur {
                    self.toggle_complete(tid);
                    self.clamp_course_row(pid);
                }
            }
            KeyCode::Char('p') => {
                if let Some(Row::Task(tid)) = cur {
                    self.toggle_pin(tid);
                }
            }
            KeyCode::Char('c') => {
                if let Some(Row::Task(tid)) = cur {
                    self.make_priority(tid);
                }
            }
            KeyCode::Char('t') => {
                if let Some(Row::Task(tid)) = cur {
                    self.mode = Mode::DueMenu { task: tid };
                }
            }
            KeyCode::Char('e') => match cur {
                Some(Row::Task(tid)) => {
                    let pre = self.task(tid).map(|t| t.text.clone()).unwrap_or_default();
                    self.mode = Mode::Insert(InsertTarget::RenameTask { task: tid }, pre);
                }
                Some(Row::Section(sid)) => {
                    let pre = self
                        .data
                        .sections
                        .iter()
                        .find(|s| s.id == sid)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    self.mode = Mode::Insert(InsertTarget::RenameSection { section: sid }, pre);
                }
                _ => {}
            },
            KeyCode::Char('R') => {
                let pre = self.project(pid).map(|p| p.name.clone()).unwrap_or_default();
                self.mode = Mode::Insert(InsertTarget::RenameCourse { project: pid }, pre);
            }
            KeyCode::Char('d') => {
                if pend == Some('d') {
                    match cur {
                        Some(Row::Task(tid)) => self.delete_task(tid),
                        Some(Row::Section(sid)) => self.delete_section(sid),
                        _ => {}
                    }
                    self.clamp_course_row(pid);
                } else {
                    self.pending = Some('d');
                }
            }

            // move / reorder
            KeyCode::Char('J') => match cur {
                Some(Row::Task(tid)) => {
                    self.move_task_in_section(tid, 1);
                    self.focus(pid, Row::Task(tid));
                }
                Some(Row::Section(sid)) => {
                    self.move_section(sid, 1);
                    self.focus(pid, Row::Section(sid));
                }
                _ => {}
            },
            KeyCode::Char('K') => match cur {
                Some(Row::Task(tid)) => {
                    self.move_task_in_section(tid, -1);
                    self.focus(pid, Row::Task(tid));
                }
                Some(Row::Section(sid)) => {
                    self.move_section(sid, -1);
                    self.focus(pid, Row::Section(sid));
                }
                _ => {}
            },
            KeyCode::Char('m') => {
                if let Some(Row::Task(tid)) = cur {
                    self.cycle_task_section(tid, 1);
                    self.focus(pid, Row::Task(tid));
                }
            }
            KeyCode::Char('M') => {
                if let Some(Row::Task(tid)) = cur {
                    self.cycle_task_section(tid, -1);
                    self.focus(pid, Row::Task(tid));
                }
            }
            KeyCode::Char('u') => self.undo(),
            _ => {}
        }
    }

    fn handle_dashboard(&mut self, code: KeyCode, pend: Option<char>) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.view = View::Help,
            KeyCode::Char(':') => self.mode = Mode::Command(String::new()),
            KeyCode::Char('o') => {
                self.log_sel = 0;
                self.view = View::Log;
            }

            // pane movement (only use Tab)
            KeyCode::Tab => {
                self.pane = match self.pane {
                    Pane::Tasks => Pane::Courses,
                    Pane::Courses => Pane::Tasks,
                }
            }

            // list movement
            KeyCode::Char('j') | KeyCode::Down => match self.pane {
                Pane::Tasks => {
                    if self.task_sel + 1 < self.dash_ids().len() {
                        self.task_sel += 1;
                    }
                }
                Pane::Courses => {
                    if self.course_sel + 1 < self.data.projects.len() {
                        self.course_sel += 1;
                    }
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.pane {
                Pane::Tasks => self.task_sel = self.task_sel.saturating_sub(1),
                Pane::Courses => self.course_sel = self.course_sel.saturating_sub(1),
            },
            KeyCode::Char('g') => {
                if pend == Some('g') {
                    match self.pane {
                        Pane::Tasks => self.task_sel = 0,
                        Pane::Courses => self.course_sel = 0,
                    }
                } else {
                    self.pending = Some('g');
                }
            }
            KeyCode::Char('G') => match self.pane {
                Pane::Tasks => self.task_sel = self.dash_ids().len().saturating_sub(1),
                Pane::Courses => self.course_sel = self.data.projects.len().saturating_sub(1),
            },

            // reorder dashboard
            KeyCode::Char('J') => {
                if self.pane == Pane::Tasks {
                    self.move_dash_task(1)
                }
            }
            KeyCode::Char('K') => {
                if self.pane == Pane::Tasks {
                    self.move_dash_task(-1)
                }
            }

            // create
            KeyCode::Char('a') | KeyCode::Char('i') => match self.pane {
                Pane::Tasks => self.mode = Mode::Insert(InsertTarget::DashTask, String::new()),
                Pane::Courses => self.mode = Mode::Insert(InsertTarget::Course, String::new()),
            },
            KeyCode::Char('A') => {
                if let Some(p) = self.data.projects.get(self.course_sel) {
                    if self.pane == Pane::Courses {
                        self.mode = Mode::Insert(
                            InsertTarget::CourseTask {
                                project: p.id,
                                section: None,
                            },
                            String::new(),
                        );
                        return;
                    }
                }
                self.mode = Mode::Insert(InsertTarget::DashTask, String::new());
            }

            // open course modal
            KeyCode::Enter | KeyCode::Char(' ') => match self.pane {
                Pane::Tasks => {
                    if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                        self.toggle_complete(tid);
                    }
                }
                Pane::Courses => {
                    if let Some(p) = self.data.projects.get(self.course_sel) {
                        let pid = p.id;
                        self.open_course(pid);
                    }
                }
            },

            KeyCode::Char('c') => {
                if self.pane == Pane::Tasks {
                    if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                        self.make_priority(tid);
                    }
                }
            }
            KeyCode::Char('u') => self.undo(),

            // rename / due on dashboard tasks & courses
            KeyCode::Char('e') => match self.pane {
                Pane::Tasks => {
                    if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                        let pre = self.task(tid).map(|t| t.text.clone()).unwrap_or_default();
                        self.mode = Mode::Insert(InsertTarget::RenameTask { task: tid }, pre);
                    }
                }
                Pane::Courses => {
                    if let Some(p) = self.data.projects.get(self.course_sel) {
                        let (pid, pre) = (p.id, p.name.clone());
                        self.mode = Mode::Insert(InsertTarget::RenameCourse { project: pid }, pre);
                    }
                }
            },
            KeyCode::Char('t') => {
                if self.pane == Pane::Tasks {
                    if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                        let pre = self.task(tid).and_then(|t| t.due.clone()).unwrap_or_default();
                        self.mode = Mode::Insert(InsertTarget::Due { task: tid }, pre);
                    }
                }
            }

            // delete: dd unlinks course tasks / deletes loose ones; D always deletes
            KeyCode::Char('d') => {
                if pend == Some('d') {
                    match self.pane {
                        Pane::Tasks => {
                            if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                                let has_course =
                                    self.task(tid).map(|t| t.project.is_some()).unwrap_or(false);
                                if has_course {
                                    self.data.dashboard.retain(|id| *id != tid);
                                    self.status = String::from(
                                        "unlinked from dashboard (still in its course) — D deletes for real",
                                    );
                                    self.clamp();
                                    self.save_quiet();
                                } else {
                                    self.delete_task(tid);
                                }
                            }
                        }
                        Pane::Courses => {
                            if let Some(p) = self.data.projects.get(self.course_sel) {
                                let pid = p.id;
                                self.delete_course(pid);
                            }
                        }
                    }
                } else {
                    self.pending = Some('d');
                }
            }
            KeyCode::Char('D') => {
                if self.pane == Pane::Tasks {
                    if let Some(tid) = self.dash_ids().get(self.task_sel).copied() {
                        self.delete_task(tid);
                    }
                }
            }

            // misc
            KeyCode::Char('r') => self.pick_random(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.data.daily_goal += 1;
                self.status = format!("daily goal: {} tasks/day", self.data.daily_goal);
                self.save_quiet();
            }
            KeyCode::Char('-') => {
                if self.data.daily_goal > 1 {
                    self.data.daily_goal -= 1;
                }
                self.status = format!("daily goal: {} tasks/day", self.data.daily_goal);
                self.save_quiet();
            }
            KeyCode::Char('[') => self.year -= 1,
            KeyCode::Char(']') => {
                if self.year < Local::now().year() {
                    self.year += 1;
                }
            }
            _ => {}
        }
    }

    fn run_command(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        let (head, arg) = match cmd.split_once(' ') {
            Some((h, a)) => (h, a.trim()),
            None => (cmd, ""),
        };
        match head {
            "q" | "quit" => self.quit = true,
            "w" | "write" => self.save(),
            "wq" | "x" => {
                self.save();
                self.quit = true;
            }
            "log" | "history" => {
                self.log_sel = 0;
                self.view = View::Log;
            }
            "db" | "dashboard" => self.view = View::Dashboard,
            "help" | "h" => self.view = View::Help,
            "goal" => match arg.parse::<u32>() {
                Ok(n) if n >= 1 => {
                    self.data.daily_goal = n;
                    self.status = format!("daily goal: {} tasks/day", n);
                    self.save_quiet();
                }
                _ => self.status = String::from("usage: :goal <n>"),
            },
            "year" => match arg.parse::<i32>() {
                Ok(y) => self.year = y,
                Err(_) => self.status = String::from("usage: :year <yyyy>"),
            },
            "clearlog" => {
                self.data.log.clear();
                self.log_sel = 0;
                self.status = String::from("completed history cleared");
                self.save_quiet();
            }
            "reset" => {
                match Data::reset() {
                    Ok(_) => {
                        self.data = Data::default();
                        self.data.sanitize();
                        self.saved_snapshot = self.data.clone();
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                        self.task_sel = 0;
                        self.course_sel = 0;
                        self.log_sel = 0;
                        self.cur_course = None;
                        self.view = View::Dashboard;
                        let _ = self.data.save();
                        self.status = String::from(
                            "wiped local data — previous file kept as data.json.bak",
                        );
                    }
                    Err(e) => self.status = format!("reset failed: {}", e),
                }
            }
            "" => {}
            other => self.status = format!("not a taskvim command: :{}", other),
        }
    }
}


// ---------------------------------------------------------------------------
// Due-date parsing & display
// ---------------------------------------------------------------------------

/// Accepts: "" / none / today / tomorrow / +N / YYYY-MM-DD / MM-DD / M/D
fn parse_due(spec: &str) -> Result<Option<String>, ()> {
    let s = spec.trim().to_lowercase();
    let today = Local::now().date_naive();
    if s.is_empty() || s == "none" || s == "clear" || s == "-" {
        return Ok(None);
    }
    if s == "today" {
        return Ok(Some(today.format("%Y-%m-%d").to_string()));
    }
    if s == "tomorrow" || s == "tmr" {
        return Ok(Some((today + CDuration::days(1)).format("%Y-%m-%d").to_string()));
    }
    // Bare unit shortcuts: "w" = +1 week, "mo" = +1 month, "d" = +1 day, "y" = +1 year.
    let bare = match s.as_str() {
        "d" => Some(today + CDuration::days(1)),
        "w" => Some(today + CDuration::days(7)),
        "mo" | "m" => today.checked_add_months(Months::new(1)),
        "y" => today.checked_add_months(Months::new(12)),
        _ => None,
    };
    if let Some(date) = bare {
        return Ok(Some(date.format("%Y-%m-%d").to_string()));
    }
    // Relative offsets: +N, +Nd, +Nw, +Nmo, +Ny (leading '+' optional for these units).
    let stripped = s.strip_prefix('+').unwrap_or(&s);
    let try_relative = |stripped: &str| -> Option<NaiveDate> {
        let (num_str, unit) = if let Some(rest) = stripped.strip_suffix("mo") {
            (rest, "mo")
        } else if let Some(rest) = stripped.strip_suffix('w') {
            (rest, "w")
        } else if let Some(rest) = stripped.strip_suffix('d') {
            (rest, "d")
        } else if let Some(rest) = stripped.strip_suffix('y') {
            (rest, "y")
        } else {
            (stripped, "d")
        };
        let n: i64 = num_str.parse().ok()?;
        match unit {
            "d" => Some(today + CDuration::days(n)),
            "w" => Some(today + CDuration::days(n * 7)),
            "mo" => {
                if n >= 0 {
                    today.checked_add_months(Months::new(n as u32))
                } else {
                    today.checked_sub_months(Months::new((-n) as u32))
                }
            }
            "y" => {
                if n >= 0 {
                    today.checked_add_months(Months::new((n as u32) * 12))
                } else {
                    today.checked_sub_months(Months::new(((-n) as u32) * 12))
                }
            }
            _ => None,
        }
    };
    if s.starts_with('+') {
        return match try_relative(stripped) {
            Some(d) => Ok(Some(d.format("%Y-%m-%d").to_string())),
            None => Err(()),
        };
    }
    // Relative without '+' — only if it ends in a unit letter (avoid clobbering "12-25").
    if s.ends_with('w') || s.ends_with("mo") || s.ends_with('d') || s.ends_with('y') {
        if let Some(d) = try_relative(stripped) {
            return Ok(Some(d.format("%Y-%m-%d").to_string()));
        }
    }
    let norm = s.replace('/', "-");
    let parts: Vec<&str> = norm.split('-').collect();
    let (y, m, d) = match parts.len() {
        3 => (
            parts[0].parse::<i32>().map_err(|_| ())?,
            parts[1].parse::<u32>().map_err(|_| ())?,
            parts[2].parse::<u32>().map_err(|_| ())?,
        ),
        2 => (
            today.year(),
            parts[0].parse::<u32>().map_err(|_| ())?,
            parts[1].parse::<u32>().map_err(|_| ())?,
        ),
        _ => return Err(()),
    };
    match NaiveDate::from_ymd_opt(y, m, d) {
        Some(date) => Ok(Some(date.format("%Y-%m-%d").to_string())),
        None => Err(()),
    }
}

fn due_span(due: &Option<String>) -> Option<Span<'static>> {
    let d = NaiveDate::parse_from_str(due.as_deref()?, "%Y-%m-%d").ok()?;
    let today = Local::now().date_naive();
    let (txt, style) = if d < today {
        (
            format!("  ⚠ was due {}", d.format("%b %d")),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )
    } else if d == today {
        (
            String::from("  ● due today"),
            Style::default().fg(Color::LightYellow),
        )
    } else if d == today + CDuration::days(1) {
        (
            String::from("  due tomorrow"),
            Style::default().fg(Color::Yellow),
        )
    } else {
        (
            format!("  due {}", d.format("%b %d")),
            Style::default().fg(DIM),
        )
    };
    Some(Span::styled(txt, style))
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &App) {
    let root = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
    let body = root[0];
    let statusbar = root[1];

    match app.view {
        View::Help => draw_help(f, body),
        View::Log => draw_log(f, app, body),
        View::DayLog => draw_daylog(f, app, body),
        View::Dashboard => draw_dashboard(f, app, body),
        View::Course => {
            draw_dashboard(f, app, body);
            draw_course_modal(f, app, body);
        }
    }

    draw_statusbar(f, app, statusbar);

    if let Mode::Insert(target, buf) = &app.mode {
        draw_input_popup(f, target, buf);
    }
    if let Mode::DueMenu { task } = &app.mode {
        draw_due_menu(f, app, *task);
    }
}

fn draw_due_menu(f: &mut Frame, app: &App, task: u64) {
    let today = Local::now().date_naive();
    let d_days = |n: i64| (today + CDuration::days(n)).format("%b %d").to_string();
    let d_months = |n: u32| {
        today
            .checked_add_months(Months::new(n))
            .unwrap_or(today)
            .format("%b %d")
            .to_string()
    };
    let current = app
        .task(task)
        .and_then(|t| t.due.clone())
        .unwrap_or_else(|| String::from("(none)"));

    let key_style = Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    let row = |k: &'static str, label: &'static str, when: String| {
        Line::from(vec![
            Span::styled(format!("  [{}]  ", k), key_style),
            Span::raw(label),
            Span::styled(format!("   → {}", when), dim),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  currently: {}", current),
            Style::default().fg(DIM),
        )),
        Line::from(""),
        row("1", "Today       ", d_days(0)),
        row("2", "Tomorrow    ", d_days(1)),
        row("3", "In 3 days   ", d_days(3)),
        row("4", "In 1 week   ", d_days(7)),
        row("5", "In 2 weeks  ", d_days(14)),
        row("6", "In 1 month  ", d_months(1)),
        row("7", "In 3 months ", d_months(3)),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [0]  ", key_style),
            Span::raw("Clear due date"),
        ]),
        Line::from(vec![
            Span::styled("  [c]  ", key_style),
            Span::raw("Custom…  "),
            Span::styled(
                "(YYYY-MM-DD · +3d · +2w · +1mo · tomorrow · none)",
                Style::default().fg(DIM),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Esc / q  cancel",
            Style::default().fg(DIM),
        )),
    ];

    let area = f.area();
    let w = 58u16.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightGreen))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " Due date — pick a preset ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Length(40), Constraint::Min(30)]).split(area);
    draw_task_sidebar(f, app, cols[0]);

    let main = Layout::vertical([
        Constraint::Length(3),  // greeting
        Constraint::Length(6),  // priority + daily goal (reduced)
        Constraint::Length(10), // heatmap (reduced)
        Constraint::Min(10),    // courses (increased minimum)
    ])
    .split(cols[1]);

    draw_greeting(f, main[0]);

    let cards =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(main[1]);
    draw_priority(f, app, cards[0]);
    draw_daily_goal(f, app, cards[1]);

    draw_heatmap(f, app, main[2]);
    draw_courses(f, app, main[3]);
}

fn draw_greeting(f: &mut Frame, area: Rect) {
    let now = Local::now();
    let greeting = match now.hour() {
        5..=11 => "Good Morning",
        12..=17 => "Good Afternoon",
        _ => "Good Evening",
    };
    let date = now.format("%A, %B %d").to_string().to_uppercase();
    let text = vec![
        Line::from(Span::styled(date, Style::default().fg(DIM))),
        Line::from(Span::styled(
            greeting,
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    f.render_widget(
        Paragraph::new(text).block(Block::default().padding(Padding::horizontal(1))),
        area,
    );
}

fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

fn task_line(app: &App, t: &Task, show_pin: bool) -> ListItem<'static> {
    let mut spans = vec![];
    let style = if t.completed {
        Style::default().fg(DIM).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    match app.project_of(t.project) {
        Some(p) => {
            spans.push(Span::styled("▎", Style::default().fg(palette(p.color))));
            let proj_style = if t.completed {
                Style::default().fg(DIM).add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(palette(p.color))
            };
            const PROJ_CAP: usize = 14;
            const SECT_CAP: usize = 14;
            let pname = ellipsize(&p.name, PROJ_CAP);
            let label = match app.section_of(t.section) {
                Some(s) => format!("[{} - {}] ", pname, ellipsize(&s.name, SECT_CAP)),
                None => format!("[{}] ", pname),
            };
            spans.push(Span::styled(label, proj_style));
        }
        None => spans.push(Span::styled("▎", Style::default().fg(DIM))),
    }
    let text = if t.completed {
        format!("✓ {}", t.text)
    } else {
        t.text.clone()
    };
    spans.push(Span::styled(text, style));
    if show_pin && app.is_pinned(t.id) {
        spans.push(Span::styled("  ↗", Style::default().fg(Color::LightCyan)));
    }
    let mut lines = vec![Line::from(spans)];
    if !t.completed {
        if let Some(s) = due_span(&t.due) {
            // Second line so long titles never collide with the due badge.
            lines.push(Line::from(vec![Span::raw(" "), s]));
        }
    }
    ListItem::new(lines)
}

fn draw_task_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Tasks && app.view == View::Dashboard;
    let border = if active { ACCENT } else { DIM };
    let visible = app.dash_ids();
    let done_today = app.dash_completed_today();
    let footer = if done_today > 0 {
        format!(
            " a:add ⏎:done t:due dd:unlink r:random · {} done today (:log) ",
            done_today
        )
    } else {
        " a:add ⏎:done t:due dd:unlink r:random ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" Tasks — dashboard ({}) ", visible.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(DIM))));

    let items: Vec<ListItem> = visible
        .iter()
        .filter_map(|id| app.task(*id))
        .map(|t| task_line(app, t, false))
        .collect();

    if items.is_empty() {
        let msg = if done_today > 0 {
            format!(
                "\n  all clear ✓\n\n  {} completed today.\n  :log for history\n\n  a to add a task\n  r for a random one",
                done_today
            )
        } else {
            "\n  dashboard is clear —\n\n  a       add a task here\n  ⏎ on a course, then p\n          to link its tasks\n  r       pull a random one\n          from the backlog".to_string()
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(DIM)).block(block);
        f.render_widget(empty, area);
        return;
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    if active {
        state.select(Some(app.task_sel.min(visible.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_priority(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " CURRENT PRIORITY ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));

    let first = app.dash_ids().first().and_then(|id| app.task(*id));
    let lines = match first {
        Some(t) => {
            let name = match app.project_of(t.project) {
                Some(p) => match app.section_of(t.section) {
                    Some(s) => format!("[{} - {}] {}", p.name, s.name, t.text),
                    None => format!("[{}] {}", p.name, t.text),
                },
                None => t.text.clone(),
            };
            let mut title_spans = vec![Span::styled(
                name,
                Style::default().add_modifier(Modifier::BOLD),
            )];
            if let Some(s) = due_span(&t.due) {
                title_spans.push(s);
            }
            vec![
                Line::from(""),
                Line::from(title_spans).alignment(Alignment::Center),
                Line::from(Span::styled(
                    "Stay strictly on this task until completion.",
                    Style::default().fg(DIM),
                ))
                .alignment(Alignment::Center),
                Line::from(""),
                Line::from(Span::styled(
                    "[ c  Complete Task ]",
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center),
            ]
        }
        None => vec![
            Line::from(""),
            Line::from(Span::styled(
                "Dashboard clear ✓",
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            Line::from(Span::styled(
                "open a course (⏎) and press p to link a task here",
                Style::default().fg(DIM),
            ))
            .alignment(Alignment::Center),
        ],
    };

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        area,
    );
}

fn draw_daily_goal(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            " DAILY GOAL ",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " +/- adjust ",
            Style::default().fg(DIM),
        )));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let done = app.today_count();
    let goal = app.data.daily_goal.max(1);
    let ratio = (done as f64 / goal as f64).min(1.0);
    let streak = app.streak();

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner.inner(Margin {
        horizontal: 1,
        vertical: 1,
    }));

    let pct = Line::from(vec![
        Span::styled(
            format!("{:.0}% ", ratio * 100.0),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}/{}", done, goal), Style::default().fg(DIM)),
        Span::raw("   "),
        Span::styled(
            format!("🔥 {} day streak", streak),
            Style::default().fg(if streak > 0 {
                Color::LightYellow
            } else {
                DIM
            }),
        ),
    ]);
    f.render_widget(Paragraph::new(pct), rows[0]);

    let gauge = Gauge::default()
        .ratio(ratio)
        .use_unicode(true)
        .gauge_style(
            Style::default()
                .fg(Color::LightGreen)
                .bg(Color::Rgb(40, 40, 55)),
        )
        .label("");
    f.render_widget(gauge, rows[1]);

    let msg = if done == 0 {
        "Let's get started."
    } else if done < goal {
        "You're on your way!"
    } else {
        "Goal complete — keep the streak alive ✦"
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("Goal: {} tasks/day", goal),
            Style::default().fg(DIM),
        )),
        rows[2],
    );
    f.render_widget(
        Paragraph::new(Span::styled(msg, Style::default().fg(DIM))),
        rows[3],
    );
}

// ------------------------------------------------------------------- heatmap

fn heat_cell(count: u32, is_today: bool, month: u32, is_month_start: bool) -> Span<'static> {
    // Alternate empty-cell tone by month to produce GitHub-style banding.
    let dim_a = Color::Rgb(48, 50, 64);
    let dim_b = Color::Rgb(72, 74, 92);
    let empty_color = if month % 2 == 0 { dim_a } else { dim_b };
    let (ch, color) = match count {
        0 => ("·", empty_color),
        1 => ("■", Color::Rgb(60, 100, 60)),
        2 => ("■", Color::Rgb(60, 150, 60)),
        3 => ("■", Color::Rgb(80, 200, 80)),
        _ => ("■", Color::Rgb(120, 255, 120)),
    };
    let mut style = Style::default().fg(color);
    if is_month_start {
        // Left-border on the first day of each month makes month starts pop.
        style = style.add_modifier(Modifier::BOLD);
    }
    if is_today {
        style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    Span::styled(ch, style)
}

fn draw_heatmap(f: &mut Frame, app: &App, area: Rect) {
    let counts = app.counts_by_date();
    let year = app.year;
    let today = Local::now().date_naive();

    let jan1 = match NaiveDate::from_ymd_opt(year, 1, 1) {
        Some(d) => d,
        None => return,
    };
    let dec31 = NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(jan1);
    let offset = jan1.weekday().num_days_from_monday() as i64; // Mon=0
    let days_in_year = (dec31 - jan1).num_days() + 1;
    let weeks = ((offset + days_in_year) as usize + 6) / 7;

    let mut total = 0u32;
    let mut active: HashSet<NaiveDate> = HashSet::new();
    for (d, c) in &counts {
        if d.year() == year {
            total += c;
            active.insert(*d);
        }
    }

    // Precompute the first week of each month so we can drop a divider column there.
    // Skip January (week 0) since it's already the left edge.
    let mut month_start_weeks: HashSet<usize> = HashSet::new();
    for m in 2..=12u32 {
        if let Some(first) = NaiveDate::from_ymd_opt(year, m, 1) {
            let w = ((offset + (first - jan1).num_days()) / 7) as usize;
            if w < weeks {
                month_start_weeks.insert(w);
            }
        }
    }

    // Effective column position of week `w` in the rendered grid (accounts for dividers).
    let col_of_week = |w: usize| -> usize {
        w + month_start_weeks.iter().filter(|mw| **mw <= w).count()
    };

    let mut month_row = vec![Span::raw("     ")];
    let mut cursor = 0usize;
    for m in 1..=12 {
        if let Some(first) = NaiveDate::from_ymd_opt(year, m, 1) {
            let w = ((offset + (first - jan1).num_days()) / 7) as usize;
            if w < weeks {
                let col = col_of_week(w);
                if col >= cursor {
                    month_row.push(Span::raw(" ".repeat(col - cursor)));
                    let label = first.format("%b").to_string();
                    cursor = col + label.len();
                    month_row.push(Span::styled(label, Style::default().fg(DIM)));
                }
            }
        }
    }

    let mut lines: Vec<Line> = vec![Line::from(month_row)];
    for row in 0..7u32 {
        let gutter = match row {
            0 => "Mon  ",
            2 => "Wed  ",
            4 => "Fri  ",
            _ => "     ",
        };
        let mut spans = vec![Span::styled(gutter, Style::default().fg(DIM))];
        for w in 0..weeks {
            if month_start_weeks.contains(&w) {
                // Thin divider column that separates months.
                spans.push(Span::styled("│", Style::default().fg(DIM)));
            }
            let day_index = w as i64 * 7 + row as i64 - offset;
            if day_index < 0 || day_index >= days_in_year {
                spans.push(Span::raw(" "));
                continue;
            }
            let date = jan1 + CDuration::days(day_index);
            let c = counts.get(&date).copied().unwrap_or(0);
            let is_month_start = date.day() == 1;
            spans.push(heat_cell(c, date == today, date.month(), is_month_start));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(vec![
        Span::styled(
            format!("     {} contributions in {}", total, year),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ·  {} active days", active.len()),
            Style::default().fg(DIM),
        ),
        Span::styled("    less ", Style::default().fg(DIM)),
        heat_cell(0, false, 1, false),
        heat_cell(1, false, 1, false),
        heat_cell(2, false, 1, false),
        heat_cell(3, false, 1, false),
        heat_cell(4, false, 1, false),
        Span::styled(" more", Style::default().fg(DIM)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            format!(" CONTRIBUTION TRACKER — {} ", year),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " [ / ] change year · o: history log · u: undo last ",
            Style::default().fg(DIM),
        )));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_courses(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Courses && app.view == View::Dashboard;

    let n_projects = app.data.projects.len();
    let inner_h = area.height.saturating_sub(2) as usize;
    let max_visible = inner_h / 2;
    let hidden = n_projects.saturating_sub(max_visible);

    let title = if hidden > 0 {
        format!(" Courses & Projects  (+{} hidden) ", hidden)
    } else {
        String::from(" Courses & Projects ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { ACCENT } else { DIM }))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " ⏎:open a:new course A:quick task e:rename dd:delete ",
            Style::default().fg(DIM),
        )));

    if app.data.projects.is_empty() {
        f.render_widget(
            Paragraph::new("\n no courses yet — press a to create one")
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .data
        .projects
        .iter()
        .map(|p| {
            let (done, total) = app.project_stats(p.id);
            let overdue = app.overdue_count(p.id);
            let pct = if total == 0 {
                0.0
            } else {
                done as f64 / total as f64
            };
            let color = palette(p.color);

            let bar_w = 18usize;
            let filled = (pct * bar_w as f64).round() as usize;
            let bar: String = "█".repeat(filled) + &"─".repeat(bar_w.saturating_sub(filled));

            let name_line = Line::from(vec![
                Span::styled("  ● ", Style::default().fg(color)),
                Span::styled(
                    p.name.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]);

            let mut stats_spans = vec![
                Span::styled("      ", Style::default()),
                Span::styled(
                    format!("{}/{}", done, total),
                    Style::default().fg(DIM),
                ),
                Span::styled("  ", Style::default()),
                Span::styled(bar, Style::default().fg(color)),
            ];
            if overdue > 0 {
                stats_spans.push(Span::styled(
                    format!("  ⚠{}", overdue),
                    Style::default().fg(Color::LightRed),
                ));
            }
            let stats_line = Line::from(stats_spans);

            ListItem::new(vec![name_line, stats_line])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));

    let mut state = ListState::default();
    if active {
        state.select(Some(app.course_sel.min(app.data.projects.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut state);
}

// -------------------------------------------------------------- course modal

fn draw_course_modal(f: &mut Frame, app: &App, area: Rect) {
    let Some(pid) = app.cur_course else { return };
    let Some(p) = app.project(pid) else { return };
    let color = palette(p.color);
    let (done, total) = app.project_stats(pid);

    let w = area.width.saturating_sub(8).min(90).max(40);
    let h = area.height.saturating_sub(4).min(32).max(12);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            format!(" {} — {}/{} goals done ", p.name, done, total),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " a:task S:section ⏎:done p:link↗ t:due e:rename m:move J/K:reorder dd:del Tab:next q:close ",
            Style::default().fg(DIM),
        )));

    let rows = app.course_rows(pid);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(
                "\n  empty course —\n\n  S   add a section (e.g. Lectures, Labs, Assignments)\n  a   add a task",
            )
            .style(Style::default().fg(DIM))
            .block(block),
            popup,
        );
        return;
    }

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            Row::Section(sid) => {
                let s = app.data.sections.iter().find(|s| s.id == *sid);
                let name = s.map(|s| s.name.clone()).unwrap_or_default();
                let n = app
                    .data
                    .tasks
                    .iter()
                    .filter(|t| t.section == Some(*sid))
                    .count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("▸ {}", name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  ({})", n), Style::default().fg(DIM)),
                ]))
            }
            Row::NoSection => ListItem::new(Line::from(Span::styled(
                "▸ (no section)",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            ))),
            Row::Task(tid) => {
                let Some(t) = app.task(*tid) else {
                    return ListItem::new(Line::from(""));
                };
                let mut spans = vec![Span::raw("   ")];
                let style = if t.completed {
                    Style::default().fg(DIM).add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                if app.is_pinned(*tid) {
                    spans.push(Span::styled("↗ ", Style::default().fg(Color::LightCyan)));
                } else {
                    spans.push(Span::styled("○ ", style));
                }
                let text = if t.completed {
                    format!("✓ {}", t.text)
                } else {
                    t.text.clone()
                };
                spans.push(Span::styled(text, style));
                let mut lines = vec![Line::from(spans)];
                if !t.completed {
                    if let Some(s) = due_span(&t.due) {
                        // Indent to line up under the task text, past the bullet marker.
                        lines.push(Line::from(vec![Span::raw("     "), s]));
                    }
                }
                ListItem::new(lines)
            }
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.course_row.min(rows.len().saturating_sub(1))));
    f.render_stateful_widget(list, popup, &mut state);
}

// ----------------------------------------------------------------- log view

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let days = app.active_days_desc();
    let total_commits = app.data.log.len();
    let goal = app.data.daily_goal.max(1);
    let streak = app.streak();

    let title = format!(
        " COMPLETED HISTORY — {} commits · {} active days · {}-day streak ",
        total_commits,
        days.len(),
        streak,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " j/k · gg/G · ⏎ or d: view day details · u: undo last · q back ",
            Style::default().fg(DIM),
        )));

    if days.is_empty() {
        f.render_widget(
            Paragraph::new("\n  nothing here yet — complete a task to make your first commit")
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let today = Local::now().date_naive();
    let max_daily = days.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);
    let bar_w = 20usize;

    let items: Vec<ListItem> = days
        .iter()
        .map(|(date, count)| {
            let met = *count >= goal;
            let bullet_style = if met {
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
            } else if *count > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(DIM)
            };
            let bullet = if met { "● " } else { "○ " };

            let dow = date.format("%a").to_string();
            let mmdd = date.format("%b %e").to_string();
            let date_span = Span::styled(
                format!("{} {}", dow, mmdd.trim_start()),
                Style::default().add_modifier(Modifier::BOLD),
            );

            let count_span = Span::styled(
                format!("  {:>3} done", count),
                Style::default().fg(DIM),
            );

            let filled = ((*count as f64 / max_daily as f64) * bar_w as f64).round() as usize;
            let filled = filled.min(bar_w);
            let bar_color = if met { Color::LightGreen } else { Color::Yellow };
            let bar_span = Span::styled(
                format!("  {}{}", "▓".repeat(filled), "░".repeat(bar_w - filled)),
                Style::default().fg(bar_color),
            );

            let mut spans = vec![
                Span::styled(bullet, bullet_style),
                date_span,
                count_span,
                bar_span,
            ];

            if *date == today {
                spans.push(Span::styled(
                    "  ← today",
                    Style::default().fg(Color::LightCyan),
                ));
            }
            if met {
                let mult = count / goal;
                let label = if mult > 1 {
                    format!("  ✓ goal ×{}", mult)
                } else {
                    String::from("  ✓ goal")
                };
                spans.push(Span::styled(label, Style::default().fg(Color::LightGreen)));
            } else if *count > 0 {
                spans.push(Span::styled(
                    format!("  {}/{}", count, goal),
                    Style::default().fg(DIM),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.log_sel.min(days.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

// --------------------------------------------------------------- day log view

fn draw_daylog(f: &mut Frame, app: &App, area: Rect) {
    let Some(date) = &app.selected_date else { return };
    let logs = app.logs_for_date(date);
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            format!(" CONTRIBUTIONS — {} — {} tasks ", date, logs.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " j/k scroll · u: undo · q back ",
            Style::default().fg(DIM),
        )));

    if logs.is_empty() {
        f.render_widget(
            Paragraph::new(format!("\n  no completions on {}", date))
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = logs
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut spans = vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(DIM)),
                Span::styled(format!("{:5} ", e.time), Style::default().fg(DIM)),
            ];
            if !e.project_name.is_empty() {
                let label = if !e.section_name.is_empty() {
                    format!("[{} - {}] ", e.project_name, e.section_name)
                } else {
                    format!("[{}] ", e.project_name)
                };
                spans.push(Span::styled(
                    label,
                    Style::default().fg(palette(e.color)),
                ));
            }
            spans.push(Span::raw(e.text.clone()));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.day_sel.min(logs.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

// --------------------------------------------------------------- help view

fn draw_help(f: &mut Frame, area: Rect) {
    let key = |k: &'static str, d: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("  {:<14}", k),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(d),
        ])
    };
    let hdr = |t: &'static str| {
        Line::from(Span::styled(
            format!(" {}", t),
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ))
    };
    let lines = vec![
        Line::from(""),
        hdr("MOTIONS (everywhere)"),
        key("j k gg G", "move · jump to top / bottom"),
        key("h l Tab", "switch pane · Tab in a course = next course"),
        key("J / K", "reorder task / section"),
        Line::from(""),
        hdr("DASHBOARD"),
        key("a", "add task here (supports '@course' and '!due')"),
        key("⏎ / Space", "complete selected task · on a course: OPEN it"),
        key("c", "make selected task the CURRENT PRIORITY (move to top)"),
        key("dd", "unlink course task from dashboard (D deletes for real)"),
        key("r", "pull a random backlog task → current priority"),
        key("t / e", "set due date · rename"),
        key("+ -  [ ]", "daily goal · heatmap year"),
        key("o", "open history log"),
        key("u / Ctrl-R", "undo · redo the last action (deletion, completion, add…)"),
        Line::from(""),
        hdr("INSIDE A COURSE (⏎ on a course card)"),
        key("S", "add a section (subtitle): Lectures, Labs, …"),
        key("a", "add task under the section at the cursor"),
        key("p", "link/unlink task ↗ the main dashboard"),
        key("c", "make task the CURRENT PRIORITY (link → dashboard top)"),
        key("m / M", "move task to next / previous section"),
        key("t / e / R", "due date · rename task or section · rename course"),
        key("⏎ / dd", "complete · delete (section keeps its tasks)"),
        Line::from(""),
        hdr("DUE DATES  (t opens picker; '!' when adding: 'lab 2 @algo !+3d')"),
        key("picker", "1 today · 2 tomorrow · 3 +3d · 4 +1w · 5 +2w · 6 +1mo · 7 +3mo"),
        key("picker", "0 clear  ·  c custom text  ·  Esc cancel"),
        key("formats", "2026-07-10 · 7-10 · +3d · +2w · +1mo · today · tomorrow · none"),
        Line::from(""),
        hdr("EX COMMANDS"),
        key(":w :q :wq", "save / quit    :log :db  history / dashboard"),
        key(":goal N", ":year YYYY · :clearlog · :help"),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to return",
            Style::default().fg(DIM),
        )),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " taskvim — help ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ---------------------------------------------------------------- chrome

fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let line = match &app.mode {
        Mode::Command(buf) => Line::from(vec![
            Span::styled(":", Style::default().fg(Color::White)),
            Span::raw(buf.clone()),
            Span::styled("█", Style::default().fg(Color::White)),
        ]),
        Mode::Insert(_, _) => Line::from(Span::styled(
            " -- INSERT --  (Enter: confirm · Esc: cancel)",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Mode::DueMenu { .. } => Line::from(Span::styled(
            " -- DUE PICKER --  (1-7 preset · 0 clear · c custom · Esc cancel)",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Mode::Normal => {
            let pend = match app.pending {
                Some(c) => format!("  [{}‥]", c),
                None => String::new(),
            };
            let mode_tag = match app.view {
                View::Course => " COURSE ",
                View::Log => " LOG ",
                View::DayLog => " DAY LOG ",
                View::Help => " HELP ",
                View::Dashboard => " NORMAL ",
            };
            Line::from(vec![
                Span::styled(
                    mode_tag,
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}{}", app.status, pend),
                    Style::default().fg(DIM),
                ),
            ])
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_input_popup(f: &mut Frame, target: &InsertTarget, buf: &str) {
    let title = match target {
        InsertTarget::DashTask => " New task  (…text @course !due) ",
        InsertTarget::CourseTask { .. } => " New task in course  (…text !due) ",
        InsertTarget::Course => " New course / project ",
        InsertTarget::Section { .. } => " New section (subtitle) ",
        InsertTarget::Due { .. } => " Due date  (YYYY-MM-DD · 7-10 · +3d · +2w · +1mo · today · none) ",
        InsertTarget::RenameTask { .. } => " Rename task ",
        InsertTarget::RenameSection { .. } => " Rename section ",
        InsertTarget::RenameCourse { .. } => " Rename course ",
    };
    let display: String = buf.to_string();
    let area = f.area();
    let w = (area.width.saturating_sub(10)).min(70).max(24);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + area.height / 2 - 1,
        width: w,
        height: 3,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightGreen))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let text = Line::from(vec![
        Span::raw(display),
        Span::styled("█", Style::default().fg(Color::LightGreen)),
    ]);
    f.render_widget(Paragraph::new(text).block(block), popup);
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(400))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.handle_key(k.code, k.modifiers);
                }
            }
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut app = App::new();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    let _ = app.data.save();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        Local::now().date_naive()
    }

    fn ymd(d: NaiveDate) -> String {
        d.format("%Y-%m-%d").to_string()
    }

    #[test]
    fn parse_due_clears_on_none_forms() {
        for s in ["", "none", "clear", "-", "  NONE  "] {
            assert_eq!(parse_due(s), Ok(None), "input: {:?}", s);
        }
    }

    #[test]
    fn parse_due_keywords() {
        assert_eq!(parse_due("today"), Ok(Some(ymd(today()))));
        assert_eq!(
            parse_due("tomorrow"),
            Ok(Some(ymd(today() + CDuration::days(1))))
        );
        assert_eq!(parse_due("tmr"), Ok(Some(ymd(today() + CDuration::days(1)))));
    }

    #[test]
    fn parse_due_relative_offsets() {
        assert_eq!(parse_due("+3"), Ok(Some(ymd(today() + CDuration::days(3)))));
        assert_eq!(parse_due("+3d"), Ok(Some(ymd(today() + CDuration::days(3)))));
        assert_eq!(parse_due("+2w"), Ok(Some(ymd(today() + CDuration::days(14)))));
        assert_eq!(parse_due("2w"), Ok(Some(ymd(today() + CDuration::days(14)))));
    }

    #[test]
    fn parse_due_bare_unit_shortcuts() {
        assert_eq!(parse_due("d"), Ok(Some(ymd(today() + CDuration::days(1)))));
        assert_eq!(parse_due("w"), Ok(Some(ymd(today() + CDuration::days(7)))));
    }

    #[test]
    fn parse_due_absolute_dates() {
        assert_eq!(parse_due("2026-07-10"), Ok(Some("2026-07-10".to_string())));
        assert_eq!(parse_due("2026/07/10"), Ok(Some("2026-07-10".to_string())));
        let expected = format!("{}-12-25", today().year());
        assert_eq!(parse_due("12-25"), Ok(Some(expected)));
    }

    #[test]
    fn parse_due_rejects_garbage() {
        assert_eq!(parse_due("banana"), Err(()));
        assert_eq!(parse_due("2026-13-45"), Err(()));
        assert_eq!(parse_due("1-2-3-4"), Err(()));
    }
}
