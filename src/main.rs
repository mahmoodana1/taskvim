// taskvim v0.2 — a vim-motions-driven terminal task / project / course manager.
//
// Model:
//   Courses hold the full backlog, organized under sections ("subtitles").
//   The main dashboard's Tasks list contains only tasks you pin/link there
//   (p inside a course) or add manually. Completing a task anywhere creates
//   a "commit" that feeds the heatmap, streak, daily goal and course stats.
//
// Data lives in $XDG_DATA_HOME/taskvim/data.json (~/.local/share/taskvim/).
// Fully local — no cloud sync.

use std::collections::HashSet;
use std::io;
use std::time::Duration;

use chrono::{Datelike, Duration as CDuration, Local, Months, NaiveDate, Timelike};
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Gauge, List, ListItem, ListState, Padding, Paragraph, Wrap,
};
use ratatui::{DefaultTerminal, Frame};

mod app;
mod due;
mod model;

use app::{App, InsertTarget, Mode, Pane, Row, View};
use model::Task;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

const PALETTE: [Color; 6] = [
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
    Color::LightCyan,
];

const ACCENT: Color = Color::LightMagenta;
const DIM: Color = Color::DarkGray;
const SEL_BG: Color = Color::Rgb(45, 45, 65);

fn palette(idx: usize) -> Color {
    PALETTE[idx % PALETTE.len()]
}

// ---------------------------------------------------------------------------
// Due-date parsing & display
// ---------------------------------------------------------------------------

fn due_span(due: &Option<String>) -> Option<Span<'static>> {
    let d = NaiveDate::parse_from_str(due.as_deref()?, "%Y-%m-%d").ok()?;
    let today = Local::now().date_naive();
    let (txt, style) = if d < today {
        (
            format!("  ⚠ was due {}", d.format("%b %d")),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )
    } else if d == today {
        (
            String::from("  ● due today"),
            Style::default().fg(Color::LightYellow),
        )
    } else if d == today + CDuration::days(1) {
        (
            String::from("  due tomorrow"),
            Style::default().fg(Color::Yellow),
        )
    } else {
        (
            format!("  due {}", d.format("%b %d")),
            Style::default().fg(DIM),
        )
    };
    Some(Span::styled(txt, style))
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn ui(f: &mut Frame, app: &App) {
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
}

fn draw_due_menu(f: &mut Frame, app: &App, task: u64) {
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

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_greeting(f: &mut Frame, area: Rect) {
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

fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

fn task_line(app: &App, t: &Task, show_pin: bool) -> ListItem<'static> {
    let mut spans = vec![];
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
            lines.push(Line::from(vec![Span::raw(" "), s]));
        }
    }
    ListItem::new(lines)
}

fn draw_task_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Tasks && app.view == View::Dashboard;
    let border = if active { ACCENT } else { DIM };
    let visible = app.dash_ids();
    let done_today = app.dash_completed_today();
    let footer = if done_today > 0 {
        format!(
            " a:add ⏎:done t:due dd:unlink r:random · {} done today (:log) ",
            done_today
        )
    } else {
        " a:add ⏎:done t:due dd:unlink r:random ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" Tasks — dashboard ({}) ", visible.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(DIM))));

    let items: Vec<ListItem> = visible
        .iter()
        .filter_map(|id| app.task(*id))
        .map(|t| task_line(app, t, false))
        .collect();

    if items.is_empty() {
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
        state.select(Some(app.task_sel.min(visible.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_priority(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_daily_goal(f: &mut Frame, app: &App, area: Rect) {
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

// ------------------------------------------------------------------- heatmap

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

fn draw_heatmap(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_courses(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Courses && app.view == View::Dashboard;

    let n_projects = app.data.projects.len();
    let inner_h = area.height.saturating_sub(2) as usize;
    let max_visible = inner_h / 2;
    let hidden = n_projects.saturating_sub(max_visible);

    let title = if hidden > 0 {
        format!(" Courses & Projects  (+{} hidden) ", hidden)
    } else {
        String::from(" Courses & Projects ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { ACCENT } else { DIM }))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " ⏎:open a:new course A:quick task e:rename dd:delete ",
            Style::default().fg(DIM),
        )));

    if app.data.projects.is_empty() {
        f.render_widget(
            Paragraph::new("\n no courses yet — press a to create one")
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .data
        .projects
        .iter()
        .map(|p| {
            let (done, total) = app.project_stats(p.id);
            let overdue = app.overdue_count(p.id);
            let pct = if total == 0 {
                0.0
            } else {
                done as f64 / total as f64
            };
            let color = palette(p.color);

            let bar_w = 18usize;
            let filled = (pct * bar_w as f64).round() as usize;
            let bar: String = "█".repeat(filled) + &"─".repeat(bar_w.saturating_sub(filled));

            let name_line = Line::from(vec![
                Span::styled("  ● ", Style::default().fg(color)),
                Span::styled(
                    p.name.clone(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]);

            let mut stats_spans = vec![
                Span::styled("      ", Style::default()),
                Span::styled(
                    format!("{}/{}", done, total),
                    Style::default().fg(DIM),
                ),
                Span::styled("  ", Style::default()),
                Span::styled(bar, Style::default().fg(color)),
            ];
            if overdue > 0 {
                stats_spans.push(Span::styled(
                    format!("  ⚠{}", overdue),
                    Style::default().fg(Color::LightRed),
                ));
            }
            let stats_line = Line::from(stats_spans);

            ListItem::new(vec![name_line, stats_line])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));

    let mut state = ListState::default();
    if active {
        state.select(Some(app.course_sel.min(app.data.projects.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, area, &mut state);
}

// -------------------------------------------------------------- course modal

fn draw_course_modal(f: &mut Frame, app: &App, area: Rect) {
    let Some(pid) = app.cur_course else { return };
    let Some(p) = app.project(pid) else { return };
    let color = palette(p.color);
    let (done, total) = app.project_stats(pid);

    let w = area.width.saturating_sub(8).min(90).max(40);
    let h = area.height.saturating_sub(4).min(32).max(12);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            format!(" {} — {}/{} goals done ", p.name, done, total),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " a:task S:section ⏎:done p:link↗ t:due e:rename m:move J/K:reorder dd:del Tab:next q:close ",
            Style::default().fg(DIM),
        )));

    let rows = app.course_rows(pid);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(
                "\n  empty course —\n\n  S   add a section (e.g. Lectures, Labs, Assignments)\n  a   add a task",
            )
            .style(Style::default().fg(DIM))
            .block(block),
            popup,
        );
        return;
    }

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            Row::Section(sid) => {
                let s = app.data.sections.iter().find(|s| s.id == *sid);
                let name = s.map(|s| s.name.clone()).unwrap_or_default();
                let n = app
                    .data
                    .tasks
                    .iter()
                    .filter(|t| t.section == Some(*sid))
                    .count();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("▸ {}", name),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  ({})", n), Style::default().fg(DIM)),
                ]))
            }
            Row::NoSection => ListItem::new(Line::from(Span::styled(
                "▸ (no section)",
                Style::default().fg(DIM).add_modifier(Modifier::BOLD),
            ))),
            Row::Task(tid) => {
                let Some(t) = app.task(*tid) else {
                    return ListItem::new(Line::from(""));
                };
                let mut spans = vec![Span::raw("   ")];
                let style = if t.completed {
                    Style::default().fg(DIM).add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                if app.is_pinned(*tid) {
                    spans.push(Span::styled("↗ ", Style::default().fg(Color::LightCyan)));
                } else {
                    spans.push(Span::styled("○ ", style));
                }
                let text = if t.completed {
                    format!("✓ {}", t.text)
                } else {
                    t.text.clone()
                };
                spans.push(Span::styled(text, style));
                let mut lines = vec![Line::from(spans)];
                if !t.completed {
                    if let Some(s) = due_span(&t.due) {
                        // Indent to line up under the task text, past the bullet marker.
                        lines.push(Line::from(vec![Span::raw("     "), s]));
                    }
                }
                ListItem::new(lines)
            }
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.course_row.min(rows.len().saturating_sub(1))));
    f.render_stateful_widget(list, popup, &mut state);
}

// ----------------------------------------------------------------- log view

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let days = app.active_days_desc();
    let total_commits = app.data.log.len();
    let goal = app.data.daily_goal.max(1);
    let streak = app.streak();

    let title = format!(
        " COMPLETED HISTORY — {} commits · {} active days · {}-day streak ",
        total_commits,
        days.len(),
        streak,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " j/k · gg/G · ⏎ or d: view day details · u: undo last · q back ",
            Style::default().fg(DIM),
        )));

    if days.is_empty() {
        f.render_widget(
            Paragraph::new("\n  nothing here yet — complete a task to make your first commit")
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let today = Local::now().date_naive();
    let max_daily = days.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);
    let bar_w = 20usize;

    let items: Vec<ListItem> = days
        .iter()
        .map(|(date, count)| {
            let met = *count >= goal;
            let bullet_style = if met {
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
            } else if *count > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(DIM)
            };
            let bullet = if met { "● " } else { "○ " };

            let dow = date.format("%a").to_string();
            let mmdd = date.format("%b %e").to_string();
            let date_span = Span::styled(
                format!("{} {}", dow, mmdd.trim_start()),
                Style::default().add_modifier(Modifier::BOLD),
            );

            let count_span = Span::styled(
                format!("  {:>3} done", count),
                Style::default().fg(DIM),
            );

            let filled = ((*count as f64 / max_daily as f64) * bar_w as f64).round() as usize;
            let filled = filled.min(bar_w);
            let bar_color = if met { Color::LightGreen } else { Color::Yellow };
            let bar_span = Span::styled(
                format!("  {}{}", "▓".repeat(filled), "░".repeat(bar_w - filled)),
                Style::default().fg(bar_color),
            );

            let mut spans = vec![
                Span::styled(bullet, bullet_style),
                date_span,
                count_span,
                bar_span,
            ];

            if *date == today {
                spans.push(Span::styled(
                    "  ← today",
                    Style::default().fg(Color::LightCyan),
                ));
            }
            if met {
                let mult = count / goal;
                let label = if mult > 1 {
                    format!("  ✓ goal ×{}", mult)
                } else {
                    String::from("  ✓ goal")
                };
                spans.push(Span::styled(label, Style::default().fg(Color::LightGreen)));
            } else if *count > 0 {
                spans.push(Span::styled(
                    format!("  {}/{}", count, goal),
                    Style::default().fg(DIM),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.log_sel.min(days.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

// --------------------------------------------------------------- day log view

fn draw_daylog(f: &mut Frame, app: &App, area: Rect) {
    let Some(date) = &app.selected_date else { return };
    let logs = app.logs_for_date(date);
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            format!(" CONTRIBUTIONS — {} — {} tasks ", date, logs.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " j/k scroll · u: undo · q back ",
            Style::default().fg(DIM),
        )));

    if logs.is_empty() {
        f.render_widget(
            Paragraph::new(format!("\n  no completions on {}", date))
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = logs
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut spans = vec![
                Span::styled(format!("{}. ", i + 1), Style::default().fg(DIM)),
                Span::styled(format!("{:5} ", e.time), Style::default().fg(DIM)),
            ];
            if !e.project_name.is_empty() {
                let label = if !e.section_name.is_empty() {
                    format!("[{} - {}] ", e.project_name, e.section_name)
                } else {
                    format!("[{}] ", e.project_name)
                };
                spans.push(Span::styled(
                    label,
                    Style::default().fg(palette(e.color)),
                ));
            }
            spans.push(Span::raw(e.text.clone()));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.day_sel.min(logs.len().saturating_sub(1))));
    f.render_stateful_widget(list, area, &mut state);
}

// --------------------------------------------------------------- help view

fn draw_help(f: &mut Frame, area: Rect) {
    let key = |k: &'static str, d: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("  {:<14}", k),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(d),
        ])
    };
    let hdr = |t: &'static str| {
        Line::from(Span::styled(
            format!(" {}", t),
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ))
    };
    let lines = vec![
        Line::from(""),
        hdr("MOTIONS (everywhere)"),
        key("j k gg G", "move · jump to top / bottom"),
        key("h l Tab", "switch pane · Tab in a course = next course"),
        key("J / K", "reorder task / section"),
        Line::from(""),
        hdr("DASHBOARD"),
        key("a", "add task here (supports '@course' and '!due')"),
        key("⏎ / Space", "complete selected task · on a course: OPEN it"),
        key("c", "make selected task the CURRENT PRIORITY (move to top)"),
        key("dd", "unlink course task from dashboard (D deletes for real)"),
        key("r", "pull a random backlog task → current priority"),
        key("t / e", "set due date · rename"),
        key("+ -  [ ]", "daily goal · heatmap year"),
        key("o", "open history log"),
        key("u / Ctrl-R", "undo · redo the last action (deletion, completion, add…)"),
        Line::from(""),
        hdr("INSIDE A COURSE (⏎ on a course card)"),
        key("S", "add a section (subtitle): Lectures, Labs, …"),
        key("a", "add task under the section at the cursor"),
        key("p", "link/unlink task ↗ the main dashboard"),
        key("c", "make task the CURRENT PRIORITY (link → dashboard top)"),
        key("m / M", "move task to next / previous section"),
        key("t / e / R", "due date · rename task or section · rename course"),
        key("⏎ / dd", "complete · delete (section keeps its tasks)"),
        Line::from(""),
        hdr("DUE DATES  (t opens picker; '!' when adding: 'lab 2 @algo !+3d')"),
        key("picker", "1 today · 2 tomorrow · 3 +3d · 4 +1w · 5 +2w · 6 +1mo · 7 +3mo"),
        key("picker", "0 clear  ·  c custom text  ·  Esc cancel"),
        key("formats", "2026-07-10 · 7-10 · +3d · +2w · +1mo · today · tomorrow · none"),
        Line::from(""),
        hdr("EX COMMANDS"),
        key(":w :q :wq", "save / quit    :log :db  history / dashboard"),
        key(":goal N", ":year YYYY · :clearlog · :help"),
        Line::from(""),
        Line::from(Span::styled(
            "  press any key to return",
            Style::default().fg(DIM),
        )),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " taskvim — help ",
            Style::default().add_modifier(Modifier::BOLD),
        ));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ---------------------------------------------------------------- chrome

fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_input_popup(f: &mut Frame, target: &InsertTarget, buf: &str) {
    let title = match target {
        InsertTarget::DashTask => " New task  (…text @course !due) ",
        InsertTarget::CourseTask { .. } => " New task in course  (…text !due) ",
        InsertTarget::Course => " New course / project ",
        InsertTarget::Section { .. } => " New section (subtitle) ",
        InsertTarget::Due { .. } => " Due date  (YYYY-MM-DD · 7-10 · +3d · +2w · +1mo · today · none) ",
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

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(400))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.handle_key(k.code, k.modifiers);
                }
            }
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut app = App::new();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    let _ = app.data.save();
    result
}
