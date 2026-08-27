//! The dashboard view: task sidebar, greeting banner, and the task-row
//! rendering used by the sidebar's list.

use chrono::{Local, Timelike};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, DashRow, Pane, View};
use crate::model::Task;
use crate::ui::courses::draw_courses;
use crate::ui::heatmap::{draw_daily_goal, draw_heatmap, draw_priority};
use crate::ui::theme::{due_span, ellipsize, palette, ACCENT, DIM, SEL_BG};

pub fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Length(40), Constraint::Min(30)]).split(area);
    draw_task_sidebar(f, app, cols[0]);

    let main = Layout::vertical([
        Constraint::Length(3),  // greeting
        Constraint::Length(6),  // priority + daily goal (reduced)
        Constraint::Length(10), // heatmap (reduced)
        Constraint::Min(10),    // courses (increased minimum)
    ])
    .split(cols[1]);

    draw_greeting(f, main[0]);

    let cards =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(main[1]);
    draw_priority(f, app, cards[0]);
    draw_daily_goal(f, app, cards[1]);

    draw_heatmap(f, app, main[2]);
    draw_courses(f, app, main[3]);
}

pub fn draw_greeting(f: &mut Frame, area: Rect) {
    let now = Local::now();
    let greeting = match now.hour() {
        5..=11 => "Good Morning",
        12..=17 => "Good Afternoon",
        _ => "Good Evening",
    };
    let date = now.format("%A, %B %d").to_string().to_uppercase();
    let text = vec![
        Line::from(Span::styled(date, Style::default().fg(DIM))),
        Line::from(Span::styled(
            greeting,
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    f.render_widget(
        Paragraph::new(text).block(Block::default().padding(Padding::horizontal(1))),
        area,
    );
}

pub fn task_line(app: &App, t: &Task, show_pin: bool, indent: bool) -> ListItem<'static> {
    let mut spans = vec![];
    if indent {
        // Two columns, so the task reads as belonging to the header above it.
        spans.push(Span::raw("  "));
    }
    let style = if t.completed {
        Style::default().fg(DIM).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };

    match app.project_of(t.project) {
        Some(p) => {
            spans.push(Span::styled("▎", Style::default().fg(palette(p.color))));
            let proj_style = if t.completed {
                Style::default().fg(DIM).add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(palette(p.color))
            };
            const PROJ_CAP: usize = 14;
            const SECT_CAP: usize = 14;
            let pname = ellipsize(&p.name, PROJ_CAP);
            let label = match app.section_of(t.section) {
                Some(s) => format!("[{} - {}] ", pname, ellipsize(&s.name, SECT_CAP)),
                None => format!("[{}] ", pname),
            };
            spans.push(Span::styled(label, proj_style));
        }
        None => spans.push(Span::styled("▎", Style::default().fg(DIM))),
    }
    let text = if t.completed {
        format!("✓ {}", t.text)
    } else {
        t.text.clone()
    };
    spans.push(Span::styled(text, style));
    if show_pin && app.is_pinned(t.id) {
        spans.push(Span::styled("  ↗", Style::default().fg(Color::LightCyan)));
    }
    let mut lines = vec![Line::from(spans)];
    if !t.completed {
        if let Some(s) = due_span(&t.due) {
            // Second line so long titles never collide with the due badge.
            let lead = if indent { "   " } else { " " };
            lines.push(Line::from(vec![Span::raw(lead), s]));
        }
    }
    ListItem::new(lines)
}

pub fn draw_task_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Tasks && app.view == View::Dashboard;
    let border = if active { ACCENT } else { DIM };
    let done_today = app.dash_completed_today();
    // 1/2/3, t and dd all act on the task under the cursor — hide them when
    // the cursor is sitting on a section header instead.
    let on_task = !active || app.selected_task().is_some();
    let footer = match (on_task, done_today > 0) {
        (true, true) => format!(
            " a:add ⏎:done 1/2/3:section t:due dd:unlink · {} done today (:log) ",
            done_today
        ),
        (true, false) => " a:add ⏎:done 1/2/3:section t:due dd:unlink r:random ".to_string(),
        (false, true) => format!(" a:add · {} done today (:log) ", done_today),
        (false, false) => " a:add r:random ".to_string(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" Tasks — dashboard ({}) ", app.dash_ids().len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(DIM))));

    let rows = app.dash_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            DashRow::Section(sec) => {
                let count = app
                    .dash_ids()
                    .iter()
                    .filter(|id| {
                        app.task(**id).map(|t| t.dash_section == *sec).unwrap_or(false)
                    })
                    .count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("▸ {}", sec.title()),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  {}", count), Style::default().fg(DIM)),
                ]))
            }
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => task_line(app, t, false, true),
                None => ListItem::new(Line::from("")),
            },
        })
        .collect();

    if app.dash_ids().is_empty() {
        let msg = if done_today > 0 {
            format!(
                "\n  all clear ✓\n\n  {} completed today.\n  :log for history\n\n  a to add a task\n  r for a random one",
                done_today
            )
        } else {
            "\n  dashboard is clear —\n\n  a       add a task here\n  ⏎ on a course, then p\n          to link its tasks\n  r       pull a random one\n          from the backlog".to_string()
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(DIM)).block(block);
        f.render_widget(empty, area);
        return;
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    if active {
        state.select(Some(app.task_sel.min(rows.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut state);
}
