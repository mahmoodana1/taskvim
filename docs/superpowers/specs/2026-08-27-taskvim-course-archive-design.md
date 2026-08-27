# taskvim — archiving courses, and a legible overflow indicator

Date: 2026-08-27
Status: approved design, pending implementation plan

## Goal

Two related asks:

1. Archive a course you're done with, and un-archive it just as easily.
2. Make it obvious when the `COURSES` panel is holding more than it can show.

Archiving is the frequent direction and gets a single keystroke. Restoring is
rare and gets a modal.

## Decisions

Settled with the user before writing this:

| Question | Decision |
|---|---|
| Archived course's tasks | Unlinked from the dashboard on archive, **re-linked on un-archive** |
| Where archived courses live | A **modal**, not a full-screen view |
| Overflow signal | Count in the panel title plus a `▾` when truncated |

## Data model (version 4)

Both fields are `#[serde(default)]`, so a v3 file loads unchanged and the
migration is only the version bump — the same shape as v2 → v3.

```rust
struct Project {
    // … existing …
    /// Archived courses are hidden from the COURSES panel and from Tab
    /// cycling. Their tasks still exist and still count toward the course's
    /// own progress.
    #[serde(default)]
    pub archived: bool,
}

struct Task {
    // … existing …
    /// This task was on the dashboard when its course was archived, so
    /// un-archiving should put it back.
    #[serde(default)]
    pub relink: bool,
}
```

**Why the flag lives on the task.** The obvious alternative — a `Vec<u64>` of
remembered task ids on the `Project` — goes stale when a task is deleted, and
would need pruning in `sanitize()` like `dashboard` does. A `bool` on the task
cannot dangle: it dies with the task it belongs to.

## Archiving

`x` on a course card in the Courses pane:

1. Set `archived = true`.
2. For every task of that course currently on `data.dashboard`: set
   `relink = true` and remove it from the dashboard.
3. Status: `archived — X: open the archive`.

The card disappears from `COURSES`, from `Tab` cycling inside the course modal,
and from the panel's count.

## Un-archiving

`x` on a row in the archive modal:

1. Set `archived = false`.
2. For every task of that course with `relink == true`: clear the flag, and
   push it back onto the dashboard **only if** it is not completed and not
   already on the dashboard.
3. Status: `restored — N tasks back on the dashboard`.

The two guards matter. A task completed while the course sat archived should
not reappear as pending work, and a task the user re-linked by hand with `p`
must not be added twice.

## The archive modal

Opened with `X` from the dashboard, or `:archived`. Styled exactly like the
course modal — a centred popup with `Clear`, a border, a title and a footer
hint — so it reads as the same kind of object.

```
┌ ARCHIVED — 2 courses ───────────────────────────────┐
│  ○ Statistics            8/8 done   ────────────    │
│  ○ Calculus I           12/12 done  ────────────    │
└ x:restore  ⏎:open  q:close ─────────────────────────┘
```

- `j` / `k` move, `gg` / `G` jump.
- `x` restores the selected course.
- `⏎` opens that course's normal modal, so its tasks stay readable without
  restoring first. Archiving therefore never makes anything unreachable.
- `q` / `Esc` closes.
- Empty state: `nothing archived — press x on a course to archive it`.

Sized to content, capped by the available height, like the course modal.

## The index bug this exposes

`App.course_sel` currently indexes `data.projects` directly. Once archived
courses are filtered out of the panel, that index refers to a different list
than the one on screen, so `⏎`, `A`, `e` and `dd` would act on the wrong
course.

A `visible_courses() -> Vec<&Project>` helper (archived excluded, order
preserved) becomes the single source of truth, and **every** site that resolves
`course_sel` must go through it. This is the same class of bug as the dashboard
row-index change, and the same fix: one accessor, all call sites converted,
proven by grep.

`clamp()` must also bound `course_sel` against the visible count, not
`data.projects.len()`.

## Overflow indicator

`draw_courses` already computes a `hidden` count and renders
`COURSES (+N hidden)`. It fires only above ~7 courses, so in practice nobody
has seen it. Replace it with:

```
┌ COURSES ──────────────── 3 of 7 ▾ ┐     truncated
┌ COURSES ────────────────────── 3 ┐     everything fits
```

`j` / `k` already scroll via `ListState`; the `▾` just makes it legible that
there is more below. The archived count is **not** included — this is a count
of what the panel could show, not of everything that exists.

The footer hint gains `x:archive`.

## Not changing

The heatmap, the log, per-course progress, and the completion/streak maths. An
archived course's tasks still exist and still count toward that course's own
`project_stats`.

## Testing

- Archiving flags and unlinks exactly the course's dashboard tasks, and leaves
  other courses' tasks alone.
- Un-archiving re-links exactly the flagged set.
- Un-archiving skips a task completed while archived.
- Un-archiving does not double-add a task the user re-linked by hand.
- `visible_courses()` excludes archived and preserves order.
- `course_sel` resolves against the visible list after an archive.
- A v3 file migrates to v4 with nothing dropped.

Rendering is verified by tmux capture: the modal at a couple of terminal sizes,
and the `N of M ▾` title with enough courses to truncate.

## Risks

- **Silent wrong-course actions** if a `course_sel` call site is missed. Grep
  is the mitigation, and one test pins the mapping.
- **`x` is destructive-adjacent** — it removes tasks from the dashboard. It is
  undoable via the existing snapshot `undo`, which covers it, but the status
  message should say what happened rather than just "archived".
