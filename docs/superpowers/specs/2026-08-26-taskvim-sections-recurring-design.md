# taskvim v0.3 — dashboard sections, recurring tasks, module split

Date: 2026-08-26
Status: approved design, pending implementation plan

## Goal

Four pieces of work on taskvim, in this order:

1. Split `src/main.rs` (2845 lines) into modules — pure code motion.
2. Fix two log-integrity bugs and add the first test suite.
3. Fixed sections (Important / General / Agent) on the main dashboard.
4. Recurring tasks.

UI polish is scoped to what 3 and 4 need — no general visual pass this round.

## Bugs being fixed

Both stem from the same root cause: `LogEntry` has no link back to the task that
produced it, so the code matches log entries to tasks by text.

**A — log data loss (`src/main.rs:728`).** Un-completing a task runs:

```rust
self.data.log.retain(|e| !(e.text == text && e.project == project && e.section == section));
```

This deletes *every* log entry with that text across all history. A task
completed 40 times over months loses all 40 commits — heatmap, streak, daily
goal, and course percentages all silently lose history — when it is
un-completed once. Recurring tasks complete the same text repeatedly by design,
which turns this from rare into routine.

**B — daily-goal misattribution (`src/main.rs:492`).** `dash_completed_today()`
joins log entries to dashboard tasks by text. Completing a task that is *not* on
the dashboard therefore still counts toward today's total whenever its text
happens to match a task that is — and unlinking a task retroactively changes
today's number.

(An earlier draft of this spec described B as double-counting two same-named
dashboard tasks. That was wrong: the join is a per-entry boolean, so a single
completion can only ever contribute 0 or 1. The defect is misattribution, not
multiplication. Caught while writing the regression test, which passed against
the unfixed code until it was rewritten around the real scenario.)

**Fix:** add `LogEntry.task: Option<u64>` and match on it. Un-complete removes
**at most one** entry — the newest whose `task` matches. Entries migrated from
older files have `task: None` and fall back to text matching, still capped at
one removal.

## Data model (version 3)

```rust
enum DashSection { Important, General, Agent }   // serialized lowercase

enum RepeatUnit { Day, Week, Month }

struct Repeat {
    every: u32,
    unit: RepeatUnit,
    anchor: String,   // YYYY-MM-DD, the date the repeat was set — defines the grid
}

struct Task {
    // ... existing fields ...
    dash_section: DashSection,   // #[serde(default)] -> General
    repeat: Option<Repeat>,      // #[serde(default)]
    period: Option<String>,      // YYYY-MM-DD start of the period `completed` refers to
}

struct LogEntry {
    // ... existing fields ...
    task: Option<u64>,           // #[serde(default)]
}
```

`DashSection` is a fixed enum, not a `Section` record. The existing `Section`
struct is course-bound (`project: u64`) and the three dashboard sections are
fixed and non-renameable, so an enum cannot dangle, duplicate, or need
sanitizing — unlike an id-based section it would otherwise mirror.

### Migration v2 → v3

Runs inside `Data::sanitize()`, alongside the existing v1 → v2 step:

- Every task gets `dash_section: General` (serde default covers it).
- Every task gets `repeat: None` and `period: None`.
- Every existing log entry gets `task: None`.
- `version = 3`.

No data is dropped. A v2 file opened by v3 works; a v3 file opened by v2 loses
the new fields (acceptable — single-user local tool).

## Dashboard sections

### Rendering

The dashboard changes from a flat list indexed by `task_sel` into interleaved
rows, mirroring the `Row` enum the course modal already uses:

```rust
enum DashRow { Section(DashSection), Task(u64) }
```

All three section headers always render, including empty ones, so `1`/`2`/`3`
always have a visible target. Each header shows its task count. Section order is
fixed: Important, General, Agent.

`task_sel` becomes an index into `dash_rows()` rather than into
`data.dashboard`. Cursor movement skips section headers for actions that need a
task (complete, delete, due) but stops on them for `a` (add into that section).

`data.dashboard: Vec<u64>` stays the source of truth for task *order*; the
section is a property of the task. `dash_rows()` walks `dashboard` in order and
buckets each id under its task's `dash_section`, so relative order within a
section is preserved and `J`/`K` swap adjacent ids that share a section.

### Keys (dashboard only)

| key | action |
|---|---|
| `1` `2` `3` | send task under cursor to Important / General / Agent |
| `a` | add a task into the section under the cursor |
| `J` `K` | reorder within the current section |

`1`, `2`, `3` are currently unbound on the dashboard — verified against
`handle_dashboard`, which binds only `q ? : o j k g G J K a i A Enter Space c u e t d D r + = - [ ]`.

### Add-line syntax

The existing parser strips a trailing `!due` and `@course`. It gains `#section`,
prefix-matched so `#i` / `#g` / `#a` work:

```
fix undo bug @taskvim !+3 #important
buy milk #general
```

Tokens may appear in any order. Absent `#section`, a dashboard add lands in the
section under the cursor; a course task linked with `p` lands in General.

### Behavior changes to existing keys

- `c` (complete current priority) and `r` (pull random → current priority) mean
  "top of the flat dashboard list" today. They now mean **top of Important**.
- `make_priority` and `pick_random` insert at `dashboard[0]`; they must also set
  `dash_section = Important`.
- `dd` / `D` / `t` / `e` are unchanged; they no-op on a section header row.

## Recurring tasks

### Semantics

Recurring tasks are habits, not deadlines. **They never carry a due date and
never go overdue.** One task record exists per recurring task, forever; it is
reset in place when the period rolls over, so two occurrences of the same task
can never be live at once.

The rules:

- Completing a recurring task logs a commit exactly like any other task and sets
  `completed = true`. It therefore **disappears from the dashboard** for the rest
  of the period — `dash_ids()` already filters completed tasks, so this needs no
  new code.
- On each **tick** (the start of a new period), the task is reset:
  `completed = false`, `period = <new period start>`. It reappears on the
  dashboard.
- A period that passes without a completion leaves **no trace** — no overdue
  badge, no backlog, no carry-over. Missing Tuesday's habit does not make
  Wednesday's harder.

### Period grid (fixed calendar cadence)

Ticks land on a fixed grid regardless of when — or whether — you complete the
task. The grid is defined by `unit` and strided by `every`, starting from
`anchor`:

| unit | grid | `every: 1` | `every: N` |
|---|---|---|---|
| `Day` | every midnight | daily | every N days from `anchor` |
| `Week` | Mondays | weekly, resets Monday | every N weeks from `anchor`'s Monday |
| `Month` | the 1st | monthly, resets on the 1st | every N months from `anchor`'s month |

```
fn period_start(today, repeat) -> NaiveDate {
    let base  = align(repeat.anchor, repeat.unit);   // Day: as-is · Week: that Monday · Month: the 1st
    let steps = units_between(base, align(today, repeat.unit)) / repeat.every;
    base + steps * repeat.every * repeat.unit
}
```

`units_between` counts days for `Day`, whole weeks between Mondays for `Week`,
and calendar months for `Month` — so month arithmetic never needs end-of-month
clamping, because the grid only ever lands on the 1st.

A task is due for reset whenever `task.period != Some(period_start(today))`.

### When the tick runs

- At startup, in `App::new()` after `Data::load()`.
- In the run loop, which already calls `event::poll(400ms)` — so each iteration
  can cheaply compare today's date against the last tick date and re-run the
  sweep when it changes. No restructuring of `run()` is needed.

The tick is **not a user edit**: it saves via `Data::save()` directly and
refreshes `saved_snapshot` so it never lands on the undo stack. Pressing `u`
after midnight should undo your last real action, not the rollover.

### Interactions

- `t` and the `!due` suffix are rejected on a recurring task with a status
  message; setting a repeat on a task that has a due date clears the due date.
- The `⚠ overdue` badge and every overdue count never apply to recurring tasks.
- Recurring tasks are **excluded from course progress** (`project_stats`). They
  never finish, so counting them would make a course's percentage oscillate as
  habits reset. They still feed the heatmap, streak, and daily goal on every
  completion — that is the point of them.
- Undo restores the whole `Data` snapshot, so undoing a recurring completion
  restores `completed`, `period`, and the removed log entry with no special case.

### Keys and syntax

- `~` opens a repeat menu, following the existing `DueMenu` pattern: preset
  intervals (daily, every 3 days, weekly, biweekly, monthly), plus a custom
  entry and "none" to clear.
- Add-line suffix `~3d` / `~2w` / `~1m`, parsed alongside `!due`, `@course`, and
  `#section`.

### Display

A recurring task shows a dimmed `↻ daily` / `↻ every 3d` / `↻ weekly` badge
after its text, in the slot where a due span would otherwise sit — the two are
mutually exclusive.

## Module split

Step 1, done before any feature work, so the features land in small files. Pure
code motion — no behavior change, no signature changes beyond what visibility
requires.

```
src/main.rs        entry point, terminal setup, run loop
src/model.rs       Task, Section, DashSection, Repeat, Project, LogEntry, Data,
                   load/save/sanitize/migrate
src/due.rs         parse_due, due_span, recurrence math
src/app/mod.rs     App struct, state, undo/redo, checkpoint
src/app/actions.rs add/complete/delete/move/pin/priority
src/app/keys.rs    handle_key and the per-view handlers
src/ui/mod.rs      ui(), top-level layout
src/ui/dashboard.rs
src/ui/courses.rs  course cards + course modal
src/ui/log.rs      log and daylog views
src/ui/heatmap.rs  heatmap, daily goal, priority widget
src/ui/help.rs
src/ui/popup.rs    input popup, due menu, repeat menu, statusbar
```

Verification for this step: `cargo build --release` clean with no new warnings,
and a manual smoke run confirming the dashboard, course modal, log, heatmap, and
help all render and respond as before.

## Testing

The project has no tests today. Add unit tests covering the pure logic — the
parts that do not need a terminal:

- `parse_due` — all documented formats (`2026-07-10`, `7-10`, `+3`, `today`,
  `tomorrow`, `none`) plus malformed input.
- `period_start` — the day/week/month grids, `every: N` striding from the
  anchor, week alignment to Monday, month alignment to the 1st, and a period
  boundary crossed with no completion in between (task resets, nothing logged).
- Add-line parser — `@course`, `!due`, `#section` in any order, prefix matching,
  and text that legitimately contains `@`, `!`, or `#`.
- Migration v2 → v3 — fields defaulted, nothing dropped, `version` bumped.
- Un-complete removes exactly one log entry, both with `task: Some(id)` and via
  the `task: None` text fallback.

TUI rendering is not unit-tested; it is verified by the manual smoke run.

## UI polish (scoped)

Only what the new features need:

- Section headers on the dashboard with per-section task counts.
- `↻ every 3d` badge on recurring tasks.
- Help screen updated with `1`/`2`/`3`, `~`, and the `#section` syntax.
- Statusbar hints for the repeat menu.

## Out of scope

Search/filter, notes on tasks, subtasks, archive, user-created dashboard
sections, a general visual pass, and any change to the course modal beyond what
the module split requires.

## Open assumptions

Stated explicitly so they can be overridden during implementation:

1. A recurring task's grid is anchored to the date its repeat was set, and
   `every: 1` snaps to the natural boundary (midnight / Monday / the 1st).
2. Recurring tasks are excluded from course progress percentages.
3. The tick bypasses the undo stack.
4. Linking a course task to the dashboard with `p` puts it in General.
5. Section headers always render, even when empty.
