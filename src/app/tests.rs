use crate::app::App;
use crate::model::{Data, Task};
use chrono::NaiveDate;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// An App built around in-memory data. Note that methods under test call
/// `save_quiet()`, which writes to `Data::path()` — tests must be run with
/// `XDG_DATA_HOME` pointed at a scratch dir. Assertions here are on in-memory
/// state only; nothing depends on what lands on disk.
pub fn test_app(tasks: Vec<Task>) -> App {
    let mut data = Data::default();
    data.next_id = 100;
    data.daily_goal = 2;
    data.dashboard = tasks.iter().map(|t| t.id).collect();
    data.tasks = tasks;
    data.version = 3;
    App::with_data(data)
}

pub fn task(id: u64, text: &str) -> Task {
    Task {
        id,
        text: text.to_string(),
        project: None,
        section: None,
        due: None,
        completed: false,
        dash_section: crate::model::DashSection::General,
        repeat: None,
        period: None,
        relink: false,
    }
}

#[test]
fn uncompleting_removes_only_the_newest_matching_log_entry() {
    let mut app = test_app(vec![task(1, "water plants")]);

    // Complete it three times, resetting to pending in between the way a
    // recurring habit would. Each completion appends one log entry.
    for _ in 0..3 {
        app.toggle_complete(1);
        app.data.tasks[0].completed = false;
    }
    assert_eq!(app.data.log.len(), 3, "three completions, three commits");

    // Now genuinely complete it, then un-complete it once.
    app.toggle_complete(1);
    assert_eq!(app.data.log.len(), 4);
    app.toggle_complete(1);

    assert_eq!(
        app.data.log.len(),
        3,
        "un-completing removes exactly one entry, not all four"
    );
}

#[test]
fn uncompleting_falls_back_to_text_match_for_migrated_entries() {
    let mut app = test_app(vec![task(1, "water plants")]);
    app.toggle_complete(1);
    // Simulate a v2-migrated entry: back-link stripped.
    app.data.log[0].task = None;

    app.toggle_complete(1);
    assert_eq!(app.data.log.len(), 0, "text fallback still finds it");
}

#[test]
fn daily_count_ignores_completions_from_non_dashboard_tasks() {
    // Only task 1 goes on the dashboard.
    let mut app = test_app(vec![task(1, "review")]);

    // Task 2 shares its text but is never added to the dashboard.
    app.data.tasks.push(task(2, "review"));

    // Complete the off-dashboard task. This must not count toward the
    // dashboard's daily total, even though its text matches task 1.
    app.toggle_complete(2);

    assert_eq!(
        app.dash_completed_today(),
        0,
        "completing a non-dashboard task must not count, even though its text matches a dashboard task"
    );
}

use crate::app::DashRow;
use crate::model::DashSection;

fn sectioned(id: u64, text: &str, sec: DashSection) -> crate::model::Task {
    let mut t = task(id, text);
    t.dash_section = sec;
    t
}

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

#[test]
fn selected_task_is_none_on_a_section_header() {
    let mut app = test_app(vec![sectioned(1, "x", DashSection::General)]);
    app.task_sel = 0; // Important box top
    assert_eq!(app.selected_task(), None);
    app.task_sel = 4; // the task under General
    assert_eq!(app.selected_task(), Some(1));
}

#[test]
fn set_dash_section_moves_the_task_and_follows_the_cursor() {
    let mut app = test_app(vec![
        sectioned(1, "a", DashSection::General),
        sectioned(2, "b", DashSection::General),
    ]);
    app.task_sel = 5; // one of the tasks under General (exact row is irrelevant here —
                      // set_dash_section acts on the task id, not the cursor)

    app.set_dash_section(1, DashSection::Important);

    assert_eq!(app.data.tasks[0].dash_section, DashSection::Important);
    assert_eq!(
        app.dash_rows()[1],
        DashRow::Task(1),
        "task 1 now sits directly under the Important box top"
    );
    assert_eq!(app.task_sel, 1, "cursor follows the task it just moved");
}

#[test]
fn moving_a_task_down_stays_inside_its_section() {
    let mut app = test_app(vec![
        sectioned(1, "first", DashSection::General),
        sectioned(2, "second", DashSection::General),
        sectioned(3, "agent task", DashSection::Agent),
    ]);
    // rows: 0 top(Imp) 1 fill(Imp) 2 bottom(Imp) 3 top(Gen) 4 task1 5 task2
    //       6 bottom(Gen) 7 top(Agent) 8 task3 9 bottom(Agent)
    app.task_sel = 5; // "second"

    app.move_dash_task(1);

    assert_eq!(
        app.dash_rows()[7],
        DashRow::SectionTop(DashSection::Agent),
        "task 2 did not leak into the Agent section"
    );
    assert_eq!(
        app.task_sel, 5,
        "cursor did not move — already last in section"
    );
}

#[test]
fn moving_a_task_up_swaps_with_its_section_sibling() {
    let mut app = test_app(vec![
        sectioned(1, "first", DashSection::General),
        sectioned(2, "second", DashSection::General),
    ]);
    // rows: 0 top(Imp) 1 fill(Imp) 2 bottom(Imp) 3 top(Gen) 4 task1 5 task2 …
    app.task_sel = 5; // "second"

    app.move_dash_task(-1);

    assert_eq!(app.dash_rows()[4], DashRow::Task(2));
    assert_eq!(app.dash_rows()[5], DashRow::Task(1));
    assert_eq!(app.task_sel, 4, "cursor follows the moved task");
}

#[test]
fn make_priority_moves_the_task_to_the_top_of_important() {
    let mut app = test_app(vec![
        sectioned(1, "a", DashSection::Important),
        sectioned(2, "b", DashSection::Agent),
    ]);

    app.make_priority(2);

    assert_eq!(app.data.tasks[1].dash_section, DashSection::Important);
    assert_eq!(app.dash_rows()[1], DashRow::Task(2), "top of Important");
}

#[test]
fn pinning_a_course_task_lands_it_in_general() {
    let mut app = test_app(vec![]);
    let mut t = task(9, "lecture 7");
    t.dash_section = DashSection::Agent; // stale value from an earlier stint
    app.data.tasks.push(t);
    app.data.dashboard.clear();

    app.toggle_pin(9);

    assert_eq!(app.data.tasks[0].dash_section, DashSection::General);
    assert!(app.data.dashboard.contains(&9));
}

use crate::model::{Repeat, RepeatUnit};

fn habit(id: u64, text: &str, every: u32, unit: RepeatUnit, anchor: &str) -> crate::model::Task {
    let mut t = task(id, text);
    t.repeat = Some(Repeat {
        every,
        unit,
        anchor: anchor.to_string(),
    });
    t
}

#[test]
fn tick_resets_a_completed_habit_when_the_period_rolls_over() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);
    app.data.tasks[0].completed = true;
    app.data.tasks[0].period = Some("2026-08-25".to_string());

    app.tick_on(d("2026-08-26"));

    assert!(
        !app.data.tasks[0].completed,
        "new day, task is pending again"
    );
    assert_eq!(app.data.tasks[0].period, Some("2026-08-26".to_string()));
}

#[test]
fn tick_leaves_a_habit_alone_inside_its_period() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);
    app.data.tasks[0].completed = true;
    app.data.tasks[0].period = Some("2026-08-26".to_string());

    app.tick_on(d("2026-08-26"));

    assert!(app.data.tasks[0].completed, "same day, stays done");
}

#[test]
fn tick_never_touches_non_recurring_tasks() {
    let mut app = test_app(vec![task(1, "one off")]);
    app.data.tasks[0].completed = true;

    app.tick_on(d("2026-08-26"));

    assert!(app.data.tasks[0].completed);
    assert_eq!(app.data.tasks[0].period, None);
}

#[test]
fn tick_does_not_push_onto_the_undo_stack() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);
    app.data.tasks[0].completed = true;
    app.data.tasks[0].period = Some("2026-08-25".to_string());

    app.tick_on(d("2026-08-26"));

    assert!(
        app.undo_stack.is_empty(),
        "a midnight rollover is not a user edit — u must undo the last real action"
    );
}

#[test]
fn a_missed_period_leaves_no_trace() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);
    app.data.tasks[0].period = Some("2026-08-20".to_string()); // never completed

    app.tick_on(d("2026-08-26"));

    assert!(!app.data.tasks[0].completed);
    assert_eq!(app.data.log.len(), 0, "no backlog, no phantom commits");
    assert_eq!(app.data.tasks[0].due, None, "habits never gain a due date");
}

#[test]
fn adding_a_task_with_only_tokens_reports_an_error() {
    let mut app = test_app(vec![]);

    app.confirm_insert(crate::app::InsertTarget::DashTask, "#i".to_string());

    assert!(app.data.tasks.is_empty(), "nothing was created");
    assert!(
        app.status.to_lowercase().contains("name") || app.status.to_lowercase().contains("empty"),
        "user gets told why nothing happened, got: {:?}",
        app.status
    );
}

#[test]
fn unlinking_a_task_leaves_its_section_alone() {
    let mut app = test_app(vec![sectioned(1, "lecture 7", DashSection::Important)]);

    app.toggle_pin(1); // already on the dashboard, so this unlinks

    assert!(!app.data.dashboard.contains(&1), "unlinked");
    assert_eq!(
        app.data.tasks[0].dash_section,
        DashSection::Important,
        "section survives an unlink, so re-linking doesn't lose it"
    );
}

#[test]
fn completing_a_habit_logs_a_commit_and_hides_it() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);

    app.toggle_complete(1);

    assert_eq!(
        app.data.log.len(),
        1,
        "habits feed the heatmap like anything else"
    );
    assert!(app.data.tasks[0].completed);
    assert!(
        app.dash_ids().is_empty(),
        "done for this period, so it disappears from the dashboard"
    );
}

#[test]
fn habits_are_excluded_from_course_progress() {
    let mut app = test_app(vec![]);
    app.data.projects.push(crate::model::Project {
        id: 7,
        name: "Algorithms 2".into(),
        color: 0,
        archived: false,
    });

    let mut one_off = task(1, "lecture 7");
    one_off.project = Some(7);
    one_off.completed = true;

    let mut h = habit(2, "review flashcards", 1, RepeatUnit::Day, "2026-08-01");
    h.project = Some(7);

    app.data.tasks = vec![one_off, h];

    assert_eq!(
        app.project_stats(7),
        (1, 1),
        "the habit is not counted — it never finishes, so it would make the bar oscillate"
    );
}

#[test]
fn setting_a_due_date_on_a_habit_is_refused() {
    let mut app = test_app(vec![habit(
        1,
        "water plants",
        1,
        RepeatUnit::Day,
        "2026-08-01",
    )]);

    app.apply_due(1, Some("2026-09-01".to_string()));

    assert_eq!(app.data.tasks[0].due, None, "habits never carry a due date");
}

#[test]
fn setting_a_repeat_clears_an_existing_due_date() {
    let mut app = test_app(vec![task(1, "water plants")]);
    app.data.tasks[0].due = Some("2026-09-01".to_string());

    app.set_repeat(
        1,
        Some(Repeat {
            every: 1,
            unit: RepeatUnit::Day,
            anchor: "2026-08-26".to_string(),
        }),
    );

    assert_eq!(app.data.tasks[0].due, None);
    assert!(app.data.tasks[0].repeat.is_some());
    assert!(app.data.tasks[0].period.is_some(), "stamped immediately");
}

// ---------------------------------------------------------------- navigation

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

    assert_eq!(
        app.task_sel, 3,
        "General's header, not its top border chrome"
    );
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
    assert_eq!(
        row,
        DashRow::Task(1),
        "G lands on the task, not the box bottom"
    );
}

#[test]
fn clamp_never_parks_the_cursor_on_chrome() {
    let mut app = test_app(vec![sectioned(1, "a", DashSection::General)]);
    // Past the end — clamp used to settle on the very last row, which is
    // always a box bottom.
    app.task_sel = 99;

    app.clamp();

    let row = app.dash_rows()[app.task_sel];
    assert!(row.is_selectable(), "clamped onto {:?}", row);
}

// ------------------------------------------------------------------- courses

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
