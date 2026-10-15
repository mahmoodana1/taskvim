//! The `App` state: the view/mode enums, undo/redo, and the read-only
//! lookups and stats derived from `Data`.

pub mod actions;
pub mod keys;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use chrono::{Datelike, Local, NaiveDate};
use rand::Rng;

use crate::model::{DashSection, Data, LogEntry, Project, Section, Task};

#[derive(PartialEq, Clone, Copy)]
pub enum Pane {
    Tasks,
    Courses,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum View {
    Dashboard,
    Calendar,
    Course,   // modal over the dashboard; cur_course holds the project id
    Archived, // modal over the dashboard listing archived courses
    Log,
    DayLog, // view for a specific day's contributions
    Help,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum CalendarMode {
    Day,
    Week,
    Month,
}

/// A row inside the course modal (sections interleaved with their tasks).
#[derive(PartialEq, Clone, Copy)]
pub enum Row {
    Section(u64),
    NoSection,
    Task(u64),
}

/// A row on the main dashboard. Each section renders as a bordered box, so the
/// row list includes the box chrome — this keeps `App.task_sel` 1:1 with the
/// items ratatui's `ListState` indexes, which is what makes scrolling and
/// selection work across section boundaries.
///
/// `SectionFill` and `SectionBottom` are never selectable; movement skips them.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum DashRow {
    /// `┌ IMPORTANT ──── 2 ┐`
    SectionTop(DashSection),
    /// `│ buy groceries    │`
    Task(u64),
    /// `│                  │` — the blank interior of an empty section.
    SectionFill(DashSection),
    /// `└──────────────────┘`
    SectionBottom(DashSection),
}

impl DashRow {
    /// Can the cursor land here? Box chrome cannot.
    ///
    /// Unused until Task 2 wires navigation to skip chrome rows.
    pub fn is_selectable(&self) -> bool {
        matches!(self, DashRow::SectionTop(_) | DashRow::Task(_))
    }
}

#[derive(PartialEq, Clone)]
pub enum InsertTarget {
    DashTask,
    CalendarTask { due: String },
    CalendarTaskTime { task: u64 },
    CalendarEvent { date: String },
    CalendarEventTime { event: u64 },
    CourseTask { project: u64, section: Option<u64> },
    Course,
    Section { project: u64 },
    Due { task: u64 },
    Repeat { task: u64 },
    RenameTask { task: u64 },
    RenameSection { section: u64 },
    RenameCourse { project: u64 },
}

#[derive(PartialEq, Clone)]
pub enum Mode {
    Normal,
    Insert(InsertTarget, String),
    Command(String),
    DueMenu { task: u64 },
    RepeatMenu { task: u64 },
}

pub struct App {
    pub data: Data,
    pub pane: Pane,
    pub view: View,
    pub mode: Mode,
    pub task_sel: usize,   // index into dash_rows()
    pub course_sel: usize, // index into visible_courses(), NOT data.projects
    pub cur_course: Option<u64>,
    pub course_row: usize,   // cursor inside the course modal
    pub archived_sel: usize, // cursor inside the archive modal
    pub log_sel: usize,
    pub day_sel: usize,                // selected day in day log view
    pub selected_date: Option<String>, // YYYY-MM-DD format for day view
    pub year: i32,
    pub calendar_mode: CalendarMode,
    pub calendar_date: NaiveDate,
    pub calendar_sel: usize,
    pub calendar_palette_base: usize,
    pub calendar_feed: Vec<crate::calendar::Class>,
    pub pending: Option<char>, // gg / dd
    pub status: String,
    pub quit: bool,
    /// Snapshot of `data` matching the last checkpoint; used to detect real changes.
    pub saved_snapshot: Data,
    /// Prior data states, most recent last.
    pub undo_stack: Vec<Data>,
    /// Undone data states, most recent last.
    pub redo_stack: Vec<Data>,
    /// The date `tick()` was last run for; the run loop compares this against
    /// today to catch midnight rollovers while the app sits open.
    pub last_tick: NaiveDate,
    /// A Yedion fetch running in the background; its feed replaces
    /// `calendar_feed` when it lands.
    pub feed_refresh: Option<std::sync::mpsc::Receiver<Vec<crate::calendar::Class>>>,
    /// Modification time of data.json as of our last load or save. Anything
    /// newer was written by someone else (the phone, through the subscreen
    /// agent), and `sync_from_disk` takes it in.
    pub disk_mtime: Option<std::time::SystemTime>,
}

pub const UNDO_HISTORY_LIMIT: usize = 100;

impl App {
    pub fn new() -> App {
        let mut app = App::with_data(Data::load());
        // Draw from the cached feed at once; the network refresh catches up.
        app.calendar_feed = crate::calendar::load_cached();
        app.feed_refresh = Some(crate::calendar::refresh_in_background());
        app.tick();
        app.disk_mtime = Self::data_mtime();
        app
    }

    /// Swap in a background-fetched feed if one has arrived. Returns true when
    /// the feed changed, so the caller knows to redraw.
    pub fn poll_feed(&mut self) -> bool {
        let Some(rx) = &self.feed_refresh else {
            return false;
        };
        match rx.try_recv() {
            Ok(feed) => {
                let manual = self.status.starts_with("refreshing Yedion");
                self.calendar_feed = feed;
                self.feed_refresh = None;
                if manual {
                    self.status = format!("Yedion calendar refreshed · {} items", self.calendar_feed.len());
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if self.status.starts_with("refreshing Yedion") {
                    self.status = String::from("Yedion refresh failed · showing cached calendar");
                }
                self.feed_refresh = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
        }
    }

    /// Build an App around already-loaded data. Used by `new()` and by tests.
    pub fn with_data(data: Data) -> App {
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
            archived_sel: 0,
            log_sel: 0,
            day_sel: 0,
            selected_date: None,
            year: Local::now().year(),
            calendar_mode: CalendarMode::Week,
            calendar_date: Local::now().date_naive(),
            calendar_sel: 0,
            // Keep calendar accents in the calmer blue / cyan / green part of
            // TaskVim's palette; the value is only the starting offset.
            calendar_palette_base: rand::thread_rng().gen_range(0..3),
            calendar_feed: Vec::new(),
            pending: None,
            status: String::from("taskvim ready — ? help · ⏎ on a course opens it · : commands"),
            quit: false,
            saved_snapshot,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_tick: Local::now().date_naive(),
            feed_refresh: None,
            disk_mtime: None,
        }
    }

    pub fn save_quiet(&mut self) {
        self.checkpoint();
        let _ = self.data.save();
        self.disk_mtime = Self::data_mtime();
    }

    fn data_mtime() -> Option<std::time::SystemTime> {
        std::fs::metadata(Data::path()).and_then(|m| m.modified()).ok()
    }

    /// Take in changes another program made to data.json while we were open.
    /// Every edit here is saved straight away, so nothing of ours is pending
    /// when this replaces `data`; the undo history is dropped because it
    /// describes a file that no longer exists.
    pub fn sync_from_disk(&mut self) {
        let mtime = Self::data_mtime();
        if mtime.is_none() || mtime == self.disk_mtime {
            return;
        }
        self.disk_mtime = mtime;
        let fresh = Data::load();
        if fresh != self.data {
            self.data = fresh;
            self.saved_snapshot = self.data.clone();
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.clamp();
            self.status = String::from("updated from another device");
        }
    }

    pub fn save(&mut self) {
        self.checkpoint();
        let saved = self.data.save();
        self.disk_mtime = Self::data_mtime();
        match saved {
            Ok(_) => self.status = format!("written to {}", Data::path().display()),
            Err(e) => self.status = format!("save failed: {}", e),
        }
    }

    /// Record the pre-mutation state on the undo stack if data actually changed.
    /// Clears the redo stack — any new edit forks history, like vim.
    pub fn checkpoint(&mut self) {
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

    pub fn undo(&mut self) {
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

    pub fn redo(&mut self) {
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

    pub fn counts_by_date(&self) -> HashMap<NaiveDate, u32> {
        let mut m: HashMap<NaiveDate, u32> = HashMap::new();
        for e in &self.data.log {
            if let Ok(d) = NaiveDate::parse_from_str(&e.date, "%Y-%m-%d") {
                *m.entry(d).or_insert(0) += 1;
            }
        }
        m
    }

    /// (date, count) for every day that has at least one commit, most recent first.
    pub fn active_days_desc(&self) -> Vec<(NaiveDate, u32)> {
        let mut pairs: Vec<(NaiveDate, u32)> = self.counts_by_date().into_iter().collect();
        pairs.sort_by(|a, b| b.0.cmp(&a.0));
        pairs
    }

    pub fn today_count(&self) -> u32 {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        self.data.log.iter().filter(|e| e.date == today).count() as u32
    }

    pub fn logs_for_date(&self, date: &str) -> Vec<&LogEntry> {
        self.data.log.iter().filter(|e| e.date == date).collect()
    }

    pub fn streak(&self) -> u32 {
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

    pub fn project(&self, id: u64) -> Option<&Project> {
        self.data.projects.iter().find(|p| p.id == id)
    }

    pub fn project_of(&self, id: Option<u64>) -> Option<&Project> {
        id.and_then(|pid| self.project(pid))
    }

    pub fn section(&self, id: u64) -> Option<&Section> {
        self.data.sections.iter().find(|s| s.id == id)
    }

    pub fn section_of(&self, id: Option<u64>) -> Option<&Section> {
        id.and_then(|sid| self.section(sid))
    }

    pub fn task(&self, id: u64) -> Option<&Task> {
        self.data.tasks.iter().find(|t| t.id == id)
    }

    /// (done, total) for a project. Recurring tasks are excluded: they never
    /// finish, so counting them would make the progress bar oscillate every
    /// time a habit resets.
    pub fn project_stats(&self, pid: u64) -> (u32, u32) {
        let relevant = || {
            self.data
                .tasks
                .iter()
                .filter(|t| t.project == Some(pid) && t.repeat.is_none())
        };
        let completed = relevant().filter(|t| t.completed).count() as u32;
        let pending = relevant().filter(|t| !t.completed).count() as u32;
        (completed, completed + pending)
    }

    pub fn overdue_count(&self, pid: u64) -> u32 {
        let today = Local::now().date_naive();
        self.data
            .tasks
            .iter()
            .filter(|t| t.project == Some(pid) && !t.completed && t.repeat.is_none())
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
    pub fn dash_ids(&self) -> Vec<u64> {
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

    /// How many of today's commits came from tasks currently on the dashboard.
    /// Joins on the `LogEntry.task` back-link; entries migrated from v2 have
    /// no link and fall back to the old text join so migration day doesn't
    /// visibly reset the counter.
    pub fn dash_completed_today(&self) -> u32 {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        self.data
            .log
            .iter()
            .filter(|e| e.date == today)
            .filter(|e| match e.task {
                Some(id) => self.data.dashboard.contains(&id),
                None => self.data.dashboard.iter().any(|d| {
                    self.data
                        .tasks
                        .iter()
                        .any(|t| t.id == *d && t.text == e.text)
                }),
            })
            .count() as u32
    }

    pub fn is_pinned(&self, tid: u64) -> bool {
        self.data.dashboard.contains(&tid)
    }

    /// Dashboard rows in display order, including section-box chrome.
    /// `data.dashboard` remains the source of truth for task *order*; the
    /// section is a property of the task, so this walks the dashboard once per
    /// section and preserves relative order.
    pub fn dash_rows(&self) -> Vec<DashRow> {
        let visible = self.dash_ids();
        let mut rows = Vec::with_capacity(visible.len() + DashSection::ALL.len() * 3);
        for sec in DashSection::ALL {
            rows.push(DashRow::SectionTop(sec));
            let before = rows.len();
            for id in &visible {
                if self
                    .task(*id)
                    .map(|t| t.dash_section == sec)
                    .unwrap_or(false)
                {
                    rows.push(DashRow::Task(*id));
                }
            }
            if rows.len() == before {
                rows.push(DashRow::SectionFill(sec));
            }
            rows.push(DashRow::SectionBottom(sec));
        }
        rows
    }

    /// The courses the Courses pane shows, in order. `course_sel` indexes
    /// THIS list, never `data.projects` — archived courses are hidden, so the
    /// two differ and using the wrong one acts on the wrong course.
    pub fn visible_courses(&self) -> Vec<&Project> {
        self.data.projects.iter().filter(|p| !p.archived).collect()
    }

    /// Id of the course under the Courses-pane cursor.
    pub fn selected_course(&self) -> Option<u64> {
        self.visible_courses().get(self.course_sel).map(|p| p.id)
    }

    /// The archived courses, in order.
    pub fn archived_courses(&self) -> Vec<&Project> {
        self.data.projects.iter().filter(|p| p.archived).collect()
    }

    /// The task under the cursor, or `None` when the cursor is on a header.
    pub fn selected_task(&self) -> Option<u64> {
        match self.dash_rows().get(self.task_sel) {
            Some(DashRow::Task(id)) => Some(*id),
            _ => None,
        }
    }

    /// The section the cursor sits in — its own header, or the header above
    /// the selected task. Used to decide where `a` adds a new task.
    pub fn selected_section(&self) -> DashSection {
        let rows = self.dash_rows();
        for i in (0..=self.task_sel.min(rows.len().saturating_sub(1))).rev() {
            if let Some(DashRow::SectionTop(s)) = rows.get(i) {
                return *s;
            }
        }
        DashSection::General
    }

    // ------------------------------------------------------------ course rows

    pub fn course_rows(&self, pid: u64) -> Vec<Row> {
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

    pub fn focus(&mut self, pid: u64, target: Row) {
        let rows = self.course_rows(pid);
        self.course_row = rows
            .iter()
            .position(|r| *r == target)
            .unwrap_or_else(|| self.course_row.min(rows.len().saturating_sub(1)));
    }

    pub fn clamp_course_row(&mut self, pid: u64) {
        let n = self.course_rows(pid).len();
        self.course_row = self.course_row.min(n.saturating_sub(1));
    }

    // -------------------------------------------------------- bounds clamping

    /// Move the dashboard cursor by `delta` rows, skipping box chrome. Stays
    /// put when there is no selectable row in that direction.
    pub fn step_dash_cursor(&mut self, delta: isize) {
        let rows = self.dash_rows();
        if rows.is_empty() {
            return;
        }
        let mut i = self.task_sel as isize;
        loop {
            i += delta;
            if i < 0 || i as usize >= rows.len() {
                return; // ran off the end — leave the cursor where it was
            }
            if rows[i as usize].is_selectable() {
                self.task_sel = i as usize;
                return;
            }
        }
    }

    /// Index of the last selectable row, for `G`.
    pub fn last_dash_row(&self) -> usize {
        let rows = self.dash_rows();
        rows.iter().rposition(|r| r.is_selectable()).unwrap_or(0)
    }

    pub fn clamp(&mut self) {
        let rows = self.dash_rows();
        if self.task_sel >= rows.len() {
            // The last row is always a box bottom, so settle onto the last row
            // the cursor is actually allowed to occupy.
            self.task_sel = self.last_dash_row();
        } else if !rows[self.task_sel].is_selectable() {
            // A delete or a section move can leave the cursor on chrome.
            self.task_sel = rows[..self.task_sel]
                .iter()
                .rposition(|r| r.is_selectable())
                .unwrap_or(0);
        }
        let visible_courses = self.visible_courses().len();
        if self.course_sel >= visible_courses {
            self.course_sel = visible_courses.saturating_sub(1);
        }
        if self.log_sel >= self.data.log.len() {
            self.log_sel = self.data.log.len().saturating_sub(1);
        }
    }
}
