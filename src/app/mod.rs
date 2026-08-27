//! The `App` state: the view/mode enums, undo/redo, and the read-only
//! lookups and stats derived from `Data`.

pub mod actions;
pub mod keys;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use chrono::{Datelike, Local, NaiveDate};

use crate::model::{Data, LogEntry, Project, Section, Task};

#[derive(PartialEq, Clone, Copy)]
pub enum Pane {
    Tasks,
    Courses,
}

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Dashboard,
    Course, // modal over the dashboard; cur_course holds the project id
    Log,
    DayLog, // view for a specific day's contributions
    Help,
}

/// A row inside the course modal (sections interleaved with their tasks).
#[derive(PartialEq, Clone, Copy)]
pub enum Row {
    Section(u64),
    NoSection,
    Task(u64),
}

#[derive(PartialEq, Clone)]
pub enum InsertTarget {
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
pub enum Mode {
    Normal,
    Insert(InsertTarget, String),
    Command(String),
    DueMenu { task: u64 },
}

pub struct App {
    pub data: Data,
    pub pane: Pane,
    pub view: View,
    pub mode: Mode,
    pub task_sel: usize,   // index into data.dashboard
    pub course_sel: usize, // index into data.projects
    pub cur_course: Option<u64>,
    pub course_row: usize, // cursor inside the course modal
    pub log_sel: usize,
    pub day_sel: usize,    // selected day in day log view
    pub selected_date: Option<String>, // YYYY-MM-DD format for day view
    pub year: i32,
    pub pending: Option<char>, // gg / dd
    pub status: String,
    pub quit: bool,
    /// Snapshot of `data` matching the last checkpoint; used to detect real changes.
    pub saved_snapshot: Data,
    /// Prior data states, most recent last.
    pub undo_stack: Vec<Data>,
    /// Undone data states, most recent last.
    pub redo_stack: Vec<Data>,
}

pub const UNDO_HISTORY_LIMIT: usize = 100;

impl App {
    pub fn new() -> App {
        App::with_data(Data::load())
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

    pub fn save_quiet(&mut self) {
        self.checkpoint();
        let _ = self.data.save();
    }

    pub fn save(&mut self) {
        self.checkpoint();
        match self.data.save() {
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

    /// (done, total) for a project: done = logged completions, total = done + pending
    pub fn project_stats(&self, pid: u64) -> (u32, u32) {
        let completed = self.data.tasks.iter().filter(|t| t.project == Some(pid) && t.completed).count() as u32;
        let pending = self.data.tasks.iter().filter(|t| t.project == Some(pid) && !t.completed).count() as u32;
        (completed, completed + pending)
    }

    pub fn overdue_count(&self, pid: u64) -> u32 {
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

    pub fn dash_completed_today(&self) -> u32 {
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        self.data
            .log
            .iter()
            .filter(|e| e.date == today && self.data.dashboard.iter().any(|d| {
                self.data.tasks.iter().any(|t| t.id == *d && t.text == e.text)
            }))
            .count() as u32
    }

    pub fn is_pinned(&self, tid: u64) -> bool {
        self.data.dashboard.contains(&tid)
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

    pub fn clamp(&mut self) {
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
}
