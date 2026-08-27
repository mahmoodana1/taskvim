# Course Archiving Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Archive a course with `x`, restore it from a modal opened with `X`, and make the `COURSES` panel say when it is holding more than it can show.

**Architecture:** Two `#[serde(default)]` fields carry the state — `Project.archived` and `Task.relink`. Archiving unlinks the course's dashboard tasks and flags them; un-archiving re-links exactly those. A `visible_courses()` accessor becomes the single source of truth for what the Courses pane shows, because `course_sel` currently indexes `data.projects` and would otherwise point at the wrong course once archived ones are filtered out.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28, serde/serde_json, chrono 0.4.

**Spec:** `docs/superpowers/specs/2026-08-27-taskvim-course-archive-design.md`

---

## ⚠️ MANDATORY for every task: data file isolation

`cargo test` and the binary write to the user's REAL data file at `~/.local/share/taskvim/data.json`, which holds 74 real tasks. **It was clobbered once already during this project.** Always:

```bash
export XDG_DATA_HOME=/tmp/taskvim-scratch
cargo test
```

Never run `cargo test` or the binary without it. Never touch anything under `~/.local/share/taskvim/`. A demo fixture for visual checks lives at `~/.local/share/taskvim-demo`.

**Baseline: 59 tests pass, build warning-free.**

**After each task, if the binary changed, the controller re-installs it** — `install -Dm755 target/release/taskvim ~/.local/bin/taskvim`. Implementers do not need to.

---

## File Structure

| File | Change |
|---|---|
| `src/model.rs` | `Project.archived`, `Task.relink`, v4 migration |
| `src/app/mod.rs` | `visible_courses()`, `selected_course()`, `View::Archived`, `clamp()` |
| `src/app/actions.rs` | `archive_course()`, `unarchive_course()` |
| `src/app/keys.rs` | `x` / `X`, `:archived`, `handle_archived`, every `course_sel` site |
| `src/ui/courses.rs` | `draw_archived_modal`, overflow indicator, footer hint |
| `src/ui/mod.rs` | dispatch `View::Archived` |
| `src/ui/help.rs`, `README.md` | document the keys |

---

### Task 1: Data model and v4 migration

**Files:**
- Modify: `src/model.rs`
- Modify: `src/app/tests.rs` (the `task()` helper constructs `Task` literally)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/model.rs`. It already has a `v2_json()` helper and migration tests — reuse it, since a v2 file must still migrate all the way to v4:

```rust
    #[test]
    fn migrates_to_v4_with_archive_fields_defaulted() {
        let mut d: Data = serde_json::from_str(v2_json()).expect("v2 json parses");
        d.sanitize();

        assert_eq!(d.version, 4);
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
        assert_eq!(back.version, 4);
    }
```

- [ ] **Step 2: Run and watch it FAIL**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test migrates_to_v4 2>&1 | tail -20`
Expected: FAIL — compile error, `struct 'Project' has no field named 'archived'`.

Report the actual failure output.

- [ ] **Step 3: Add the fields**

In `src/model.rs`, add to `Project`:

```rust
    /// Archived courses are hidden from the COURSES panel and from Tab
    /// cycling. Their tasks still exist and still count toward this course's
    /// own progress.
    #[serde(default)]
    pub archived: bool,
```

and to `Task`:

```rust
    /// This task was on the dashboard when its course was archived, so
    /// un-archiving should put it back. Lives on the task rather than as a
    /// list of ids on the project, so it cannot dangle — it dies with the
    /// task it belongs to.
    #[serde(default)]
    pub relink: bool,
```

- [ ] **Step 4: Bump the migration**

In `Data::sanitize`, after the existing `if self.version < 3 { … }` block:

```rust
        // v4: Project gained `archived`, Task gained `relink`. Both are
        // `#[serde(default)]`, so migration is just the version bump.
        if self.version < 4 {
            self.version = 4;
        }
```

- [ ] **Step 5: Fix every `Project {` and `Task {` construction site**

Run: `grep -rn "Project {" src/ | grep -v "^src/model.rs"` and `grep -rn "Task {" src/ | grep -v "^src/model.rs"`

Add `archived: false` / `relink: false` to each struct literal found. Known sites: `add_course` in `src/app/actions.rs`, `add_task` in `src/app/actions.rs`, the `task()` helper in `src/app/tests.rs`, and the `habits_are_excluded_from_course_progress` test which builds a `Project` inline. **Report every site you touched.**

- [ ] **Step 6: Run the tests**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **61 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 7: Commit**

```bash
git add src/model.rs src/app
git commit -m "feat: Project.archived and Task.relink, migrated to v4"
```

---

### Task 2: `visible_courses()` and the `course_sel` conversion

This is the risky task. `course_sel` indexes `data.projects` today; once archived courses are filtered out it must index the visible list instead, or `⏎`, `A`, `e` and `dd` silently act on the wrong course.

**Files:**
- Modify: `src/app/mod.rs`, `src/app/keys.rs`, `src/ui/courses.rs`, `src/app/actions.rs`
- Modify: `src/app/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/app/tests.rs`:

```rust
fn course(id: u64, name: &str) -> crate::model::Project {
    crate::model::Project {
        id,
        name: name.to_string(),
        color: 0,
        archived: false,
    }
}

#[test]
fn visible_courses_excludes_archived_and_keeps_order() {
    let mut app = test_app(vec![]);
    app.data.projects = vec![course(1, "Algo"), course(2, "Stats"), course(3, "Calc")];
    app.data.projects[1].archived = true;

    let names: Vec<&str> = app
        .visible_courses()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["Algo", "Calc"]);
}

#[test]
fn selected_course_resolves_against_the_visible_list() {
    let mut app = test_app(vec![]);
    app.data.projects = vec![course(1, "Algo"), course(2, "Stats"), course(3, "Calc")];
    app.data.projects[1].archived = true;

    // Row 1 on screen is "Calc" — the third project — because Stats is hidden.
    app.course_sel = 1;
    assert_eq!(app.selected_course(), Some(3));

    // Past the end resolves to nothing rather than the wrong course.
    app.course_sel = 9;
    assert_eq!(app.selected_course(), None);
}

#[test]
fn clamp_bounds_course_sel_against_visible_courses() {
    let mut app = test_app(vec![]);
    app.data.projects = vec![course(1, "Algo"), course(2, "Stats")];
    app.data.projects[1].archived = true;
    app.course_sel = 1; // valid before archiving, out of range after

    app.clamp();

    assert_eq!(app.course_sel, 0);
}
```

- [ ] **Step 2: Run and watch it FAIL**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test visible_courses 2>&1 | tail -20`
Expected: FAIL — `no method named 'visible_courses' found for struct 'App'`.

- [ ] **Step 3: Add the accessors**

Add to `impl App` in `src/app/mod.rs`:

```rust
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
```

Import `Project` in `src/app/mod.rs` if it isn't already.

- [ ] **Step 4: Fix `clamp()`**

In `src/app/mod.rs`, replace the `course_sel` bound:

```rust
        let visible_courses = self.visible_courses().len();
        if self.course_sel >= visible_courses {
            self.course_sel = visible_courses.saturating_sub(1);
        }
```

- [ ] **Step 5: Convert every call site**

Run `grep -rn "course_sel" src/ | grep -v tests.rs` and convert each. The sites, and what each becomes:

- `src/app/keys.rs` in `handle_dashboard` — `j` bound: `if self.course_sel + 1 < self.visible_courses().len()`
- `src/app/keys.rs` — `G` bound: `self.course_sel = self.visible_courses().len().saturating_sub(1)`
- `src/app/keys.rs` — the four `if let Some(p) = self.data.projects.get(self.course_sel)` blocks (`A` quick-add, `⏎` open, `e` rename, `dd` delete). Each becomes:

```rust
                    if let Some(pid) = self.selected_course() {
```

and the body uses `pid` directly instead of `p.id`. Where the body also needs the name (the `e` rename pre-fill), fetch it with `self.project(pid).map(|p| p.name.clone()).unwrap_or_default()`.

- `src/app/keys.rs:246` — `Tab` cycling inside the course modal. This walks to the next course and must skip archived ones. Replace the index maths with:

```rust
                    let visible: Vec<u64> = self.visible_courses().iter().map(|p| p.id).collect();
                    if !visible.is_empty() {
                        let cur = self.cur_course.and_then(|c| visible.iter().position(|id| *id == c));
                        let next = cur.map(|i| (i + 1) % visible.len()).unwrap_or(0);
                        self.open_course(visible[next]);
                    }
```

Read the existing `Tab` arm first and preserve whatever else it does (it also sets `course_sel`; keep that in sync by setting `self.course_sel = next`).

- `src/ui/courses.rs` — the `ListState` selection clamp: `app.course_sel.min(app.visible_courses().len().saturating_sub(1))`, and the `items` iteration must iterate `app.visible_courses()` rather than `app.data.projects`.
- `src/app/actions.rs` in `add_course` — `self.course_sel = self.data.projects.len() - 1;` becomes `self.course_sel = self.visible_courses().len().saturating_sub(1);` (a new course is never archived, so it is last in the visible list).

**Report every site you changed.** After converting, `grep -rn "projects.get(self.course_sel)\|projects.len()" src/app/keys.rs` must return nothing.

- [ ] **Step 6: Run the tests**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **64 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 7: Commit**

```bash
git add src/app src/ui/courses.rs
git commit -m "refactor: course_sel indexes visible courses, not all projects"
```

---

### Task 3: Archive and un-archive

**Files:**
- Modify: `src/app/actions.rs`, `src/app/tests.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/app/tests.rs`:

```rust
/// A course with two tasks, both on the dashboard.
fn app_with_course() -> App {
    let mut a = test_app(vec![]);
    a.data.projects = vec![course(1, "Stats"), course(2, "Algo")];
    let mut t1 = task(10, "lab 2");
    t1.project = Some(1);
    let mut t2 = task(11, "quiz 4");
    t2.project = Some(1);
    let mut other = task(12, "lecture");
    other.project = Some(2);
    a.data.tasks = vec![t1, t2, other];
    a.data.dashboard = vec![10, 11, 12];
    a
}

#[test]
fn archiving_unlinks_and_flags_only_that_courses_tasks() {
    let mut app = app_with_course();

    app.archive_course(1);

    assert!(app.data.projects[0].archived);
    assert_eq!(app.data.dashboard, vec![12], "only the other course's task left");
    assert!(app.data.tasks[0].relink);
    assert!(app.data.tasks[1].relink);
    assert!(!app.data.tasks[2].relink, "the other course is untouched");
}

#[test]
fn unarchiving_relinks_exactly_the_flagged_tasks() {
    let mut app = app_with_course();
    app.archive_course(1);

    app.unarchive_course(1);

    assert!(!app.data.projects[0].archived);
    assert!(app.data.dashboard.contains(&10));
    assert!(app.data.dashboard.contains(&11));
    assert!(!app.data.tasks[0].relink, "flag is cleared after restoring");
}

#[test]
fn unarchiving_skips_a_task_completed_while_archived() {
    let mut app = app_with_course();
    app.archive_course(1);
    app.data.tasks[0].completed = true;

    app.unarchive_course(1);

    assert!(!app.data.dashboard.contains(&10), "finished work stays finished");
    assert!(app.data.dashboard.contains(&11));
    assert!(!app.data.tasks[0].relink, "flag cleared either way");
}

#[test]
fn unarchiving_does_not_double_add_a_manually_relinked_task() {
    let mut app = app_with_course();
    app.archive_course(1);
    app.data.dashboard.push(10); // user re-linked it by hand with p

    app.unarchive_course(1);

    assert_eq!(
        app.data.dashboard.iter().filter(|id| **id == 10).count(),
        1,
        "task 10 appears exactly once"
    );
}
```

- [ ] **Step 2: Run and watch it FAIL**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test archiv 2>&1 | tail -20`
Expected: FAIL — `no method named 'archive_course' found for struct 'App'`.

- [ ] **Step 3: Implement**

Add to `impl App` in `src/app/actions.rs`:

```rust
    /// Archive a course: hide it, and take its tasks off the dashboard while
    /// remembering which ones were there so `unarchive_course` can put exactly
    /// those back.
    pub fn archive_course(&mut self, pid: u64) {
        let ids: Vec<u64> = self
            .data
            .tasks
            .iter()
            .filter(|t| t.project == Some(pid) && self.data.dashboard.contains(&t.id))
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
        let mut restored = 0usize;
        let mut to_link: Vec<u64> = Vec::new();
        for t in self.data.tasks.iter_mut() {
            if t.project == Some(pid) && t.relink {
                t.relink = false;
                if !t.completed {
                    to_link.push(t.id);
                }
            }
        }
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
```

Note the borrow in `archive_course`: the filter closure reads `self.data.dashboard` while iterating `self.data.tasks`. If the borrow checker objects, clone the dashboard into a local first (`let dash = self.data.dashboard.clone();`) and check against that. Report which you needed.

- [ ] **Step 4: Run the tests**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **68 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output. A `dead_code` warning is expected for both new methods — Task 4 binds the keys. Add a narrow `#[allow(dead_code)]` on each with a comment naming Task 4, following the convention used elsewhere in this codebase, and remove it in Task 4.

- [ ] **Step 5: Commit**

```bash
git add src/app
git commit -m "feat: archive and un-archive a course, remembering its dashboard links"
```

---

### Task 4: The archive modal and its keys

**Files:**
- Modify: `src/app/mod.rs` (`View::Archived`), `src/app/keys.rs`, `src/ui/courses.rs`, `src/ui/mod.rs`
- Modify: `src/app/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/app/tests.rs`:

```rust
#[test]
fn x_archives_the_selected_course_and_capital_x_opens_the_archive() {
    let mut app = app_with_course();
    app.pane = crate::app::Pane::Courses;
    app.course_sel = 0; // "Stats"

    app.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(app.data.projects[0].archived);
    assert_eq!(app.visible_courses().len(), 1);

    app.handle_key(KeyCode::Char('X'), KeyModifiers::NONE);
    assert_eq!(app.view, crate::app::View::Archived);
}
```

`View` needs `PartialEq` and `Debug` for that assertion — it already derives `PartialEq, Clone, Copy`; add `Debug` if the compiler asks.

- [ ] **Step 2: Run and watch it FAIL**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test x_archives 2>&1 | tail -20`
Expected: FAIL — `no variant named 'Archived' found for enum 'View'`.

- [ ] **Step 3: Add the view**

In `src/app/mod.rs`, add to `View`:

```rust
    Archived, // modal over the dashboard listing archived courses
```

Add a cursor field to `App`, initialised to 0 in `with_data`:

```rust
    /// Cursor inside the archive modal.
    pub archived_sel: usize,
```

- [ ] **Step 4: Bind the keys**

In `src/app/keys.rs`, in `handle_dashboard`:

```rust
            KeyCode::Char('x') => {
                if self.pane == Pane::Courses {
                    if let Some(pid) = self.selected_course() {
                        self.archive_course(pid);
                    }
                }
            }
            KeyCode::Char('X') => {
                self.archived_sel = 0;
                self.view = View::Archived;
            }
```

Add the handler beside `handle_log`:

```rust
    fn handle_archived(&mut self, code: KeyCode, pend: Option<char>) {
        let n = self.archived_courses().len();
        match code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('X') => {
                self.view = View::Dashboard;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.archived_sel + 1 < n {
                    self.archived_sel += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.archived_sel = self.archived_sel.saturating_sub(1)
            }
            KeyCode::Char('g') => {
                if pend == Some('g') {
                    self.archived_sel = 0;
                } else {
                    self.pending = Some('g');
                }
            }
            KeyCode::Char('G') => self.archived_sel = n.saturating_sub(1),
            KeyCode::Char('x') => {
                let pid = self.archived_courses().get(self.archived_sel).map(|p| p.id);
                if let Some(pid) = pid {
                    self.unarchive_course(pid);
                    self.archived_sel = self
                        .archived_sel
                        .min(self.archived_courses().len().saturating_sub(1));
                    if self.archived_courses().is_empty() {
                        self.view = View::Dashboard;
                    }
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let pid = self.archived_courses().get(self.archived_sel).map(|p| p.id);
                if let Some(pid) = pid {
                    // Readable without restoring: archiving never makes a
                    // course's tasks unreachable.
                    self.open_course(pid);
                }
            }
            KeyCode::Char('?') => self.view = View::Help,
            KeyCode::Char(':') => self.mode = Mode::Command(String::new()),
            _ => {}
        }
    }
```

Dispatch it in `handle_key`'s `Mode::Normal` match, beside the other views:

```rust
                    View::Archived => self.handle_archived(code, pend),
```

Add the ex-command in `run_command`, beside `:log`:

```rust
            "archived" => {
                self.archived_sel = 0;
                self.view = View::Archived;
            }
```

Read the existing `run_command` match first and follow its exact shape (it may match on a trimmed `&str`).

Remove the `#[allow(dead_code)]` attributes added to `archive_course` / `unarchive_course` in Task 3.

- [ ] **Step 5: Draw the modal**

Add to `src/ui/courses.rs`, modelled on `draw_course_modal` — same `Clear`, border, title, footer and content sizing:

```rust
pub fn draw_archived_modal(f: &mut Frame, app: &App, area: Rect) {
    let courses = app.archived_courses();

    let w = area.width.saturating_sub(8).min(70).max(36);
    let h = (courses.len() as u16 + 2).clamp(5, area.height.saturating_sub(4).max(5));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            format!(
                " ARCHIVED — {} course{} ",
                courses.len(),
                if courses.len() == 1 { "" } else { "s" }
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " x:restore  ⏎:open  q:close ",
            Style::default().fg(DIM),
        )));

    if courses.is_empty() {
        f.render_widget(
            Paragraph::new(" nothing archived — press x on a course to archive it")
                .style(Style::default().fg(DIM))
                .block(block),
            popup,
        );
        return;
    }

    let items: Vec<ListItem> = courses
        .iter()
        .map(|p| {
            let (done, total) = app.project_stats(p.id);
            ListItem::new(Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(DIM)),
                Span::styled(p.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("   {}/{} done", done, total),
                    Style::default().fg(DIM),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(
        app.archived_sel.min(courses.len().saturating_sub(1)),
    ));
    f.render_stateful_widget(list, popup, &mut state);
}
```

In `src/ui/mod.rs`, dispatch it. Read `ui()` first and follow how `View::Course` is handled — the archive modal draws over the dashboard the same way:

```rust
        View::Archived => {
            draw_dashboard(f, app, body);
            draw_archived_modal(f, app, body);
        }
```

- [ ] **Step 6: Verify**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **69 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

Then drive it for real:

```bash
mkdir -p /tmp/taskvim-scratch/taskvim
cat > /tmp/taskvim-scratch/taskvim/data.json <<'JSON'
{"version":4,"tasks":[
 {"id":1,"text":"lab 2","project":50,"section":null,"due":null,"completed":false,"dash_section":"general","repeat":null,"period":null,"relink":false},
 {"id":2,"text":"buy milk","project":null,"section":null,"due":null,"completed":false,"dash_section":"general","repeat":null,"period":null,"relink":false}],
 "projects":[{"id":50,"name":"Statistics","color":0,"archived":false},
             {"id":51,"name":"Algorithms 2","color":1,"archived":false}],
 "sections":[],"log":[],"dashboard":[1,2],"daily_goal":2,"next_id":100,"next_color":2}
JSON
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 110 -y 32 "XDG_DATA_HOME=/tmp/taskvim-scratch ./target/release/taskvim"
sleep 2
tmux send-keys -t tv Tab; sleep 1
tmux capture-pane -t tv -p | head -14      # two courses, "lab 2" on the dashboard
tmux send-keys -t tv x; sleep 1
tmux capture-pane -t tv -p | head -14      # Statistics gone, lab 2 unlinked
tmux send-keys -t tv X; sleep 1
tmux capture-pane -t tv -p | head -16      # THE ARCHIVE MODAL
tmux send-keys -t tv x; sleep 1
tmux capture-pane -t tv -p | head -14      # restored, lab 2 back
tmux kill-session -t tv
```

**Paste all four captures.** Verify: archiving removes the card and unlinks `lab 2`; the modal lists Statistics; restoring brings both back.

- [ ] **Step 7: Commit**

```bash
git add src/app src/ui
git commit -m "feat: archive modal, x to archive and X to open it"
```

---

### Task 5: Overflow indicator and documentation

**Files:**
- Modify: `src/ui/courses.rs`, `src/ui/help.rs`, `README.md`

- [ ] **Step 1: Replace the hidden-count title**

`draw_courses` currently computes `hidden` and renders `COURSES (+N hidden)`. Replace that block with:

```rust
    let courses = app.visible_courses();
    let inner_h = area.height.saturating_sub(2) as usize;
    let max_visible = inner_h / 2; // two rows per course
    let title = if courses.len() > max_visible {
        format!(" COURSES  {} of {} ▾ ", max_visible, courses.len())
    } else {
        format!(" COURSES  {} ", courses.len())
    };
```

Update the footer hint to include archiving:

```rust
        .title_bottom(Line::from(Span::styled(
            " ⏎:open a:new x:archive X:archived e:rename dd:delete ",
            Style::default().fg(DIM),
        )));
```

- [ ] **Step 2: Verify the indicator**

```bash
python3 - <<'PY'
import json, os
p = "/tmp/taskvim-scratch/taskvim/data.json"
d = json.load(open(p))
d["projects"] = [{"id": 100+i, "name": f"Course {i}", "color": i % 6, "archived": False}
                 for i in range(9)]
json.dump(d, open(p, "w"))
PY
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 110 -y 32 "XDG_DATA_HOME=/tmp/taskvim-scratch ./target/release/taskvim"
sleep 2; tmux capture-pane -t tv -p | grep -A3 COURSES; tmux kill-session -t tv
```

Expected: the title reads `COURSES  N of 9 ▾`. **Paste the capture.** If the panel is capped such that all nine fit, note the real numbers rather than forcing the expected ones.

- [ ] **Step 3: Update the help screen**

In `src/ui/help.rs`, add to the DASHBOARD section:

```rust
        key("x / X", "archive the selected course · open the archive"),
```

and to the EX COMMANDS line, extend the existing text to mention `:archived`.

Check the help screen still fits after adding a line — it was compressed once already to fit 44 rows. Render it (`?`) at 100x44 and confirm nothing is clipped. If it no longer fits, tighten wording rather than letting it clip, and say what you changed.

- [ ] **Step 4: Update the README**

Add to the Dashboard key table:

```markdown
| `x` / `X` | archive the selected course · open the archive to restore |
```

And a short section after the courses description:

```markdown
### Archiving courses

`x` on a course card archives it: the card disappears and its tasks come off
your dashboard, but nothing is deleted. `X` (or `:archived`) opens the archive,
where `x` restores a course and puts exactly those tasks back — skipping any
you finished in the meantime. `⏎` there opens an archived course read-only, so
archiving never puts its tasks out of reach.
```

- [ ] **Step 5: Final verification**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test && cargo build --release 2>&1 | grep -E "^(warning|error)"`
Expected: 69 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/ui README.md
git commit -m "feat: COURSES overflow indicator; document archiving"
```

---

## Notes for the implementer

**Task 2 is the one to be careful with.** A missed `course_sel` site means `⏎` or `dd` silently acts on a different course than the one highlighted — no crash, no error, just the wrong thing. Grep is the proof, not inspection.

**`undo` already covers archiving.** `archive_course` calls `save_quiet()`, which checkpoints, so `u` reverses it including the unlinked tasks. No special handling needed — but do not add any `checkpoint()` calls of your own.

**Do not touch the heatmap, log, or `project_stats`.** An archived course's tasks still count toward its own progress; that is intentional and the archive modal displays it.
