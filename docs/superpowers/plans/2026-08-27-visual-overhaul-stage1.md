# Visual Overhaul — Stage 1: Boxed Sections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the three dashboard sections as colour-coded bordered boxes with inline badges, so each task is one row and the grouping is unmistakable.

**Architecture:** The box borders are drawn as rows inside the existing single `List` widget, not as separate widgets — this keeps unified `j`/`k` scrolling and `ListState` selection intact. `DashRow` grows from two variants to four so that `App.task_sel` stays 1:1 with rendered items; the two extra variants are non-selectable and movement skips them.

**Tech Stack:** Rust 2021, ratatui 0.29, crossterm 0.28. `ratatui::text::Line::width()` provides Unicode display width — no new dependency needed for border alignment.

**Spec:** `docs/superpowers/specs/2026-08-27-taskvim-visual-overhaul-design.md`

---

## ⚠️ MANDATORY for every task: data file isolation

`cargo test` and the binary write to the user's REAL data file at `~/.local/share/taskvim/data.json`, which holds 74 real tasks. **It was clobbered once already during v0.3.** Always:

```bash
export XDG_DATA_HOME=/tmp/taskvim-scratch
cargo test
```

Never run `cargo test` or the binary without it. Never touch anything under `~/.local/share/taskvim/`.

A demo fixture for visual checks lives at `~/.local/share/taskvim-demo/taskvim/data.json` — use `XDG_DATA_HOME=~/.local/share/taskvim-demo` to render against it.

**Baseline: 52 tests pass, build is warning-free.**

---

## File Structure

| File | Change |
|---|---|
| `src/app/mod.rs` | `DashRow` gains `SectionFill` / `SectionBottom`; `dash_rows()` emits them; `Section` renamed `SectionTop` |
| `src/app/keys.rs` | `j` / `k` / `G` skip non-selectable rows |
| `src/app/tests.rs` | row-model and navigation tests |
| `src/ui/theme.rs` | `section_color()` |
| `src/ui/dashboard.rs` | box-drawing helpers; `task_line` badge goes inline; `draw_task_sidebar` renders boxes |

---

### Task 1: Extend the row model

**Files:**
- Modify: `src/app/mod.rs` (`DashRow`, `dash_rows`)
- Modify: `src/app/tests.rs`

- [ ] **Step 1: Write the failing tests**

Replace the existing `dash_rows_groups_tasks_under_all_three_headers`, `dash_rows_renders_empty_section_headers` and `dash_rows_preserves_dashboard_order_within_a_section` tests in `src/app/tests.rs` with these — the old ones assert the two-variant shape and are superseded:

```rust
#[test]
fn dash_rows_wraps_each_section_in_a_box() {
    let app = test_app(vec![
        sectioned(1, "b", DashSection::General),
        sectioned(2, "a", DashSection::Important),
        sectioned(3, "c", DashSection::Agent),
    ]);

    assert_eq!(
        app.dash_rows(),
        vec![
            DashRow::SectionTop(DashSection::Important),
            DashRow::Task(2),
            DashRow::SectionBottom(DashSection::Important),
            DashRow::SectionTop(DashSection::General),
            DashRow::Task(1),
            DashRow::SectionBottom(DashSection::General),
            DashRow::SectionTop(DashSection::Agent),
            DashRow::Task(3),
            DashRow::SectionBottom(DashSection::Agent),
        ]
    );
}

#[test]
fn an_empty_section_still_renders_a_full_box() {
    let app = test_app(vec![sectioned(1, "only", DashSection::General)]);

    assert_eq!(
        app.dash_rows(),
        vec![
            DashRow::SectionTop(DashSection::Important),
            DashRow::SectionFill(DashSection::Important),
            DashRow::SectionBottom(DashSection::Important),
            DashRow::SectionTop(DashSection::General),
            DashRow::Task(1),
            DashRow::SectionBottom(DashSection::General),
            DashRow::SectionTop(DashSection::Agent),
            DashRow::SectionFill(DashSection::Agent),
            DashRow::SectionBottom(DashSection::Agent),
        ],
        "empty sections get a blank interior row so the box is never collapsed"
    );
}

#[test]
fn dash_rows_preserves_dashboard_order_within_a_section() {
    let mut app = test_app(vec![
        sectioned(1, "first", DashSection::General),
        sectioned(2, "second", DashSection::General),
    ]);
    app.data.dashboard = vec![2, 1];

    let rows = app.dash_rows();
    let general: Vec<&DashRow> = rows
        .iter()
        .skip_while(|r| **r != DashRow::SectionTop(DashSection::General))
        .take_while(|r| **r != DashRow::SectionBottom(DashSection::General))
        .collect();

    assert_eq!(
        general,
        vec![
            &DashRow::SectionTop(DashSection::General),
            &DashRow::Task(2),
            &DashRow::Task(1),
        ]
    );
}

#[test]
fn selected_task_is_none_on_any_box_chrome_row() {
    let mut app = test_app(vec![sectioned(1, "x", DashSection::General)]);
    // rows: 0 top(Imp) 1 fill(Imp) 2 bottom(Imp) 3 top(Gen) 4 task 5 bottom(Gen) …
    for chrome in [0usize, 1, 2, 3, 5] {
        app.task_sel = chrome;
        assert_eq!(app.selected_task(), None, "row {} is chrome", chrome);
    }
    app.task_sel = 4;
    assert_eq!(app.selected_task(), Some(1));
}

#[test]
fn selected_section_still_walks_back_to_its_header() {
    let mut app = test_app(vec![sectioned(1, "x", DashSection::Agent)]);
    let rows = app.dash_rows();
    let pos = rows.iter().position(|r| *r == DashRow::Task(1)).unwrap();
    app.task_sel = pos;
    assert_eq!(app.selected_section(), DashSection::Agent);
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test dash_rows 2>&1 | tail -20`
Expected: FAIL — compile error, `no variant named 'SectionTop' found for enum 'DashRow'`.

- [ ] **Step 3: Extend the enum**

In `src/app/mod.rs`, replace the `DashRow` definition:

```rust
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
    pub fn is_selectable(&self) -> bool {
        matches!(self, DashRow::SectionTop(_) | DashRow::Task(_))
    }
}
```

- [ ] **Step 4: Emit the new rows**

Replace `dash_rows()` in `src/app/mod.rs`:

```rust
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
                if self.task(*id).map(|t| t.dash_section == sec).unwrap_or(false) {
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
```

- [ ] **Step 5: Fix the two matchers**

`selected_task()` matches `DashRow::Task` and needs no change. `selected_section()` matches `DashRow::Section(s)` — rename that pattern to `DashRow::SectionTop(s)`. Leave the rest of the function alone.

`src/ui/dashboard.rs` also matches `DashRow::Section` and will now fail to compile. Rename that arm and give the two new variants blank rows, so this task's commit builds and runs. Task 5 replaces all four arms with the real box rendering:

```rust
            DashRow::SectionTop(sec) => {
                let count = app
                    .dash_ids()
                    .iter()
                    .filter(|id| app.task(**id).map(|t| t.dash_section == *sec).unwrap_or(false))
                    .count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("▸ {}", sec.title()),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {}", count), Style::default().fg(DIM)),
                ]))
            }
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => task_line(app, t, false, true),
                None => ListItem::new(Line::from("")),
            },
            DashRow::SectionFill(_) | DashRow::SectionBottom(_) => {
                ListItem::new(Line::from(""))
            }
```

- [ ] **Step 6: Run the tests**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **54 passed; 0 failed** (52 baseline − 3 replaced + 5 new).

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 7: Commit**

```bash
git add src/app src/ui/dashboard.rs
git commit -m "feat: DashRow carries section-box chrome rows"
```

---

### Task 2: Movement skips box chrome

**Files:**
- Modify: `src/app/keys.rs` (`handle_dashboard`)
- Modify: `src/app/tests.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/app/tests.rs`:

```rust
use crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn moving_down_skips_box_chrome() {
    let mut app = test_app(vec![
        sectioned(1, "a", DashSection::Important),
        sectioned(2, "b", DashSection::General),
    ]);
    // rows: 0 top(Imp) 1 task1 2 bot(Imp) 3 top(Gen) 4 task2 5 bot(Gen)
    //       6 top(Agt) 7 fill(Agt) 8 bot(Agt)
    app.task_sel = 1; // on task 1

    app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(app.task_sel, 3, "skipped the Important box bottom");
    assert_eq!(app.selected_section(), DashSection::General);
}

#[test]
fn moving_up_skips_box_chrome() {
    let mut app = test_app(vec![
        sectioned(1, "a", DashSection::Important),
        sectioned(2, "b", DashSection::General),
    ]);
    app.task_sel = 4; // task 2, under General

    app.handle_key(KeyCode::Char('k'), KeyModifiers::NONE);

    assert_eq!(app.task_sel, 3, "General's header, not its top border chrome");
}

#[test]
fn moving_never_lands_on_a_non_selectable_row() {
    let mut app = test_app(vec![sectioned(1, "a", DashSection::General)]);

    for _ in 0..12 {
        app.handle_key(KeyCode::Char('j'), KeyModifiers::NONE);
        let row = app.dash_rows()[app.task_sel];
        assert!(row.is_selectable(), "landed on {:?}", row);
    }
    for _ in 0..12 {
        app.handle_key(KeyCode::Char('k'), KeyModifiers::NONE);
        let row = app.dash_rows()[app.task_sel];
        assert!(row.is_selectable(), "landed on {:?}", row);
    }
}

#[test]
fn g_jumps_to_the_last_selectable_row() {
    let mut app = test_app(vec![sectioned(1, "a", DashSection::Agent)]);

    app.handle_key(KeyCode::Char('G'), KeyModifiers::NONE);

    let row = app.dash_rows()[app.task_sel];
    assert_eq!(row, DashRow::Task(1), "G lands on the task, not the box bottom");
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test skips_box_chrome 2>&1 | tail -20`
Expected: FAIL — `assertion left == right failed: left: 2, right: 3`. `j` landed on the box bottom.

- [ ] **Step 3: Add the movement helpers**

Add to `impl App` in `src/app/mod.rs`:

```rust
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
        rows.iter()
            .rposition(|r| r.is_selectable())
            .unwrap_or(0)
    }
```

- [ ] **Step 4: Use them in the key handler**

In `src/app/keys.rs`, in `handle_dashboard`, replace the `Pane::Tasks` arms:

```rust
            KeyCode::Char('j') | KeyCode::Down => match self.pane {
                Pane::Tasks => self.step_dash_cursor(1),
                Pane::Courses => {
                    if self.course_sel + 1 < self.data.projects.len() {
                        self.course_sel += 1;
                    }
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.pane {
                Pane::Tasks => self.step_dash_cursor(-1),
                Pane::Courses => self.course_sel = self.course_sel.saturating_sub(1),
            },
```

and:

```rust
            KeyCode::Char('G') => match self.pane {
                Pane::Tasks => self.task_sel = self.last_dash_row(),
                Pane::Courses => self.course_sel = self.data.projects.len().saturating_sub(1),
            },
```

Leave `gg` alone — row 0 is `SectionTop(Important)`, which is selectable.

- [ ] **Step 5: Run the tests**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **58 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add src/app
git commit -m "feat: dashboard cursor skips section-box chrome"
```

---

### Task 3: Section colours

**Files:**
- Modify: `src/ui/theme.rs`

- [ ] **Step 1: Add the function**

In `src/ui/theme.rs`:

```rust
use crate::model::DashSection;

/// Each dashboard section owns a colour, used on its box border and title.
/// Section colour is chrome only — task text keeps status colour (overdue red,
/// due-today amber, habits dim) so the two systems never fight for the same
/// glyph.
pub fn section_color(sec: DashSection) -> Color {
    match sec {
        DashSection::Important => Color::LightRed,
        DashSection::General => Color::LightBlue,
        DashSection::Agent => Color::LightMagenta,
    }
}
```

- [ ] **Step 2: Build**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo build --release 2>&1 | grep -E "^(error)" | head`
Expected: no errors. A `dead_code` warning for `section_color` is expected here — Task 5 consumes it. Add a narrow `#[allow(dead_code)]` with a comment naming Task 5, following the convention already used in this codebase, and remove it in Task 5.

- [ ] **Step 3: Commit**

```bash
git add src/ui/theme.rs
git commit -m "feat: per-section accent colours"
```

---

### Task 4: Inline badges — one row per task

**Files:**
- Modify: `src/ui/dashboard.rs` (`task_line`)
- Modify: `src/ui/courses.rs` (its own inline badge rendering)

- [ ] **Step 1: Change `task_line` to return spans, not a ListItem**

`task_line` currently builds a two-line `ListItem`. The box renderer in Task 5 needs the content spans so it can measure and pad them. Replace `task_line` in `src/ui/dashboard.rs` with:

```rust
/// The content spans for one dashboard task: project tag, title, and the
/// trailing badge (cadence or due date — a task never has both). The caller
/// positions and pads these; `task_line` does not decide layout.
pub fn task_spans(app: &App, t: &Task) -> (Vec<Span<'static>>, Option<Span<'static>>) {
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

    // A task has a repeat or a due date, never both.
    let badge = if t.completed {
        None
    } else {
        repeat_span(&t.repeat).or_else(|| due_span(&t.due))
    };
    (spans, badge)
}
```

Delete the old `task_line`. The `show_pin` parameter goes away — it was only ever passed `false` from the dashboard.

- [ ] **Step 2: Fix the course modal**

`draw_course_modal` in `src/ui/courses.rs` renders its own task rows and calls `due_span`/`repeat_span` directly — it does NOT call `task_line`, so it does not break. Confirm with:

Run: `grep -n "task_line" src/ui/*.rs`
Expected: no matches outside `src/ui/dashboard.rs`. If there are matches, update those call sites to use `task_spans` and report it.

- [ ] **Step 3: Build**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo build --release 2>&1 | grep -E "^(error)" | head`
Expected: errors in `draw_task_sidebar` only, because it still calls `task_line`. That is expected — Task 5 rewrites it. To keep this task's commit compiling, temporarily render tasks with the spans joined on one line:

```rust
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => {
                    let (mut spans, badge) = task_spans(app, t);
                    if let Some(b) = badge {
                        spans.push(b);
                    }
                    ListItem::new(Line::from(spans))
                }
                None => ListItem::new(Line::from("")),
            },
```

- [ ] **Step 4: Verify**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **58 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 5: Commit**

```bash
git add src/ui
git commit -m "refactor: task_spans returns content + badge for the caller to lay out"
```

---

### Task 5: Draw the boxes

**Files:**
- Modify: `src/ui/dashboard.rs` (`draw_task_sidebar`)

- [ ] **Step 1: Add the box-drawing helpers**

Add to `src/ui/dashboard.rs`:

```rust
/// `┌ IMPORTANT ─────────── 2 ┐`
fn box_top(sec: DashSection, count: usize, width: usize, color: Color) -> ListItem<'static> {
    let title = format!(" {} ", sec.title().to_uppercase());
    let tail = format!(" {} ", count);
    // 2 corners + title + tail; the rule fills whatever is left.
    let rule = width
        .saturating_sub(2 + title.chars().count() + tail.chars().count())
        .max(1);
    ListItem::new(Line::from(vec![
        Span::styled(format!("┌{}", title), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled("─".repeat(rule), Style::default().fg(color)),
        Span::styled(tail, Style::default().fg(DIM)),
        Span::styled("┐", Style::default().fg(color)),
    ]))
}

/// `└─────────────────────────┘`
fn box_bottom(width: usize, color: Color) -> ListItem<'static> {
    let rule = width.saturating_sub(2);
    ListItem::new(Line::from(Span::styled(
        format!("└{}┘", "─".repeat(rule)),
        Style::default().fg(color),
    )))
}

/// `│                         │` — an empty section's interior.
fn box_fill(width: usize, color: Color) -> ListItem<'static> {
    let inner = width.saturating_sub(2);
    ListItem::new(Line::from(vec![
        Span::styled("│", Style::default().fg(color)),
        Span::raw(" ".repeat(inner)),
        Span::styled("│", Style::default().fg(color)),
    ]))
}

/// `│ buy groceries    ↻ daily │` — content left, badge right, padded so the
/// closing bar always lands in the same column.
fn box_task(
    app: &App,
    t: &Task,
    width: usize,
    color: Color,
) -> ListItem<'static> {
    let inner = width.saturating_sub(2);
    let (content, badge) = task_spans(app, t);

    let badge_w = badge.as_ref().map(|b| b.width()).unwrap_or(0);
    // 1 leading space + content + gap + badge + 1 trailing space
    let avail = inner.saturating_sub(badge_w + 2);

    let mut line: Vec<Span<'static>> = vec![
        Span::styled("│", Style::default().fg(color)),
        Span::raw(" "),
    ];
    let mut used = 0usize;
    for s in content {
        let w = s.width();
        if used + w <= avail {
            used += w;
            line.push(s);
        } else {
            // Truncate this span to what's left, with an ellipsis.
            let room = avail.saturating_sub(used);
            if room > 1 {
                let cut: String = s.content.chars().take(room - 1).collect();
                used += cut.chars().count() + 1;
                line.push(Span::styled(format!("{}…", cut), s.style));
            }
            break;
        }
    }

    let pad = inner.saturating_sub(used + badge_w + 1);
    line.push(Span::raw(" ".repeat(pad)));
    if let Some(b) = badge {
        line.push(b);
    }
    line.push(Span::raw(" "));
    line.push(Span::styled("│", Style::default().fg(color)));
    ListItem::new(Line::from(line))
}
```

Import what these need at the top of the file:

```rust
use crate::model::{DashSection, Task};
use crate::ui::theme::section_color;
```

- [ ] **Step 2: Rewrite `draw_task_sidebar`'s item builder**

Replace the `items` construction and the surrounding block in `draw_task_sidebar`:

```rust
    // Borderless: the section boxes ARE the panel. The block is kept only for
    // its title and footer hints — nesting a second border around the boxes
    // wastes four columns and reads as heavy.
    let block = Block::default()
        .borders(Borders::NONE)
        .title(Span::styled(
            format!(" tasks ({}) ", app.dash_ids().len()),
            Style::default().fg(border).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(DIM))));

    // Borderless block, so the list gets the panel's full width and the boxes
    // span it edge to edge.
    let width = area.width as usize;
    let rows = app.dash_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            DashRow::SectionTop(sec) => {
                let count = app
                    .dash_ids()
                    .iter()
                    .filter(|id| app.task(**id).map(|t| t.dash_section == *sec).unwrap_or(false))
                    .count();
                box_top(*sec, count, width, section_color(*sec))
            }
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => {
                    let sec = t.dash_section;
                    box_task(app, t, width, section_color(sec))
                }
                None => ListItem::new(Line::from("")),
            },
            DashRow::SectionFill(sec) => box_fill(width, section_color(*sec)),
            DashRow::SectionBottom(sec) => box_bottom(width, section_color(*sec)),
        })
        .collect();
```

Leave the `app.dash_ids().is_empty()` early-return branch as it is — with no tasks at all, the help text is still the right thing to show.

Leave the `List` construction and `ListState` selection at the end of the function unchanged.

- [ ] **Step 3: Remove the placeholder allow**

Delete the `#[allow(dead_code)]` added to `section_color` in Task 3 — it now has a real caller.

- [ ] **Step 4: Verify it builds and passes**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test`
Expected: **58 passed; 0 failed**.

Run: `cargo build --release 2>&1 | grep -E "^(warning|error)" | head -20`
Expected: no output.

- [ ] **Step 5: Look at it**

```bash
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 120 -y 38 \
  "XDG_DATA_HOME=$HOME/.local/share/taskvim-demo ./target/release/taskvim"
sleep 2
tmux capture-pane -t tv -p | head -22
tmux kill-session -t tv
```

**Paste the capture.** Check specifically:
- All three boxes render, colour-coded, with correct counts in the title bar.
- **The right-hand `│` lines up on every row**, including rows with `⚠`, `↻` and course tags. This is the most likely bug; if a bar is off by one, the width maths is wrong — fix it rather than reporting success.
- Each task is one row, badge right-aligned.
- Long titles truncate with `…` and do not push the bar out.

- [ ] **Step 6: Check the selection highlight**

```bash
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 120 -y 38 \
  "XDG_DATA_HOME=$HOME/.local/share/taskvim-demo ./target/release/taskvim"
sleep 2; tmux send-keys -t tv j; sleep 1
tmux capture-pane -t tv -e | head -12
tmux kill-session -t tv
```

Confirm the highlight sits on a task row and that repeated `j` never highlights a border row. **Paste the capture.**

- [ ] **Step 7: Commit**

```bash
git add src/ui
git commit -m "feat: render dashboard sections as colour-coded boxes"
```

---

### Task 6: Narrow-terminal check and polish

**Files:**
- Modify: `src/ui/dashboard.rs` (only if the checks below find problems)

- [ ] **Step 1: Render at 80×24**

```bash
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 80 -y 24 \
  "XDG_DATA_HOME=$HOME/.local/share/taskvim-demo ./target/release/taskvim"
sleep 2
tmux capture-pane -t tv -p
tmux kill-session -t tv
```

At 80 columns the task pane is 40 wide. **Paste the capture** and confirm: bars still align, titles truncate rather than wrap, counts remain visible, nothing overflows into the right column.

- [ ] **Step 2: Render with an empty dashboard**

```bash
mkdir -p /tmp/taskvim-scratch/taskvim
echo '{"version":3,"tasks":[],"projects":[],"sections":[],"log":[],"dashboard":[],"daily_goal":2,"next_id":10,"next_color":0}' > /tmp/taskvim-scratch/taskvim/data.json
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 120 -y 38 "XDG_DATA_HOME=/tmp/taskvim-scratch ./target/release/taskvim"
sleep 2; tmux capture-pane -t tv -p | head -14; tmux kill-session -t tv
```

Expected: the "dashboard is clear" help text, not three empty boxes. **Paste the capture.**

- [ ] **Step 3: Render with one task per section, and with a long title**

```bash
cat > /tmp/taskvim-scratch/taskvim/data.json <<'JSON'
{"version":3,"tasks":[
 {"id":1,"text":"a task with a deliberately very long title that must truncate cleanly","project":50,"section":null,"due":"2026-08-20","completed":false,"dash_section":"important","repeat":null,"period":null},
 {"id":2,"text":"short","project":null,"section":null,"due":null,"completed":false,"dash_section":"general","repeat":{"every":3,"unit":"day","anchor":"2026-08-01"},"period":"2026-08-27"}],
 "projects":[{"id":50,"name":"Algorithms 2","color":0}],
 "sections":[],"log":[],"dashboard":[1,2],"daily_goal":2,"next_id":10,"next_color":1}
JSON
tmux kill-session -t tv 2>/dev/null
tmux new-session -d -s tv -x 120 -y 38 "XDG_DATA_HOME=/tmp/taskvim-scratch ./target/release/taskvim"
sleep 2; tmux capture-pane -t tv -p | head -14; tmux kill-session -t tv
```

Expected: the long title truncates with `…` and the overdue badge still sits flush against the right bar; the Agent box renders empty with a blank interior row. **Paste the capture.**

- [ ] **Step 4: Fix anything the captures revealed**

Only if needed. State plainly what was wrong and what you changed. If everything rendered correctly, say so and skip to Step 5.

- [ ] **Step 5: Final verification**

Run: `export XDG_DATA_HOME=/tmp/taskvim-scratch && cargo test && cargo build --release 2>&1 | grep -E "^(warning|error)"`
Expected: 58 passed, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/ui
git commit -m "fix: section box rendering at narrow widths"
```

(Skip this commit if Step 4 found nothing to fix.)

---

## Notes for the implementer

**Width arithmetic is the whole job.** Use `Span::width()` / `Line::width()` — ratatui's Unicode display width — never `.len()` and never `.chars().count()` on strings containing `⚠`, `↻`, `🔥` or box-drawing glyphs. A single wrong width shifts the closing bar and the whole panel looks broken.

**`ellipsize` uses `chars().count()`**, which is fine for the ASCII-ish course names it is applied to but is NOT a display-width function. Do not repurpose it for the box padding maths.

**The block is borderless on purpose.** The spec explicitly rejects nesting section boxes inside an outer panel border. If the title/footer do not render correctly on a `Borders::NONE` block, report it rather than adding the border back.

**Stages 2 and 3** (dashboard layout rebalance; the other views) are separate plans. Do not start them.
