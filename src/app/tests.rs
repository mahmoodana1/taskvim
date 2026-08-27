use crate::app::App;
use crate::model::{Data, Task};

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
