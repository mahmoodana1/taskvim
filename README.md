# taskvim v0.3

A vim-motions-driven terminal task / project / course manager. Courses hold your
full backlog organized under **sections** (subtitles like *Lectures*, *Labs*,
*Assignments*); the **main dashboard shows only the tasks you link there** (or add
manually) — your actual working set, grouped under **Important**, **General** and
**Agent**. Completing a task anywhere becomes a "commit" that feeds a
GitHub-style contribution heatmap, a streak, the daily goal, and each course's
progress bar. There's a `git log`-style history too.

Tasks can also be **habits** — give one a cadence with `~` and it comes back on
its own schedule instead of carrying a deadline.

The built-in **Calendar** combines the weekly course timetable with due tasks;
tasks added there are ordinary TaskVim tasks and appear on the dashboard too.
Use `Shift+C` to open it and `Shift+T` to return to Tasks.

Works on any Linux distro (and macOS/Windows terminals — pure crossterm).
Single binary, zero runtime dependencies, one plain-JSON data file.

## The model

```
Course "Algorithms 2"                     Main dashboard (Tasks)
├─ ▸ Lectures                             ▸ Important  2
│    ↗ Lecture 7        ── p (link) ──►     ▎[Algorithms 2] Lecture 7
│    ○ Lecture 8                            ▎fix the undo bug  ⚠ was due Jul 01
├─ ▸ Labs                                 ▸ General  2
│    ↗ Lab 3  due Jul 05                    ▎[Algorithms 2] Lab 3
└─ ▸ Assignments                            ▎buy groceries     (added manually)
     ○ Problem set 4                      ▸ Agent  1
                                            ▎water plants      ↻ every 3d
↗ = linked to dashboard
○ = backlog only                          1/2/3 moves a task between sections
```

- **Open a course** with `⏎` on its card → a modal shows its sections and tasks.
- **`p`** links/unlinks a task to the main dashboard (landing in *General*). `dd`
  on the dashboard only *unlinks* a course task (it stays in its course); `D`
  deletes it for real.
- **Dashboard sections** are fixed: Important, General, Agent. `1` `2` `3` move
  the selected task between them; `J` `K` reorder within one.
- **Add syntax:** `lab report @algo !+3 #important` — `@course`, `!due`,
  `#section` and `~repeat` may appear in any order. Sections prefix-match, so
  `#i` `#g` `#a` work. Anything that doesn't resolve stays as literal text.
- **Due dates** are optional (default none): press `t`, or append `!spec` while
  adding.
- **Habits** repeat instead: press `~`, or append `~3d`. They never carry a due
  date.
- **`u`** undoes the last completion from anywhere.

## Install

Requires `cargo` (Rust ≥ 1.75 — whatever your distro ships is fine):

```sh
# sudo apt install cargo   |   sudo dnf install cargo   |   sudo pacman -S rust
cd taskvim
cargo build --release
install -Dm755 target/release/taskvim ~/.local/bin/taskvim
taskvim
```

The included `Cargo.lock` pins dependencies known to build on rustc 1.75+, so it
compiles on distros with older Rust. Building from source takes ~30 s.

(A `taskvim-x86_64-linux` binary may sit in the working directory — it needs
glibc ≥ 2.39: Ubuntu 24.04+, Fedora 40+, Arch. It is **not** tracked in git, so
a fresh clone won't have one; build from source instead.)

Data: `~/.local/share/taskvim/data.json` (respects `$XDG_DATA_HOME`), format v5.
Older files are migrated automatically on load — existing tasks land in
*General* and nothing is dropped. Back it up, sync it, edit it by hand — it's
yours.

Note that older binaries will silently drop the newer fields if they write the
file, so don't run two versions against the same data.

## Keybindings

### Everywhere
| key | action |
|---|---|
| `Shift+C` / `Shift+T` | Calendar / Tasks |
| `j` `k` `gg` `G` | move / jump top / bottom |
| `h` `l` `Tab` | switch pane; `Tab` inside a course cycles to the next course |
| `J` / `K` | reorder task (or section) up / down |
| `u` | undo the last completion |
| `?` `:` `q` | help · ex commands · quit / close |

### Dashboard
| key | action |
|---|---|
| `a` | add a task into the section at the cursor — supports all four suffixes |
| `1` `2` `3` | move task to Important / General / Agent |
| `~` | set repeat — `3d` `2w` `1mo` `daily` `weekly` `monthly` `none` |
| `Enter` / `Space` | complete selected task — **on a course card: open it** |
| `c` | make the selected task the CURRENT PRIORITY (top of Important) |
| `dd` | unlink a course task from the dashboard · deletes loose tasks |
| `D` | delete the task for real |
| `r` | pull a random backlog task → current priority |
| `t` / `e` | set due date · rename |
| `A` | quick-add a task into the selected course (backlog) |
| `+` `-` / `[` `]` | daily goal · heatmap year |
| `o` | open the commit-style completed history |

### Calendar
| key | action |
|---|---|
| `d` / `w` / `m` | day / week / month view (week starts Sunday; all seven days shown) |
| `h` `l` / `j` `k` | move date / selection; in week and month, `j`/`k` move by a week |
| `[` / `]` | previous / next month |
| `t` | jump to today |
| `a` | add a task due on the selected day (`!date` can override it); add `09:00-10:00` for a time |
| `e` | add an event: `Team meeting 09:00-10:00` |
| `s` | set/clear a task time or edit the selected event’s time |
| `dd` | delete the selected task/event, or hide a Yedion item locally |
| `u` / `Ctrl+r` | undo / redo calendar changes |
| `Enter` | complete a selected task; from week/month opens that day |

Day view shows all 24 hours. Untimed tasks are all-day items; add a time range
while creating one (`a`) or schedule it later with `s`. Classes, events, and
timed tasks share the ordered timeline in separate Google Calendar-style colors.
Overlapping items each get their own colored branch row. Select an item and use
`dd` to delete it; Yedion entries are hidden locally and can be restored with
`u` (redo with `Ctrl+r`).

Course meetings, exams, and other academic entries come from your private Yedion
iCalendar feed. TaskVim refreshes it on startup and with `r` in the calendar;
the last successful copy is cached locally for offline use. The feed URL is
kept outside the repository in `~/.config/taskvim/yedion.ics-url`.

### Inside a course (the modal)
| key | action |
|---|---|
| `S` | add a **section** (subtitle) to group/order tasks |
| `a` | add a task under the section at the cursor |
| `p` | **link / unlink** the task ↗ to the main dashboard |
| `m` / `M` | move task to the next / previous section |
| `Enter` / `Space` | complete the task |
| `~` | set repeat — course tasks can be habits too |
| `t` `e` `R` | due date · rename task/section · rename course |
| `dd` | delete task, or delete section (its tasks are kept, unsectioned) |
| `J` / `K` | reorder tasks within a section, or reorder whole sections |
| `q` / `Esc` | close the course |

### Due date formats
`2026-07-10` · `7-10` (this year) · `+3` (in 3 days) · `today` · `tomorrow` · `none` (clear).
Overdue tasks show `⚠ was due …` and each course card shows a `⚠N` overdue badge.

### Archiving courses

`x` on a course card archives it: the card disappears and its tasks come off
your dashboard, but nothing is deleted. `X` (or `:archived`) opens the archive,
where `x` restores a course and puts exactly those tasks back — skipping any you
finished in the meantime. `⏎` there opens an archived course read-only, so
archiving never puts its tasks out of reach.

The `COURSES` panel shows `3 of 7 ▾` when it is holding more than it can
display; `j`/`k` scroll it.

### Recurring tasks

`~` turns a task into a habit. Habits never carry a due date and never go
overdue: completing one logs a commit — so it still feeds the heatmap, streak
and daily goal — then hides it until the period rolls over, at which point it
comes back.

Ticks land on a **fixed calendar grid**, whether or not you completed it: daily
at midnight, weekly on Monday, monthly on the 1st, and `every N` striding from
the day you set the repeat. **A period you skip leaves no trace** — no backlog,
no overdue badge, no guilt. Missing Tuesday's habit doesn't make Wednesday's
harder.

Formats: `3d` · `2w` · `1mo` · `daily` · `weekly` · `monthly` · `none` (clear).
A habit shows a dimmed `↻ every 3d` where a due date would sit, and is left out
of course progress percentages — it never finishes, so counting it would make
the bar oscillate.

### Ex commands
`:w` `:q` `:wq` · `:log` · `:db` · `:archived` · `:goal N` · `:year YYYY` · `:clearlog` · `:help`

## The "git log" view

Every completed task becomes a commit — hash, timestamp, course tag, message,
newest first with a `(HEAD -> today)` marker. The same entries drive the
contribution heatmap, streak, daily goal, and course percentages.
