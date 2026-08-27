//! The status bar, the freeform insert-mode text popup, and the due-date
//! preset picker.

use chrono::{Duration as CDuration, Local, Months};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, InsertTarget, Mode, View};
use crate::ui::theme::{ACCENT, DIM};

pub fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let line = match &app.mode {
        Mode::Command(buf) => Line::from(vec![
            Span::styled(":", Style::default().fg(Color::White)),
            Span::raw(buf.clone()),
            Span::styled("█", Style::default().fg(Color::White)),
        ]),
        Mode::Insert(_, _) => Line::from(Span::styled(
            " -- INSERT --  (Enter: confirm · Esc: cancel)",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Mode::DueMenu { .. } => Line::from(Span::styled(
            " -- DUE PICKER --  (1-7 preset · 0 clear · c custom · Esc cancel)",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Mode::RepeatMenu { .. } => Line::from(Span::styled(
            " -- REPEAT PICKER --  (1-5 preset · 0 clear · c custom · Esc cancel)",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Mode::Normal => {
            let pend = match app.pending {
                Some(c) => format!("  [{}‥]", c),
                None => String::new(),
            };
            let mode_tag = match app.view {
                View::Course => " COURSE ",
                View::Log => " LOG ",
                View::DayLog => " DAY LOG ",
                View::Help => " HELP ",
                View::Dashboard => " NORMAL ",
            };
            Line::from(vec![
                Span::styled(
                    mode_tag,
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}{}", app.status, pend),
                    Style::default().fg(DIM),
                ),
            ])
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

pub fn draw_input_popup(f: &mut Frame, target: &InsertTarget, buf: &str) {
    let title = match target {
        InsertTarget::DashTask => " New task  (…text @course !due) ",
        InsertTarget::CourseTask { .. } => " New task in course  (…text !due) ",
        InsertTarget::Course => " New course / project ",
        InsertTarget::Section { .. } => " New section (subtitle) ",
        InsertTarget::Due { .. } => " Due date  (YYYY-MM-DD · 7-10 · +3d · +2w · +1mo · today · none) ",
        InsertTarget::Repeat { .. } => " repeat: 3d · 2w · 1mo · daily · none ",
        InsertTarget::RenameTask { .. } => " Rename task ",
        InsertTarget::RenameSection { .. } => " Rename section ",
        InsertTarget::RenameCourse { .. } => " Rename course ",
    };
    let display: String = buf.to_string();
    let area = f.area();
    let w = (area.width.saturating_sub(10)).min(70).max(24);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + area.height / 2 - 1,
        width: w,
        height: 3,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightGreen))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
    let text = Line::from(vec![
        Span::raw(display),
        Span::styled("█", Style::default().fg(Color::LightGreen)),
    ]);
    f.render_widget(Paragraph::new(text).block(block), popup);
}

pub fn draw_due_menu(f: &mut Frame, app: &App, task: u64) {
    let today = Local::now().date_naive();
    let d_days = |n: i64| (today + CDuration::days(n)).format("%b %d").to_string();
    let d_months = |n: u32| {
        today
            .checked_add_months(Months::new(n))
            .unwrap_or(today)
            .format("%b %d")
            .to_string()
    };
    let current = app
        .task(task)
        .and_then(|t| t.due.clone())
        .unwrap_or_else(|| String::from("(none)"));

    let key_style = Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    let row = |k: &'static str, label: &'static str, when: String| {
        Line::from(vec![
            Span::styled(format!("  [{}]  ", k), key_style),
            Span::raw(label),
            Span::styled(format!("   → {}", when), dim),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  currently: {}", current),
            Style::default().fg(DIM),
        )),
        Line::from(""),
        row("1", "Today       ", d_days(0)),
        row("2", "Tomorrow    ", d_days(1)),
        row("3", "In 3 days   ", d_days(3)),
        row("4", "In 1 week   ", d_days(7)),
        row("5", "In 2 weeks  ", d_days(14)),
        row("6", "In 1 month  ", d_months(1)),
        row("7", "In 3 months ", d_months(3)),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [0]  ", key_style),
            Span::raw("Clear due date"),
        ]),
        Line::from(vec![
            Span::styled("  [c]  ", key_style),
            Span::raw("Custom…  "),
            Span::styled(
                "(YYYY-MM-DD · +3d · +2w · +1mo · tomorrow · none)",
                Style::default().fg(DIM),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Esc / q  cancel",
            Style::default().fg(DIM),
        )),
    ];

    let area = f.area();
    let w = 58u16.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightGreen))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " Due date — pick a preset ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

pub fn draw_repeat_menu(f: &mut Frame, app: &App, task: u64) {
    let current = app
        .task(task)
        .and_then(|t| t.repeat.as_ref())
        .map(|r| r.label())
        .unwrap_or_else(|| String::from("(not recurring)"));

    let key_style = Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    let row = |k: &'static str, label: &'static str| {
        Line::from(vec![
            Span::styled(format!("  [{}]  ", k), key_style),
            Span::raw(label),
        ])
    };
    let rows = [
        ("1", "daily"),
        ("2", "every 3 days"),
        ("3", "weekly"),
        ("4", "every 2 weeks"),
        ("5", "monthly"),
        ("0", "not recurring"),
        ("c", "custom…"),
    ];
    let mut lines = vec![
        Line::from(Span::styled(
            format!("  currently: {}", current),
            Style::default().fg(DIM),
        )),
        Line::from(""),
    ];
    for (k, label) in rows {
        lines.push(row(k, label));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "(custom: 3d · 2w · 1mo · daily · weekly · monthly · none)",
            dim,
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Esc / q  cancel",
        Style::default().fg(DIM),
    )));

    let area = f.area();
    let w = 50u16.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightGreen))
        .padding(Padding::horizontal(1))
        .title(Span::styled(
            " repeat ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), popup);
}
