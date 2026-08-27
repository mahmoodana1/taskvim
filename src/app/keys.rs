//! Keyboard input: the top-level dispatch plus the per-view handlers
//! and ex-commands.

use chrono::{Datelike, Local};
use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, InsertTarget, Mode, Pane, Row, View};
use crate::model::{Data, DashSection, Repeat, RepeatUnit};

impl App {
    pub fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
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
            Mode::RepeatMenu { task } => self.handle_repeat_menu(task, code),
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

    fn handle_repeat_menu(&mut self, task: u64, code: KeyCode) {
        let anchor = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let mk = |every, unit| Some(Repeat { every, unit, anchor: anchor.clone() });
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Char('1') | KeyCode::Char('d') => self.set_repeat(task, mk(1, RepeatUnit::Day)),
            KeyCode::Char('2') => self.set_repeat(task, mk(3, RepeatUnit::Day)),
            KeyCode::Char('3') | KeyCode::Char('w') => self.set_repeat(task, mk(1, RepeatUnit::Week)),
            KeyCode::Char('4') => self.set_repeat(task, mk(2, RepeatUnit::Week)),
            KeyCode::Char('5') | KeyCode::Char('m') => self.set_repeat(task, mk(1, RepeatUnit::Month)),
            KeyCode::Char('0') | KeyCode::Char('n') | KeyCode::Char('-') => {
                self.set_repeat(task, None)
            }
            KeyCode::Char('c') | KeyCode::Char('i') | KeyCode::Char('/') => {
                self.mode = Mode::Insert(InsertTarget::Repeat { task }, String::new());
                return;
            }
            _ => return,
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
            KeyCode::Char('~') => {
                if let Some(Row::Task(tid)) = cur {
                    self.mode = Mode::RepeatMenu { task: tid };
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
                    if self.task_sel + 1 < self.dash_rows().len() {
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
                Pane::Tasks => self.task_sel = self.dash_rows().len().saturating_sub(1),
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
                    if let Some(tid) = self.selected_task() {
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
                    if let Some(tid) = self.selected_task() {
                        self.make_priority(tid);
                    }
                }
            }
            KeyCode::Char('u') => self.undo(),

            // move a task into a fixed dashboard section
            KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
                if self.pane == Pane::Tasks {
                    if let Some(tid) = self.selected_task() {
                        let sec = match code {
                            KeyCode::Char('1') => DashSection::Important,
                            KeyCode::Char('2') => DashSection::General,
                            _ => DashSection::Agent,
                        };
                        self.set_dash_section(tid, sec);
                    } else {
                        self.status = String::from("put the cursor on a task first");
                    }
                }
            }

            // rename / due on dashboard tasks & courses
            KeyCode::Char('e') => match self.pane {
                Pane::Tasks => {
                    if let Some(tid) = self.selected_task() {
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
                    if let Some(tid) = self.selected_task() {
                        let pre = self.task(tid).and_then(|t| t.due.clone()).unwrap_or_default();
                        self.mode = Mode::Insert(InsertTarget::Due { task: tid }, pre);
                    }
                }
            }
            KeyCode::Char('~') => {
                if self.pane == Pane::Tasks {
                    if let Some(tid) = self.selected_task() {
                        self.mode = Mode::RepeatMenu { task: tid };
                    }
                }
            }

            // delete: dd unlinks course tasks / deletes loose ones; D always deletes
            KeyCode::Char('d') => {
                if pend == Some('d') {
                    match self.pane {
                        Pane::Tasks => {
                            if let Some(tid) = self.selected_task() {
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
                    if let Some(tid) = self.selected_task() {
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
