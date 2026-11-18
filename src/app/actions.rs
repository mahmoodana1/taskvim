//! The mutating operations on `App`: adding, completing, deleting,
//! reordering, and pinning tasks, sections, and courses.

use chrono::{Duration as CDuration, Local, Months, NaiveDate, Timelike};
use rand::Rng;

use crate::app::{App, DashRow, EventForm, EventTarget, InsertTarget, Mode, Pane, Row, View};
use crate::calendar::{AgendaItem, Class};
use crate::due::parse_due;
use crate::model::{
    CalendarEvent, ClassEdit, DashSection, LogEntry, Project, Repeat, Section, Task, COLOR_COUNT,
};

impl App {
    /// Strip only a trailing `!due` token. Used by the course modal, where the
    /// course is already known — so `@course` must be treated as literal text.
    pub fn parse_due_suffix(&self, buf: &str) -> (String, Option<String>) {
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

    pub fn add_task(
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
            start_time: None,
            end_time: None,
            completed: false,
            dash_section: crate::model::DashSection::default(),
            repeat: None,
            period: None,
            relink: false,
        });
        if pin {
            self.data.dashboard.push(id);
        }
        self.status = String::from("task added");
        self.save_quiet();
        Some(id)
    }

    pub fn add_course(&mut self, name: String) {
        if name.is_empty() {
            return;
        }
        let id = self.data.new_id();
        let color = self.data.next_color;
        self.data.next_color = (self.data.next_color + 1) % COLOR_COUNT;
        self.data.projects.push(Project {
            id,
            name,
            color,
            archived: false,
        });
        // A new course is never archived, so it is last in the visible list.
        self.course_sel = self.visible_courses().len().saturating_sub(1);
        self.status = String::from("course added — ⏎ opens it");
        self.save_quiet();
    }

    /// Archive a course: hide it, and take its tasks off the dashboard while
    /// remembering which ones were there so `unarchive_course` can put exactly
    /// those back.
    pub fn archive_course(&mut self, pid: u64) {
        let dash = self.data.dashboard.clone();
        let ids: Vec<u64> = self
            .data
            .tasks
            .iter()
            .filter(|t| t.project == Some(pid) && dash.contains(&t.id))
            .map(|t| t.id)
            .collect();

        for t in self.data.tasks.iter_mut() {
            if ids.contains(&t.id) {
                t.relink = true;
            }
        }
        self.data.dashboard.retain(|id| !ids.contains(id));

        if let Some(p) = self.data.projects.iter_mut().find(|p| p.id == pid) {
            p.archived = true;
        }
        self.status = format!(
            "archived — {} task{} unlinked · X: open the archive",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        );
        self.clamp();
        self.save_quiet();
    }

    /// Restore an archived course and re-link the tasks that were on the
    /// dashboard when it was archived. Skips anything completed in the
    /// meantime, and anything the user already re-linked by hand.
    pub fn unarchive_course(&mut self, pid: u64) {
        let mut to_link: Vec<u64> = Vec::new();
        for t in self.data.tasks.iter_mut() {
            if t.project == Some(pid) && t.relink {
                t.relink = false;
                if !t.completed {
                    to_link.push(t.id);
                }
            }
        }
        let mut restored = 0usize;
        for id in to_link {
            if !self.data.dashboard.contains(&id) {
                self.data.dashboard.push(id);
                restored += 1;
            }
        }

        if let Some(p) = self.data.projects.iter_mut().find(|p| p.id == pid) {
            p.archived = false;
        }
        self.status = format!(
            "restored — {} task{} back on the dashboard",
            restored,
            if restored == 1 { "" } else { "s" }
        );
        self.clamp();
        self.save_quiet();
    }

    pub fn add_section(&mut self, pid: u64, name: String) {
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

    pub fn toggle_complete(&mut self, tid: u64) {
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
                task: Some(tid),
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
            // Remove exactly ONE entry — the newest match. `retain` used to
            // delete every entry with the same text, wiping months of history
            // from the heatmap the first time a repeated task was un-completed.
            // Entries migrated from v2 have `task: None` and fall back to the
            // old text comparison, still capped at one removal.
            let pos = self.data.log.iter().rposition(|e| match e.task {
                Some(id) => id == tid,
                None => e.text == text && e.project == project && e.section == section,
            });
            if let Some(pos) = pos {
                self.data.log.remove(pos);
            }
            self.status = String::from("task marked as pending");
        }
        self.clamp();
        self.save_quiet();
    }

    pub fn toggle_pin(&mut self, tid: u64) {
        if let Some(pos) = self.data.dashboard.iter().position(|id| *id == tid) {
            self.data.dashboard.remove(pos);
            self.status = String::from("removed from main dashboard");
        } else {
            self.data.dashboard.push(tid);
            if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) {
                t.dash_section = DashSection::General;
            }
            self.status = String::from("linked to main dashboard ↗");
        }
        self.clamp();
        self.save_quiet();
    }

    pub fn make_priority(&mut self, tid: u64) {
        let was_linked = self.data.dashboard.iter().any(|id| *id == tid);
        self.data.dashboard.retain(|id| *id != tid);
        self.data.dashboard.insert(0, tid);
        if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) {
            t.dash_section = DashSection::Important;
        }
        self.task_sel = 1; // row 0 is the Important header
        self.status = if was_linked {
            String::from("moved to top → current priority")
        } else {
            String::from("linked to dashboard → current priority ↗")
        };
        self.clamp();
        self.save_quiet();
    }

    pub fn delete_task(&mut self, tid: u64) {
        if let Some(idx) = self.data.tasks.iter().position(|t| t.id == tid) {
            let t = self.data.tasks.remove(idx);
            self.data.dashboard.retain(|id| *id != tid);
            self.status = format!("deleted task: {}", t.text);
        }
        self.clamp();
        self.save_quiet();
    }

    pub fn delete_section(&mut self, sid: u64) {
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

    pub fn delete_course(&mut self, pid: u64) {
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

    pub fn pick_random(&mut self) {
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
        if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) {
            t.dash_section = DashSection::Important;
        }
        self.task_sel = 1; // row 0 is the Important header
        self.pane = Pane::Tasks;
        self.status = String::from("random task pulled from the backlog → current priority");
        self.save_quiet();
    }

    /// Reorder the selected task among the siblings in its own section.
    /// Because `dash_rows()` groups by section, a task's section siblings are
    /// its adjacent `DashRow::Task` rows — a move that would cross a header is
    /// simply refused.
    pub fn move_dash_task(&mut self, delta: i32) {
        let rows = self.dash_rows();
        let Some(DashRow::Task(a_id)) = rows.get(self.task_sel).copied() else {
            return;
        };
        let target = self.task_sel as i32 + delta;
        if target < 0 || target as usize >= rows.len() {
            return;
        }
        let Some(DashRow::Task(b_id)) = rows.get(target as usize).copied() else {
            // Neighbour is a section header — we're at the edge of the section.
            return;
        };
        let a = self.data.dashboard.iter().position(|id| *id == a_id);
        let b = self.data.dashboard.iter().position(|id| *id == b_id);
        if let (Some(a), Some(b)) = (a, b) {
            self.data.dashboard.swap(a, b);
            self.task_sel = target as usize;
            self.save_quiet();
        }
    }

    /// Reorder a task among its siblings (same project + section).
    pub fn move_task_in_section(&mut self, tid: u64, delta: i32) {
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
    pub fn move_section(&mut self, sid: u64, delta: i32) {
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
    pub fn cycle_task_section(&mut self, tid: u64, delta: i32) {
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
        let next = ((pos + delta).rem_euclid(options.len() as i32)) as usize;
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

    pub fn confirm_insert(&mut self, target: InsertTarget, buf: String) {
        match target {
            InsertTarget::DashTask => {
                let p = crate::parse::parse_task_input(&self.data.projects, &buf);
                let section = p.section.unwrap_or_else(|| self.selected_section());
                let repeat = p.repeat;
                if let Some(id) = self.add_task(p.text, p.project, None, p.due, true) {
                    if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == id) {
                        t.dash_section = section;
                    }
                    if let Some(r) = repeat {
                        self.set_repeat(id, Some(r));
                    }
                    if let Some(pos) = self
                        .dash_rows()
                        .iter()
                        .position(|r| *r == DashRow::Task(id))
                    {
                        self.task_sel = pos;
                    }
                    self.save_quiet();
                } else {
                    self.status = String::from(
                        "give the task a name — @course, !due and #section are suffixes",
                    );
                }
            }
            InsertTarget::CalendarTask { due } => {
                let (input, time_range) = crate::calendar::strip_time_range(&buf);
                let p = crate::parse::parse_task_input(&self.data.projects, &input);
                let section = p.section.unwrap_or(DashSection::General);
                let repeat = p.repeat;
                if let Some(id) = self.add_task(p.text, p.project, None, p.due.or(Some(due)), true)
                {
                    if let Some(task) = self.data.tasks.iter_mut().find(|t| t.id == id) {
                        task.dash_section = section;
                        if let Some((start, end)) = time_range {
                            task.start_time = Some(start);
                            task.end_time = Some(end);
                        }
                    }
                    if let Some(repeat) = repeat {
                        self.set_repeat(id, Some(repeat));
                    }
                    self.status = String::from("calendar task added · linked to TaskVim");
                } else {
                    self.status = String::from("give the task a name");
                }
            }
            InsertTarget::CalendarTaskTime { task } => {
                let input = buf.trim();
                if input.eq_ignore_ascii_case("none") || input.is_empty() {
                    self.set_task_time(task, None);
                } else if let Some((start, end)) = crate::calendar::parse_time_range(input) {
                    self.set_task_time(task, Some((start, end)));
                } else {
                    self.status = String::from("use HH:MM-HH:MM, for example 09:00-10:30 or none");
                }
            }
            InsertTarget::CalendarEventTime { event } => {
                if let Some((start, end)) = crate::calendar::parse_time_range(buf.trim()) {
                    if let Some(item) = self
                        .data
                        .calendar_events
                        .iter_mut()
                        .find(|item| item.id == event)
                    {
                        item.start_time = start;
                        item.end_time = end;
                        self.status = String::from("event time updated");
                        self.save_quiet();
                    }
                    if self.view == View::Calendar {
                        self.select_calendar(|item| {
                            matches!(item, crate::calendar::AgendaItem::Event(e) if e.id == event)
                        });
                    }
                } else {
                    self.status = String::from("use HH:MM-HH:MM, for example 09:00-10:30");
                }
            }
            InsertTarget::CourseTask { project, section } => {
                let (text, due) = self.parse_due_suffix(&buf);
                if let Some(id) = self.add_task(text, Some(project), section, due, false) {
                    self.focus(project, Row::Task(id));
                }
            }
            InsertTarget::Course => self.add_course(buf.trim().to_string()),
            InsertTarget::Section { project } => self.add_section(project, buf.trim().to_string()),
            InsertTarget::Due { task } => match parse_due(buf.trim()) {
                Ok(due) => self.apply_due(task, due),
                Err(_) => {
                    self.status =
                        String::from("bad date — try 2026-07-10, 7-10, +3, tomorrow, or none")
                }
            },
            InsertTarget::Repeat { task } => {
                let anchor = Local::now().date_naive().format("%Y-%m-%d").to_string();
                match crate::due::parse_repeat(buf.trim(), &anchor) {
                    Ok(r) => self.set_repeat(task, r),
                    Err(_) => {
                        self.status =
                            String::from("bad repeat — try 3d, 2w, 1mo, daily, weekly, or none")
                    }
                }
            }
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

    pub fn set_task_time(&mut self, task: u64, range: Option<(String, String)>) {
        if let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == task) {
            match range {
                Some((start, end)) => {
                    t.start_time = Some(start);
                    t.end_time = Some(end);
                    self.status = String::from("task time updated");
                }
                None => {
                    t.start_time = None;
                    t.end_time = None;
                    self.status = String::from("task time cleared");
                }
            }
            self.save_quiet();
        }
        if self.view == View::Calendar {
            self.select_calendar(|item| item.task_id() == Some(task));
        }
    }

    pub fn set_due_days(&mut self, task: u64, days: i64) {
        let d = Local::now().date_naive() + CDuration::days(days);
        self.apply_due(task, Some(d.format("%Y-%m-%d").to_string()));
    }

    pub fn set_due_months(&mut self, task: u64, months: u32) {
        let today = Local::now().date_naive();
        let d = today
            .checked_add_months(Months::new(months))
            .unwrap_or(today);
        self.apply_due(task, Some(d.format("%Y-%m-%d").to_string()));
    }

    pub fn clear_due(&mut self, task: u64) {
        self.apply_due(task, None);
    }

    pub fn apply_due(&mut self, task: u64, due: Option<String>) {
        if self.task(task).map(|t| t.repeat.is_some()).unwrap_or(false) {
            self.status =
                String::from("recurring tasks don't take due dates — press ~ to change the repeat");
            return;
        }
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

    /// Set or clear a task's repeat. Setting one clears the due date — the two
    /// are mutually exclusive — and stamps the current period immediately so
    /// the task doesn't reset on the very next tick.
    pub fn set_repeat(&mut self, tid: u64, repeat: Option<Repeat>) {
        let today = Local::now().date_naive();
        let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) else {
            return;
        };
        let status = match &repeat {
            Some(r) => {
                t.due = None;
                t.period = Some(
                    crate::due::period_start(today, r)
                        .format("%Y-%m-%d")
                        .to_string(),
                );
                format!("repeats {}", r.label())
            }
            None => {
                t.period = None;
                String::from("repeat cleared")
            }
        };
        t.repeat = repeat;
        self.status = status;
        self.save_quiet();
    }

    /// Move a dashboard task into one of the fixed sections and keep the
    /// cursor on it as it jumps to its new row.
    pub fn set_dash_section(&mut self, tid: u64, sec: DashSection) {
        let Some(t) = self.data.tasks.iter_mut().find(|t| t.id == tid) else {
            return;
        };
        if t.dash_section == sec {
            self.status = format!("already in {}", sec.title());
            return;
        }
        t.dash_section = sec;
        if !self.data.dashboard.contains(&tid) {
            self.data.dashboard.push(tid);
        }
        if let Some(pos) = self
            .dash_rows()
            .iter()
            .position(|r| *r == DashRow::Task(tid))
        {
            self.task_sel = pos;
        }
        self.status = format!("moved to {}", sec.title());
        self.save_quiet();
    }

    /// Reset every habit whose period has rolled over. Pure state transition
    /// against an explicit date so it can be tested without waiting for
    /// midnight; `tick()` supplies today.
    ///
    /// Deliberately bypasses the undo stack: a rollover is not a user edit, so
    /// pressing `u` after midnight should undo your last real action.
    pub fn tick_on(&mut self, today: NaiveDate) -> bool {
        let mut changed = false;
        for t in self.data.tasks.iter_mut() {
            let Some(r) = t.repeat.as_ref() else { continue };
            let start = crate::due::period_start(today, r)
                .format("%Y-%m-%d")
                .to_string();
            if t.period.as_deref() != Some(start.as_str()) {
                t.period = Some(start);
                t.completed = false;
                // Habits never carry a due date; clear any left by an edit
                // that predates the repeat being set.
                t.due = None;
                changed = true;
            }
        }
        if changed {
            self.saved_snapshot = self.data.clone();
            let _ = self.data.save();
            self.clamp();
        }
        changed
    }

    /// Run the tick for today.
    pub fn tick(&mut self) -> bool {
        self.tick_on(Local::now().date_naive())
    }

    pub fn open_course(&mut self, pid: u64) {
        self.cur_course = Some(pid);
        self.course_row = 0;
        self.view = View::Course;
        self.status = String::from(
            "a:task S:section p:link→dashboard c:priority t:due e:rename m:move dd:delete q:close",
        );
    }
}

// ---------------------------------------------------------- calendar events

impl App {
    /// A blank form for an event on the selected day, an hour long. Today it
    /// starts at the next full hour; other days at 09:00.
    pub fn new_event_form(&self) -> EventForm {
        let now = Local::now();
        let hour = if self.calendar_date == now.date_naive() {
            (now.hour() + 1).min(22)
        } else {
            9
        };
        let mut fields: [String; 6] = Default::default();
        fields[EventForm::DATE] = self.calendar_date.format("%Y-%m-%d").to_string();
        fields[EventForm::TIME] = format!("{:02}:00-{:02}:00", hour, hour + 1);
        fields[EventForm::REPEAT] = String::from("none");
        EventForm {
            target: EventTarget::New,
            opened: fields.clone(),
            fields,
            focus: EventForm::TITLE,
            all_meetings: false,
        }
    }

    /// Open the form on the selected day-view item. Tasks have their own
    /// keys (`s` time, `t` due), so only events and classes open here.
    pub fn edit_calendar_selection(&mut self) {
        let mut fields: [String; 6] = Default::default();
        let target = match self.calendar_selected() {
            Some(AgendaItem::Event(occurrence)) => {
                // Edit the series as stored, not the dated occurrence.
                let Some(event) = self.data.calendar_events.iter().find(|e| e.id == occurrence.id)
                else {
                    return;
                };
                fields[EventForm::DATE] = event.date.clone();
                fields[EventForm::REPEAT] = event
                    .repeat
                    .as_ref()
                    .map(crate::calendar::repeat_spec)
                    .unwrap_or_else(|| String::from("none"));
                fields[EventForm::UNTIL] = event
                    .repeat
                    .as_ref()
                    .and_then(|r| r.until.clone())
                    .unwrap_or_default();
                EventTarget::Event(event.id)
            }
            Some(AgendaItem::Class(class)) if !class.id.is_empty() => {
                fields[EventForm::DATE] = class.date.clone();
                EventTarget::Class(class.id.clone())
            }
            Some(AgendaItem::Class(_)) => {
                self.status = String::from("this feed item has no ID and cannot be edited");
                return;
            }
            Some(AgendaItem::Task { .. }) => {
                self.status = String::from("tasks: s sets the time · Enter completes");
                return;
            }
            None => {
                self.status = String::from("no calendar item selected");
                return;
            }
        };
        let item = self.calendar_selected().expect("matched above");
        fields[EventForm::TITLE] = item.title().to_string();
        fields[EventForm::NOTE] = item.note().to_string();
        fields[EventForm::TIME] = format!("{}-{}", item.start(), item.end());
        self.mode = Mode::EventForm(EventForm {
            target,
            opened: fields.clone(),
            fields,
            focus: EventForm::TITLE,
            all_meetings: false,
        });
    }

    pub fn save_event_form(&mut self, form: EventForm) {
        let field = |i: usize| form.fields[i].trim().to_string();
        let title = field(EventForm::TITLE);
        let note = field(EventForm::NOTE);
        let Some((start, end)) = crate::calendar::parse_time_range(&field(EventForm::TIME)) else {
            self.status = String::from("time: use HH:MM-HH:MM, for example 09:00-10:30");
            return;
        };

        if let EventTarget::Class(id) = &form.target {
            self.save_class_edit(id, &form, title, note, (start, end));
            return;
        }

        if title.is_empty() {
            self.status = String::from("give the event a title");
            return;
        }
        let date = match parse_due(&field(EventForm::DATE)) {
            Ok(Some(date)) => date,
            _ => {
                self.status = String::from("date: try 2026-10-20, 10-20, +3, or tomorrow");
                return;
            }
        };
        let mut repeat = match crate::calendar::parse_event_repeat(&field(EventForm::REPEAT)) {
            Ok(repeat) => repeat,
            Err(_) => {
                self.status =
                    String::from("repeat: none · daily · weekly · 2w · monthly · yearly · sun,tue");
                return;
            }
        };
        let until = match parse_due(&field(EventForm::UNTIL)) {
            Ok(until) => until,
            Err(_) => {
                self.status = String::from("until: a date like 2027-01-31, or empty for forever");
                return;
            }
        };
        if until.as_deref().is_some_and(|u| u < date.as_str()) {
            self.status = String::from("until is before the event's date");
            return;
        }
        if let Some(repeat) = repeat.as_mut() {
            repeat.until = until;
        }

        let id = match form.target {
            EventTarget::Event(id) => {
                let Some(event) = self.data.calendar_events.iter_mut().find(|e| e.id == id) else {
                    return;
                };
                // A new start date or rule re-lays every occurrence, so the
                // dates deleted one by one no longer mean anything.
                if event.date != date || event.repeat != repeat {
                    event.skip.clear();
                }
                event.title = title;
                event.note = note;
                event.date = date.clone();
                event.start_time = start;
                event.end_time = end;
                event.repeat = repeat;
                self.status = String::from("event updated");
                id
            }
            _ => {
                let id = self.data.new_id();
                self.data.calendar_events.push(CalendarEvent {
                    id,
                    date: date.clone(),
                    title,
                    start_time: start,
                    end_time: end,
                    note,
                    repeat,
                    skip: Vec::new(),
                });
                self.status = String::from("event added");
                id
            }
        };
        self.save_quiet();
        // Follow a new event to its day; an edited one stays where the
        // cursor is when it still happens there.
        let here = self
            .data
            .calendar_events
            .iter()
            .find(|e| e.id == id)
            .is_some_and(|e| crate::calendar::occurs_on(e, self.calendar_date));
        if !here {
            if let Ok(day) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                self.calendar_date = day;
            }
        }
        self.select_calendar(|item| matches!(item, AgendaItem::Event(e) if e.id == id));
    }

    /// Store a class edit as local overrides of the feed. Only what differs
    /// from the feed is kept, so later feed changes still show through
    /// everywhere else; an empty title goes back to the feed's. With
    /// `all_meetings`, the fields changed in the form go to every meeting
    /// sharing this one's Yedion title.
    fn save_class_edit(&mut self, id: &str, form: &EventForm, title: String, note: String, time: (String, String)) {
        let Some(picked) = self.calendar_feed.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let changed = |i: usize| form.fields[i].trim() != form.opened[i].trim();
        let targets: Vec<Class> = if form.all_meetings {
            self.calendar_feed.iter().filter(|c| c.title == picked.title && !c.id.is_empty()).cloned().collect()
        } else {
            vec![picked.clone()]
        };
        for class in &targets {
            let mut edit = self
                .data
                .calendar_class_edits
                .iter()
                .find(|e| e.id == class.id)
                .cloned()
                .unwrap_or_else(|| ClassEdit { id: class.id.clone(), ..Default::default() });
            let set = |slot: &mut Option<String>, value: &str, original: &str| {
                *slot = (value != original).then(|| value.to_string());
            };
            let only_one = !form.all_meetings;
            if only_one || changed(EventForm::TITLE) {
                let value = if title.is_empty() { class.title.as_str() } else { title.as_str() };
                set(&mut edit.title, value, &class.title);
            }
            if only_one || changed(EventForm::NOTE) {
                set(&mut edit.note, &note, &class.note);
            }
            if only_one || changed(EventForm::TIME) {
                set(&mut edit.start_time, &time.0, &class.start);
                set(&mut edit.end_time, &time.1, &class.end);
            }
            self.data.calendar_class_edits.retain(|e| e.id != class.id);
            if edit != (ClassEdit { id: class.id.clone(), ..Default::default() }) {
                self.data.calendar_class_edits.push(edit);
            }
        }
        let name = if title.is_empty() { picked.title.clone() } else { title };
        self.status = if form.all_meetings {
            format!("updated {} meetings of {}", targets.len(), name)
        } else {
            format!("updated {}", name)
        };
        self.save_quiet();
        self.select_calendar(|item| matches!(item, AgendaItem::Class(c) if c.id == id));
    }
}
