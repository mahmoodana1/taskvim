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

**B — daily-goal miscount (`src/main.rs:492`).** `dash_completed_today()` joins
log entries to dashboard tasks by text, so two tasks with the same text
double-count, and unlinking a task retroactively changes today's number.

**Fix:** add `LogEntry.task: Option<u64>` and match on it. Un-complete removes
**at most one** entry — the newest whose `task` matches. Entries migrated from
older files have `task: None` and fall back to text matching, still capped at
one removal.

## Data model (version 3)

```rust
enum DashSection { Important, General, Agent }   // serialized lowercase

struct Repeat { every: u32, unit: RepeatUnit }   // RepeatUnit = Day | Week | Month

struct Task {
    // ... existing fields ...
    dash_section: DashSection,   // #[serde(default)] -> General
    repeat: Option<Repeat>,      // #[serde(default)]
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
- Every task gets `repeat: None`.
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

Completing a recurring task logs a commit as normal, then re-arms the **same
task record** (`completed` stays false) with its due date advanced:

```
next_due = old_due + interval
```

Anchored to the old due date, not to today. A task completed late is therefore
immediately overdue again — deliberate "refuse to let it slip" behavior.

Two consequences, both intended:

- **One step per completion, no catch-up.** A daily task completed 30 days late
  becomes due 29 days ago, not today. Each completion advances one interval.
- **No due date, no anchor.** If a recurring task has `due: None`, the interval
  is counted from today instead.

Month intervals clamp to the end of the target month (Jan 31 + 1 month = Feb 28
or 29), matching how humans read "monthly".

Undo restores the whole `Data` snapshot, so undoing a recurring completion
restores both the old due date and the removed log entry with no special case.

### Keys and syntax

- `~` opens a repeat menu, following the existing `DueMenu` pattern: preset
  intervals (daily, every 3 days, weekly, biweekly, monthly), plus a custom
  entry and "none" to clear.
- Add-line suffix `~3d` / `~2w` / `~1m`, parsed alongside `!due`, `@course`, and
  `#section`.

### Display

A recurring task shows a `↻ every 3d` badge after its text, dimmed, before the
due span.

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
- Recurrence math — day/week/month advance, month-end clamping, the no-due-date
  fallback to today, and the late-completion case staying overdue.
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

1. Completing a recurring task advances one interval and does not skip forward
   past today.
2. A recurring task with no due date anchors its first interval to today.
3. Linking a course task to the dashboard with `p` puts it in General.
4. Section headers always render, even when empty.
