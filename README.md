# taskvim v0.2

A vim-motions-driven terminal task / project / course manager. Courses hold your
full backlog organized under **sections** (subtitles like *Lectures*, *Labs*,
*Assignments*); the **main dashboard shows only the tasks you link there** (or add
manually) — your actual working set. Completing a task anywhere becomes a
"commit" that feeds a GitHub-style contribution heatmap, a streak, the daily
goal, and each course's progress bar. There's a `git log`-style history too.

Works on any Linux distro (and macOS/Windows terminals — pure crossterm).
Single binary, zero runtime dependencies, one plain-JSON data file.

## The model

```
Course "Algorithms 2"                     Main dashboard (Tasks)
├─ ▸ Lectures                             ▎[Algorithms 2] Lecture 7
│    ↗ Lecture 7        ── p (link) ──►   ▎[Algorithms 2] Lab 3   ⚠ was due Jul 01
│    ○ Lecture 8                          ▎buy groceries          (added manually)
├─ ▸ Labs
│    ↗ Lab 3  due Jul 05                  ↗ = linked to dashboard
└─ ▸ Assignments                          ○ = backlog only
     ○ Problem set 4
```

- **Open a course** with `⏎` on its card → a modal shows its sections and tasks.
- **`p`** links/unlinks a task to the main dashboard. `dd` on the dashboard only
  *unlinks* a course task (it stays in its course); `D` deletes it for real.
- **Due dates** are optional (default none): press `t`, or append `!spec` while
  adding — `lab report @algo !+3`.
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
compiles on distros with older Rust. A prebuilt `taskvim-x86_64-linux` binary is
included (needs glibc ≥ 2.39: Ubuntu 24.04+, Fedora 40+, Arch); otherwise build
from source (~30 s).

Data: `~/.local/share/taskvim/data.json` (respects `$XDG_DATA_HOME`). v0.1 files
are migrated automatically. Back it up, sync it, edit it by hand — it's yours.

## Keybindings

### Everywhere
| key | action |
|---|---|
| `j` `k` `gg` `G` | move / jump top / bottom |
| `h` `l` `Tab` | switch pane; `Tab` inside a course cycles to the next course |
| `J` / `K` | reorder task (or section) up / down |
| `u` | undo the last completion |
| `?` `:` `q` | help · ex commands · quit / close |

### Dashboard
| key | action |
|---|---|
| `a` | add a task to the dashboard — supports `@course` and `!due` |
| `Enter` / `Space` | complete selected task — **on a course card: open it** |
| `c` | complete the CURRENT PRIORITY (top) task |
| `dd` | unlink a course task from the dashboard · deletes loose tasks |
| `D` | delete the task for real |
| `r` | pull a random backlog task → current priority |
| `t` / `e` | set due date · rename |
| `A` | quick-add a task into the selected course (backlog) |
| `+` `-` / `[` `]` | daily goal · heatmap year |
| `o` | open the commit-style completed history |

### Inside a course (the modal)
| key | action |
|---|---|
| `S` | add a **section** (subtitle) to group/order tasks |
| `a` | add a task under the section at the cursor |
| `p` | **link / unlink** the task ↗ to the main dashboard |
| `m` / `M` | move task to the next / previous section |
| `Enter` / `Space` | complete the task |
| `t` `e` `R` | due date · rename task/section · rename course |
| `dd` | delete task, or delete section (its tasks are kept, unsectioned) |
| `J` / `K` | reorder tasks within a section, or reorder whole sections |
| `q` / `Esc` | close the course |

### Due date formats
`2026-07-10` · `7-10` (this year) · `+3` (in 3 days) · `today` · `tomorrow` · `none` (clear).
Overdue tasks show `⚠ was due …` and each course card shows a `⚠N` overdue badge.

### Ex commands
`:w` `:q` `:wq` · `:log` · `:db` · `:goal N` · `:year YYYY` · `:clearlog` · `:help`

## The "git log" view

Every completed task becomes a commit — hash, timestamp, course tag, message,
newest first with a `(HEAD -> today)` marker. The same entries drive the
contribution heatmap, streak, daily goal, and course percentages.
