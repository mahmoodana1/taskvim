# taskvim visual overhaul — boxed sections, task-forward layout

Date: 2026-08-27
Status: approved design, pending implementation plan
Follows: v0.3 (`docs/superpowers/plans/2026-08-26-taskvim-v03.md`)

## Goal

Make the three dashboard sections unmistakable, and make the whole app read as
one deliberate system rather than a set of panels that happen to share a screen.

The user's words: *"make sure the sections are clear… make it look alive and
systematic."*

**Alive** — colour carries meaning, density replaces dead space, the screen
looks inhabited rather than mostly empty.
**Systematic** — one visual grammar everywhere: the same box idiom, the same
colour rules, the same alignment, on every view.

## What's wrong today

Rendered at 120×38 with seven tasks and one course:

- Section headers (`▸ Important  2`) carry the same visual weight as the tasks
  under them (`▎buy groceries`). Grouping is implied by indentation alone.
- Every task with a due date or cadence takes **two rows**, because the badge is
  pushed onto its own line. Seven tasks consume eleven rows.
- The `Courses & Projects` panel renders roughly **fifteen empty rows** below a
  single course.
- The task column is 38 columns; the right column is 80. The working set — the
  actual point of the app — gets the narrower third of the screen.
- `CURRENT PRIORITY` centres its task over the filler line *"Stay strictly on
  this task until completion."*, spending three rows to say very little.

## Decisions

Settled with the user before writing this:

| Question | Decision |
|---|---|
| Section treatment | Boxed sub-panels, matching the app's existing panel idiom |
| Empty sections | Always render a full box, never collapse |
| Colour | Section identity — Important red, General blue, Agent violet |
| Layout | Task-forward: task column ~55%, right column a compact stats rail |
| Reach | Every view, not just the dashboard |
| Delivery | Three reviewable stages |

## The visual system

### Section boxes

```
┌ IMPORTANT ───────────────── 2 ┐     red
│ buy groceries                 │
│ water plants      ↻ every 3d  │
└───────────────────────────────┘
┌ GENERAL ─────────────────── 0 ┐     blue
│                               │
└───────────────────────────────┘
┌ AGENT ───────────────────── 1 ┐     violet
│ review flashcards     ↻ daily │
└───────────────────────────────┘
```

No outer `TASKS` panel. The section boxes *are* the panel — nesting them inside
another border wastes four columns and reads as heavy. The pane's focused state
is shown by brightening the section borders rather than by an enclosing frame.

### Colour rules

Two colour systems coexist without competing, because they never touch the same
glyph:

- **Section identity → chrome only.** Borders and titles. Important red,
  General blue, Agent violet.
- **Status → text only.** Overdue red, due-today amber, habit cadence dim,
  course tags in their own palette colour.

This resolves the one real collision: an overdue task inside the red Important
box keeps its red `⚠` legible against a red border, because the border is the
only red chrome and the text is the only red text.

### Task rows

Badges move inline. A task is one row:

```
│ fix the undo bug        ⚠ was due Aug 24 │
│ call the dentist              due Aug 30 │
│ water plants                  ↻ every 3d │
│ [Algorithms 2] Lecture 9                 │
```

The badge right-aligns; the title truncates with `…` when the two would collide.
This reclaims roughly a third of the list's height.

## Row model change

The renderer needs rows the cursor must never land on — box bottoms, and the
blank interior of an empty section. `App.task_sel` indexes `dash_rows()`, and
ratatui's `ListState` indexes rendered items, so **the two must stay 1:1**.

`DashRow` therefore grows from two variants to four:

```rust
pub enum DashRow {
    SectionTop(DashSection),     // ┌ IMPORTANT ──── 2 ┐   selectable
    Task(u64),                   // │ …                │   selectable
    SectionFill(DashSection),    // │                  │   skipped
    SectionBottom(DashSection),  // └──────────────────┘   skipped
}
```

`dash_rows()` emits, per section: `SectionTop`, then either its `Task` rows or a
single `SectionFill` when empty, then `SectionBottom`.

Navigation (`j`/`k`/`G`) skips `SectionFill` and `SectionBottom`; `gg` still goes
to row 0, which is `SectionTop(Important)`. `selected_task()` and
`selected_section()` keep their current contracts — `selected_section()` walks
backward to the nearest `SectionTop`.

This is the only change outside `src/ui/`, and it is confined to the row model
plus the two movement handlers.

### Why boxes are drawn as list rows

The alternative — three real bordered widgets — would mean three `List`s and
three `ListState`s, losing unified `j`/`k` scrolling across section boundaries.
Drawing the borders as rows inside the single existing `List` keeps scrolling,
selection and the existing widget entirely intact. The only cost is that the
right-hand `│` must be padded to the panel's inner width on every row.

## Layout

```
┌ IMPORTANT ───────────── 2 ┐  ┌ NOW ──────────────────┐
│ buy groceries             │  │ buy groceries         │
│ water plants   ↻ every 3d │  └───────────────────────┘
└───────────────────────────┘  ┌ GOAL ─────────────────┐
┌ GENERAL ─────────────── 3 ┐  │ 0/2   🔥 3 day streak │
│ fix undo bug   ⚠ Aug 24   │  └───────────────────────┘
│ call dentist     Aug 30   │  ┌ 2026 ─────────────────┐
│ [Algorithms 2] Lecture 9  │  │ ▪▪▫▪▫▫▪▪▫▪▪▫▪▪▫▪▪▫▪▪  │
└───────────────────────────┘  └───────────────────────┘
┌ AGENT ───────────────── 1 ┐  ┌ COURSES ──────────────┐
│ review flashcards ↻ daily │  │ ● Algorithms 2  0/1   │
└───────────────────────────┘  │   ━━━━━━━━━━━━━━━━━   │
                               └───────────────────────┘
```

- Task column ~55%, stats rail ~45%.
- `NOW` drops the *"Stay strictly on this task"* filler and left-aligns.
- `GOAL` merges the daily-goal gauge and the streak onto one row.
- `COURSES` sizes to its content. With no courses it shows its one-line prompt
  rather than an empty frame.
- Panels stack top-down and the last one absorbs remaining height, so no panel
  ever renders a field of blank rows.

## Other views

Same grammar applied:

- **Course modal** — section sub-boxes matching the dashboard, course colour on
  the outer border, inline badges.
- **Log / day view** — commits grouped in dated boxes; hash, time and course tag
  on consistent columns.
- **Help** — sections in boxes, keys in a fixed-width column. Already compressed
  during v0.3 to fit 44 rows; the box chrome must not push it back over.
- **Statusbar / popups** — pickers adopt the same border and title treatment.

## Not changing

Every keybinding. The data format. All 52 existing tests. `model.rs`, `due.rs`,
`parse.rs` and `app/actions.rs` are untouched; `app/mod.rs` changes only for the
row model, and `app/keys.rs` only for movement skipping.

## Delivery

Three stages, each reviewable and each leaving the app working:

1. **Section boxes + inline badges** — the row model, `ui/dashboard.rs`,
   `ui/theme.rs`. Delivers the thing actually asked for.
2. **Dashboard layout** — `ui/mod.rs`, `ui/heatmap.rs`, `ui/courses.rs`.
3. **Other views** — course modal, log, day view, help, popups.

## Testing

The row model is pure and gets unit tests: box rows emitted in the right order,
empty sections producing a `SectionFill`, navigation skipping non-selectable
rows, `selected_task()`/`selected_section()` contracts preserved.

Rendering is verified by tmux capture at 120×38 and at 80×24, checked for
alignment, truncation and clipping — the same method used throughout v0.3.

## Risks

- **Vertical space.** Boxes cost 2 rows per section, 6 total. Inline badges save
  more than that at realistic task counts, but a dashboard with 3 tasks and no
  badges is net worse off by a few rows. Accepted: the user chose full boxes for
  empty sections knowing the cost.
- **Narrow terminals.** At 80 columns the task column is ~44 wide; long course
  tags plus a badge will truncate aggressively. Truncation must be graceful, and
  80×24 is an explicit test case.
- **Right-border alignment** is the most likely source of visual bugs, since
  every row must pad to the same inner width including rows with wide glyphs
  (`⚠`, `↻`, `🔥`). Unicode width, not byte length, must be used.
