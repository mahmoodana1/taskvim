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
fn dash_rows_groups_tasks_under_all_three_headers() {
    let app = test_app(vec![
        sectioned(1, "b", DashSection::General),
        sectioned(2, "a", DashSection::Important),
        sectioned(3, "c", DashSection::Agent),
    ]);

    assert_eq!(
        app.dash_rows(),
        vec![
            DashRow::Section(DashSection::Important),
            DashRow::Task(2),
            DashRow::Section(DashSection::General),
            DashRow::Task(1),
            DashRow::Section(DashSection::Agent),
            DashRow::Task(3),
        ]
    );
}

#[test]
fn dash_rows_renders_empty_section_headers() {
    let app = test_app(vec![sectioned(1, "only", DashSection::General)]);

    assert_eq!(
        app.dash_rows(),
        vec![
            DashRow::Section(DashSection::Important),
            DashRow::Section(DashSection::General),
            DashRow::Task(1),
            DashRow::Section(DashSection::Agent),
        ],
        "empty sections still render so 1/2/3 always have a visible target"
    );
}

#[test]
fn dash_rows_preserves_dashboard_order_within_a_section() {
    let mut app = test_app(vec![
        sectioned(1, "first", DashSection::General),
        sectioned(2, "second", DashSection::General),
    ]);
    app.data.dashboard = vec![2, 1];

    assert_eq!(
        app.dash_rows(),
        vec![
            DashRow::Section(DashSection::Important),
            DashRow::Section(DashSection::General),
            DashRow::Task(2),
            DashRow::Task(1),
            DashRow::Section(DashSection::Agent),
        ]
    );
}

#[test]
fn selected_task_is_none_on_a_section_header() {
    let mut app = test_app(vec![sectioned(1, "x", DashSection::General)]);
    app.task_sel = 0; // Important header
    assert_eq!(app.selected_task(), None);
    app.task_sel = 2; // the task under General
    assert_eq!(app.selected_task(), Some(1));
}

#[test]
fn set_dash_section_moves_the_task_and_follows_the_cursor() {
    let mut app = test_app(vec![
        sectioned(1, "a", DashSection::General),
        sectioned(2, "b", DashSection::General),
    ]);
    app.task_sel = 2; // first task under General

    app.set_dash_section(1, DashSection::Important);

    assert_eq!(app.data.tasks[0].dash_section, DashSection::Important);
    assert_eq!(
        app.dash_rows()[1],
        DashRow::Task(1),
        "task 1 now sits directly under the Important header"
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
    // rows: 0 Important · 1 General · 2 task1 · 3 task2 · 4 Agent · 5 task3
    app.task_sel = 3; // "second"

    app.move_dash_task(1);

    assert_eq!(
        app.dash_rows()[4],
        DashRow::Section(DashSection::Agent),
        "task 2 did not leak into the Agent section"
    );
    assert_eq!(app.task_sel, 3, "cursor did not move — already last in section");
}

#[test]
fn moving_a_task_up_swaps_with_its_section_sibling() {
    let mut app = test_app(vec![
        sectioned(1, "first", DashSection::General),
        sectioned(2, "second", DashSection::General),
    ]);
    app.task_sel = 3; // "second"

    app.move_dash_task(-1);

    assert_eq!(app.dash_rows()[2], DashRow::Task(2));
    assert_eq!(app.dash_rows()[3], DashRow::Task(1));
    assert_eq!(app.task_sel, 2, "cursor follows the moved task");
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
    t.repeat = Some(Repeat { every, unit, anchor: anchor.to_string() });
    t
}

#[test]
fn tick_resets_a_completed_habit_when_the_period_rolls_over() {
    let mut app = test_app(vec![habit(1, "water plants", 1, RepeatUnit::Day, "2026-08-01")]);
    app.data.tasks[0].completed = true;
    app.data.tasks[0].period = Some("2026-08-25".to_string());

    app.tick_on(d("2026-08-26"));

    assert!(!app.data.tasks[0].completed, "new day, task is pending again");
    assert_eq!(app.data.tasks[0].period, Some("2026-08-26".to_string()));
}

#[test]
fn tick_leaves_a_habit_alone_inside_its_period() {
    let mut app = test_app(vec![habit(1, "water plants", 1, RepeatUnit::Day, "2026-08-01")]);
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
    let mut app = test_app(vec![habit(1, "water plants", 1, RepeatUnit::Day, "2026-08-01")]);
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
    let mut app = test_app(vec![habit(1, "water plants", 1, RepeatUnit::Day, "2026-08-01")]);
    app.data.tasks[0].period = Some("2026-08-20".to_string()); // never completed

    app.tick_on(d("2026-08-26"));

    assert!(!app.data.tasks[0].completed);
    assert_eq!(app.data.log.len(), 0, "no backlog, no phantom commits");
    assert_eq!(app.data.tasks[0].due, None, "habits never gain a due date");
}
