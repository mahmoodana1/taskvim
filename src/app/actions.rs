//! The mutating operations on `App`: adding, completing, deleting,
//! reordering, and pinning tasks, sections, and courses.

use chrono::{Duration as CDuration, Local, Months};
use rand::Rng;

use crate::app::{App, InsertTarget, Mode, Pane, Row, View};
use crate::due::parse_due;
use crate::model::{LogEntry, Project, Section, Task};
use crate::ui::theme::PALETTE;

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

    /// Strip trailing `!due` token and a trailing `@course`, in either order.
    /// Examples: "read ch.4 @algo !fri+3"  ·  "lab 2 !2026-07-10 @stat"
    pub fn parse_task_input(&self, buf: &str) -> (String, Option<u64>, Option<String>) {
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
           completed: false,
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
        self.data.next_color = (self.data.next_color + 1) % PALETTE.len();
        self.data.projects.push(Project { id, name, color });
        self.course_sel = self.data.projects.len() - 1;
        self.status = String::from("course added — ⏎ opens it");
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
                // Wiring the back-link is Task 10/11's job; leave unset here.
                task: None,
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

    pub fn toggle_pin(&mut self, tid: u64) {
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

    pub fn make_priority(&mut self, tid: u64) {
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
        self.task_sel = 0;
        self.pane = Pane::Tasks;
        self.status = String::from("random task pulled from the backlog → current priority");
        self.save_quiet();
    }

    pub fn move_dash_task(&mut self, delta: i32) {
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

    pub fn confirm_insert(&mut self, target: InsertTarget, buf: String) {
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

    pub fn open_course(&mut self, pid: u64) {
        self.cur_course = Some(pid);
        self.course_row = 0;
        self.view = View::Course;
        self.status = String::from(
            "a:task S:section p:link→dashboard c:priority t:due e:rename m:move dd:delete q:close",
        );
    }
}
