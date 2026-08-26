//! The current-priority card, the daily-goal gauge, and the GitHub-style
//! contribution heatmap.

use std::collections::HashSet;

use chrono::{Datelike, Duration as CDuration, Local, NaiveDate};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{due_span, ACCENT, DIM};

pub fn draw_priority(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " CURRENT PRIORITY ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));

    let first = app.dash_ids().first().and_then(|id| app.task(*id));
    let lines = match first {
        Some(t) => {
            let name = match app.project_of(t.project) {
                Some(p) => match app.section_of(t.section) {
                    Some(s) => format!("[{} - {}] {}", p.name, s.name, t.text),
                    None => format!("[{}] {}", p.name, t.text),
                },
                None => t.text.clone(),
            };
            let mut title_spans = vec![Span::styled(
                name,
                Style::default().add_modifier(Modifier::BOLD),
            )];
            if let Some(s) = due_span(&t.due) {
                title_spans.push(s);
            }
            vec![
                Line::from(""),
                Line::from(title_spans).alignment(Alignment::Center),
                Line::from(Span::styled(
                    "Stay strictly on this task until completion.",
                    Style::default().fg(DIM),
                ))
                .alignment(Alignment::Center),
                Line::from(""),
                Line::from(Span::styled(
                    "[ c  Complete Task ]",
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center),
            ]
        }
        None => vec![
            Line::from(""),
            Line::from(Span::styled(
                "Dashboard clear ✓",
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            Line::from(Span::styled(
                "open a course (⏎) and press p to link a task here",
                Style::default().fg(DIM),
            ))
            .alignment(Alignment::Center),
        ],
    };

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        area,
    );
}

pub fn draw_daily_goal(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            " DAILY GOAL ",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " +/- adjust ",
            Style::default().fg(DIM),
        )));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let done = app.today_count();
    let goal = app.data.daily_goal.max(1);
    let ratio = (done as f64 / goal as f64).min(1.0);
    let streak = app.streak();

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner.inner(Margin {
        horizontal: 1,
        vertical: 1,
    }));

    let pct = Line::from(vec![
        Span::styled(
            format!("{:.0}% ", ratio * 100.0),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{}/{}", done, goal), Style::default().fg(DIM)),
        Span::raw("   "),
        Span::styled(
            format!("🔥 {} day streak", streak),
            Style::default().fg(if streak > 0 {
                Color::LightYellow
            } else {
                DIM
            }),
        ),
    ]);
    f.render_widget(Paragraph::new(pct), rows[0]);

    let gauge = Gauge::default()
        .ratio(ratio)
        .use_unicode(true)
        .gauge_style(
            Style::default()
                .fg(Color::LightGreen)
                .bg(Color::Rgb(40, 40, 55)),
        )
        .label("");
    f.render_widget(gauge, rows[1]);

    let msg = if done == 0 {
        "Let's get started."
    } else if done < goal {
        "You're on your way!"
    } else {
        "Goal complete — keep the streak alive ✦"
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("Goal: {} tasks/day", goal),
            Style::default().fg(DIM),
        )),
        rows[2],
    );
    f.render_widget(
        Paragraph::new(Span::styled(msg, Style::default().fg(DIM))),
        rows[3],
    );
}

fn heat_cell(count: u32, is_today: bool, month: u32, is_month_start: bool) -> Span<'static> {
    // Alternate empty-cell tone by month to produce GitHub-style banding.
    let dim_a = Color::Rgb(48, 50, 64);
    let dim_b = Color::Rgb(72, 74, 92);
    let empty_color = if month % 2 == 0 { dim_a } else { dim_b };
    let (ch, color) = match count {
        0 => ("·", empty_color),
        1 => ("■", Color::Rgb(60, 100, 60)),
        2 => ("■", Color::Rgb(60, 150, 60)),
        3 => ("■", Color::Rgb(80, 200, 80)),
        _ => ("■", Color::Rgb(120, 255, 120)),
    };
    let mut style = Style::default().fg(color);
    if is_month_start {
        // Left-border on the first day of each month makes month starts pop.
        style = style.add_modifier(Modifier::BOLD);
    }
    if is_today {
        style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    }
    Span::styled(ch, style)
}

pub fn draw_heatmap(f: &mut Frame, app: &App, area: Rect) {
    let counts = app.counts_by_date();
    let year = app.year;
    let today = Local::now().date_naive();

    let jan1 = match NaiveDate::from_ymd_opt(year, 1, 1) {
        Some(d) => d,
        None => return,
    };
    let dec31 = NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(jan1);
    let offset = jan1.weekday().num_days_from_monday() as i64; // Mon=0
    let days_in_year = (dec31 - jan1).num_days() + 1;
    let weeks = ((offset + days_in_year) as usize + 6) / 7;

    let mut total = 0u32;
    let mut active: HashSet<NaiveDate> = HashSet::new();
    for (d, c) in &counts {
        if d.year() == year {
            total += c;
            active.insert(*d);
        }
    }

    // Precompute the first week of each month so we can drop a divider column there.
    // Skip January (week 0) since it's already the left edge.
    let mut month_start_weeks: HashSet<usize> = HashSet::new();
    for m in 2..=12u32 {
        if let Some(first) = NaiveDate::from_ymd_opt(year, m, 1) {
            let w = ((offset + (first - jan1).num_days()) / 7) as usize;
            if w < weeks {
                month_start_weeks.insert(w);
            }
        }
    }

    // Effective column position of week `w` in the rendered grid (accounts for dividers).
    let col_of_week = |w: usize| -> usize {
        w + month_start_weeks.iter().filter(|mw| **mw <= w).count()
    };

    let mut month_row = vec![Span::raw("     ")];
    let mut cursor = 0usize;
    for m in 1..=12 {
        if let Some(first) = NaiveDate::from_ymd_opt(year, m, 1) {
            let w = ((offset + (first - jan1).num_days()) / 7) as usize;
            if w < weeks {
                let col = col_of_week(w);
                if col >= cursor {
                    month_row.push(Span::raw(" ".repeat(col - cursor)));
                    let label = first.format("%b").to_string();
                    cursor = col + label.len();
                    month_row.push(Span::styled(label, Style::default().fg(DIM)));
                }
            }
        }
    }

    let mut lines: Vec<Line> = vec![Line::from(month_row)];
    for row in 0..7u32 {
        let gutter = match row {
            0 => "Mon  ",
            2 => "Wed  ",
            4 => "Fri  ",
            _ => "     ",
        };
        let mut spans = vec![Span::styled(gutter, Style::default().fg(DIM))];
        for w in 0..weeks {
            if month_start_weeks.contains(&w) {
                // Thin divider column that separates months.
                spans.push(Span::styled("│", Style::default().fg(DIM)));
            }
            let day_index = w as i64 * 7 + row as i64 - offset;
            if day_index < 0 || day_index >= days_in_year {
                spans.push(Span::raw(" "));
                continue;
            }
            let date = jan1 + CDuration::days(day_index);
            let c = counts.get(&date).copied().unwrap_or(0);
            let is_month_start = date.day() == 1;
            spans.push(heat_cell(c, date == today, date.month(), is_month_start));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(vec![
        Span::styled(
            format!("     {} contributions in {}", total, year),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ·  {} active days", active.len()),
            Style::default().fg(DIM),
        ),
        Span::styled("    less ", Style::default().fg(DIM)),
        heat_cell(0, false, 1, false),
        heat_cell(1, false, 1, false),
        heat_cell(2, false, 1, false),
        heat_cell(3, false, 1, false),
        heat_cell(4, false, 1, false),
        Span::styled(" more", Style::default().fg(DIM)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(
            format!(" CONTRIBUTION TRACKER — {} ", year),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " [ / ] change year · o: history log · u: undo last ",
            Style::default().fg(DIM),
        )));
    f.render_widget(Paragraph::new(lines).block(block), area);
}
