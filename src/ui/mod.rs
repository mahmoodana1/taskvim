//! Top-level frame layout: dispatches to the per-view draw functions and
//! overlays the status bar and any active modal.

pub mod courses;
pub mod dashboard;
pub mod heatmap;
pub mod help;
pub mod log;
pub mod popup;
pub mod theme;

use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use crate::app::{App, Mode, View};
use crate::ui::courses::draw_course_modal;
use crate::ui::dashboard::draw_dashboard;
use crate::ui::help::draw_help;
use crate::ui::log::{draw_daylog, draw_log};
use crate::ui::popup::{draw_due_menu, draw_input_popup, draw_repeat_menu, draw_statusbar};

pub fn ui(f: &mut Frame, app: &App) {
    let root = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
    let body = root[0];
    let statusbar = root[1];

    match app.view {
        View::Help => draw_help(f, body),
        View::Log => draw_log(f, app, body),
        View::DayLog => draw_daylog(f, app, body),
        View::Dashboard => draw_dashboard(f, app, body),
        View::Course => {
            draw_dashboard(f, app, body);
            draw_course_modal(f, app, body);
        }
    }

    draw_statusbar(f, app, statusbar);

    if let Mode::Insert(target, buf) = &app.mode {
        draw_input_popup(f, target, buf);
    }
    if let Mode::DueMenu { task } = &app.mode {
        draw_due_menu(f, app, *task);
    }
    if let Mode::RepeatMenu { task } = &app.mode {
        draw_repeat_menu(f, app, *task);
    }
}
