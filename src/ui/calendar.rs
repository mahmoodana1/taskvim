//! Day / Week / Month calendar, sharing TaskVim's task data.
//!
//! Styled as plain black glass: no boxes, no tinted fills, neutral greys for
//! chrome and soft pastels only on the entries themselves.

use chrono::{Datelike, Duration as CDuration, Local, NaiveDate, Timelike};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, CalendarMode};
use crate::calendar::{all_day_tasks_on, AgendaItem};
use crate::ui::theme::{ellipsize, hints, is_rtl, palette, visual, FAINT, MUTED, PALETTE, SUB, TEXT, WARM};

const NOW: Color = WARM;

/// Selection is drawn inverted (dark text on a solid fill) rather than on a
/// grey tint, which vanishes against a dark terminal background. The
/// selected day's number (week and month) is a pill like this.
fn pill() -> Style {
    Style::default().fg(Color::Black).bg(TEXT).add_modifier(Modifier::BOLD)
}

const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// A task's course as a short label in the course's own colour, as on the
/// dashboard; `None` for a loose task.
fn course_tag(app: &App, task: &crate::model::Task, max: usize) -> Option<(String, Color)> {
    app.project_of(task.project)
        .map(|p| (ellipsize(&p.name, max), palette(p.color)))
}

/// Agenda entries take successive colours by their position in the day.
fn entry_color(app: &App, index: usize) -> Color {
    PALETTE[(app.calendar_palette_base + index) % PALETTE.len()]
}

fn minute_of_day(time: &str) -> u16 {
    let mut parts = time.split(':');
    let hour = parts.next().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    let minute = parts.next().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    hour * 60 + minute
}

fn day_agenda(app: &App, date: NaiveDate) -> Vec<AgendaItem> {
    app.calendar_agenda(date)
}

/// Cut `s` to `max` chars and shape it for display. A right-to-left title
/// reads from the right, so its ellipsis goes on the left.
fn fit(s: &str, max: usize) -> String {
    let max = max.max(1);
    if s.chars().count() <= max {
        return visual(s);
    }
    let kept: String = s.chars().take(max - 1).collect();
    let kept = visual(kept.trim_end());
    let rtl = s.chars().find(|c| c.is_alphabetic()).is_some_and(is_rtl);
    if rtl {
        format!("…{}", kept)
    } else {
        format!("{}…", kept)
    }
}

/// Title (cut to `max` chars, then shaped for display) and its trailing
/// detail: a class's note (its room) stands in for the time; anything else
/// shows its time range, after its note when it has one. A repeating event
/// is marked ↻.
fn entry_text(item: &AgendaItem, max: usize) -> (String, String) {
    let glyph = match item {
        AgendaItem::Class(_) => "",
        AgendaItem::Task { .. } => "○ ",
        AgendaItem::Event(event) if event.repeat.is_some() => "↻ ",
        AgendaItem::Event(_) => "◇ ",
    };
    let time = format!("{}–{}", item.start(), item.end());
    let detail = match item {
        AgendaItem::Class(class) if class.note.is_empty() => time,
        AgendaItem::Class(class) => class.note.clone(),
        _ if item.note().is_empty() => time,
        _ => format!("{} · {}", item.note(), time),
    };
    let title = fit(item.title(), max.saturating_sub(glyph.chars().count()));
    (format!("{}{}", glyph, title), visual(&detail))
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date - CDuration::days(date.weekday().num_days_from_sunday() as i64)
}

fn rule(width: u16, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width as usize),
        Style::default().fg(color),
    ))
}

pub fn draw_calendar(f: &mut Frame, app: &App, area: Rect) {
    let area = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..area
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);

    let tab = |label: &'static str, mode: CalendarMode| {
        if app.calendar_mode == mode {
            Span::styled(
                label,
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
        } else {
            Span::styled(label, Style::default().fg(MUTED))
        }
    };
    let left = Line::from(vec![
        Span::styled("calendar", Style::default().fg(SUB)),
        Span::styled("   ", Style::default()),
        tab("day", CalendarMode::Day),
        Span::raw("  "),
        tab("week", CalendarMode::Week),
        Span::raw("  "),
        tab("month", CalendarMode::Month),
    ]);
    f.render_widget(Paragraph::new(left), rows[0]);
    let when = Line::from(vec![
        Span::styled(
            app.calendar_date.format("%B").to_string().to_lowercase(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.calendar_date.format(" %Y").to_string(),
            Style::default().fg(MUTED),
        ),
    ]);
    f.render_widget(Paragraph::new(when).right_aligned(), rows[0]);
    f.render_widget(Paragraph::new(rule(rows[1].width, FAINT)), rows[1]);

    match app.calendar_mode {
        CalendarMode::Day => draw_day(f, app, rows[3]),
        CalendarMode::Week => draw_week(f, app, rows[3]),
        CalendarMode::Month => draw_month(f, app, rows[3]),
    }
}

fn relative_day(date: NaiveDate) -> String {
    let today = Local::now().date_naive();
    match (date - today).num_days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        n if n > 0 => format!("in {} days", n),
        n => format!("{} days ago", -n),
    }
}

fn draw_day(f: &mut Frame, app: &App, area: Rect) {
    let date = app.calendar_date;
    let all_day = all_day_tasks_on(&app.data.tasks, date);
    let agenda = day_agenda(app, date);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if all_day.is_empty() { 1 } else { 3 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let classes = agenda.iter().filter(|i| matches!(i, AgendaItem::Class(_))).count();
    let others = agenda.len() - classes + all_day.len();
    let mut summary = vec![];
    if classes > 0 {
        summary.push(format!("{} class{}", classes, if classes == 1 { "" } else { "es" }));
    }
    if others > 0 {
        summary.push(format!("{} other", others));
    }
    let summary = if summary.is_empty() { "free day".to_string() } else { summary.join(" · ") };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                date.format("%A %-d").to_string().to_lowercase(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {}", relative_day(date)), Style::default().fg(SUB)),
            Span::styled(format!("   {}", summary), Style::default().fg(MUTED)),
        ])),
        rows[0],
    );

    if !all_day.is_empty() {
        let mut line = vec![Span::styled("all day   ", Style::default().fg(MUTED))];
        for (i, task) in all_day.iter().enumerate() {
            let selected = app.calendar_sel == i;
            let style = if selected {
                pill()
            } else {
                Style::default().fg(SUB)
            };
            line.push(Span::styled(if selected { "▸○ " } else { " ○ " }, style));
            if let Some((course, color)) = course_tag(app, task, 18) {
                let fg = if selected { Color::Black } else { color };
                line.push(Span::styled(format!("[{}] ", visual(&course)), style.fg(fg)));
            }
            line.push(Span::styled(format!("{} ", visual(&task.text)), style));
            line.push(Span::raw("  "));
        }
        let block = vec![Line::default(), Line::from(line)];
        f.render_widget(Paragraph::new(block), rows[1]);
    }

    draw_timeline(f, app, &agenda, all_day.len(), rows[2]);

    f.render_widget(
        Paragraph::new(hints(&[
            ("h/l", "day"),
            ("j/k", "select"),
            ("a", "task"),
            ("e", "event"),
            ("⏎/E", "edit"),
            ("s", "time"),
            ("dd/D", "delete one/all"),
            ("t", "today"),
        ])),
        rows[3],
    );
}

/// The day's timeline: an hour gutter, then each entry as a thin coloured bar
/// spanning its slots with its title and room beside the bar's top.
fn draw_timeline(f: &mut Frame, app: &App, agenda: &[AgendaItem], sel_offset: usize, area: Rect) {
    const GUTTER: usize = 8;
    // Half-hour ticks when the whole day fits, else hourly so a short
    // terminal still reaches midnight.
    let step = if area.height >= 96 { 30u16 } else { 60u16 };
    let slot_count = (24 * 60 / step) as usize;
    let spans: Vec<(usize, usize)> = agenda
        .iter()
        .map(|item| {
            let start = (minute_of_day(item.start()) / step) as usize;
            let end = ((minute_of_day(item.end()) + step - 1) / step) as usize;
            (start, end.max(start + 1).min(slot_count))
        })
        .collect();

    let graph_w = (area.width as usize).saturating_sub(GUTTER);
    let labels: Vec<(String, String)> = agenda
        .iter()
        .map(|item| entry_text(item, graph_w.saturating_sub(12).clamp(8, 48)))
        .collect();
    let label_w = |i: usize| labels[i].0.chars().count() + 2 + labels[i].1.chars().count();

    // Overlapping entries step right past each other's bar, and past the
    // label too when they start on the same row.
    let mut xs = Vec::<usize>::with_capacity(agenda.len());
    for (i, &(start, end)) in spans.iter().enumerate() {
        let mut x = 0usize;
        for (j, &(js, je)) in spans.iter().enumerate().take(i) {
            if js < end && start < je {
                let right = if js == start { xs[j] + 4 + label_w(j) } else { xs[j] + 3 };
                x = x.max(right);
            }
        }
        xs.push(x);
    }

    let now = Local::now();
    let now_slot = (app.calendar_date == now.date_naive())
        .then(|| (now.hour() as u16 * 60 + now.minute() as u16) / step)
        .map(|s| s as usize);

    // Two rows per slot. When the day doesn't fit, start an hour above the
    // first entry (or now, or 08:00 on an empty day) but never so late that
    // the last entry falls off the bottom.
    let visible = (area.height as usize / 2).max(1);
    let first_slot = if visible > slot_count {
        0
    } else {
        let anchor = spans
            .iter()
            .map(|&(s, _)| s)
            .min()
            .or(now_slot)
            .unwrap_or(8 * 60 / step as usize);
        let last_end = spans.iter().map(|&(_, e)| e).max().unwrap_or(0);
        let lead = anchor.saturating_sub(60 / step as usize);
        // Slide down only as far as needed to show the last entry's end.
        lead.max((last_end + 1).saturating_sub(visible)).min(anchor)
    };

    let mut lines: Vec<Line<'static>> = vec![];
    for slot in first_slot..=slot_count {
        for half in [false, true] {
            if half && slot == slot_count {
                continue;
            }
            let minute = (slot as u16 * step).min(24 * 60);
            let mut cells: Vec<(char, Style)> = vec![(' ', Style::default()); graph_w];
            let mut put = |x: usize, ch: char, style: Style| {
                if x < cells.len() {
                    cells[x] = (ch, style);
                }
            };
            let row_idx = slot * 2 + half as usize;
            for (i, &(start, end)) in spans.iter().enumerate() {
                if row_idx < start * 2 || row_idx >= end * 2 {
                    continue;
                }
                let color = entry_color(app, i);
                let selected = app.calendar_sel == sel_offset + i;
                // The selected entry's bar thickens along its whole span.
                put(xs[i], if selected { '█' } else { '▌' }, Style::default().fg(color));
                if row_idx == start * 2 {
                    // Selected: the label is filled with the entry's own colour.
                    let (title, detail) = if selected {
                        let fill = Style::default().fg(Color::Black).bg(color);
                        (fill.add_modifier(Modifier::BOLD), fill)
                    } else {
                        (
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                            Style::default().fg(SUB),
                        )
                    };
                    let mut x = xs[i] + 2;
                    if selected {
                        put(x - 1, ' ', detail);
                    }
                    for ch in labels[i].0.chars() {
                        put(x, ch, title);
                        x += 1;
                    }
                    put(x, ' ', detail);
                    put(x + 1, ' ', detail);
                    x += 2;
                    for ch in labels[i].1.chars() {
                        put(x, ch, detail);
                        x += 1;
                    }
                    if selected {
                        put(x, ' ', detail);
                    }
                }
            }

            let is_now = now_slot == Some(slot) && !half;
            let stamp = if half {
                " ".repeat(GUTTER)
            } else {
                let mark = if is_now { " ● " } else { "   " };
                format!("{:02}:{:02}{}", minute / 60, minute % 60, mark)
            };
            let stamp_color = if is_now {
                NOW
            } else if minute % 60 == 0 {
                SUB
            } else {
                MUTED
            };
            let mut row = vec![Span::styled(stamp, Style::default().fg(stamp_color))];
            let mut run = String::new();
            let mut run_style = Style::default();
            for (ch, style) in cells {
                if style != run_style && !run.is_empty() {
                    row.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = style;
                run.push(ch);
            }
            if !run.is_empty() {
                row.push(Span::styled(run, run_style));
            }
            lines.push(Line::from(row));
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_week(f: &mut Frame, app: &App, area: Rect) {
    let start = week_start(app.calendar_date);
    let today = Local::now().date_naive();
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
    let cols = Layout::horizontal([Constraint::Ratio(1, 7); 7])
        .spacing(2)
        .split(rows[0]);
    for (i, col) in cols.iter().enumerate() {
        let date = start + CDuration::days(i as i64);
        let selected = date == app.calendar_date;
        let is_today = date == today;
        let width = col.width as usize;

        let mut head = if selected {
            vec![Span::styled(format!(" {} {} ", DAYS[i], date.day()), pill())]
        } else {
            let color = if is_today { NOW } else { MUTED };
            let bold = if is_today { Modifier::BOLD } else { Modifier::empty() };
            vec![
                Span::styled(DAYS[i], Style::default().fg(color)),
                Span::styled(
                    format!(" {}", date.day()),
                    Style::default().fg(color).add_modifier(bold),
                ),
            ]
        };
        if is_today {
            head.push(Span::styled(" ●", Style::default().fg(NOW)));
        }
        let mut lines = vec![
            Line::from(head),
            rule(col.width, if selected { TEXT } else { FAINT }),
            Line::default(),
        ];
        // Other days step back a shade so the selected column stands out.
        let (title_fg, detail_fg) = if selected { (TEXT, SUB) } else { (SUB, MUTED) };

        let agenda = day_agenda(app, date);
        for (index, item) in agenda.iter().enumerate() {
            let color = entry_color(app, index);
            let (title, detail) = entry_text(item, width.saturating_sub(2));
            lines.push(Line::from(vec![
                Span::styled("▌ ", Style::default().fg(color)),
                Span::styled(title, Style::default().fg(title_fg)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("▌ ", Style::default().fg(color)),
                Span::styled(detail, Style::default().fg(detail_fg)),
            ]));
            lines.push(Line::default());
        }
        for task in all_day_tasks_on(&app.data.tasks, date) {
            let text = fit(&task.text, width.saturating_sub(2));
            lines.push(Line::from(vec![
                Span::styled("○ ", Style::default().fg(MUTED)),
                Span::styled(text, Style::default().fg(title_fg)),
            ]));
            if let Some((course, color)) = course_tag(app, task, width.saturating_sub(2)) {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(visual(&course), Style::default().fg(color)),
                ]));
            }
        }
        if lines.len() == 3 {
            lines.push(Line::from(Span::styled("—", Style::default().fg(FAINT))));
        }
        f.render_widget(Paragraph::new(lines), *col);
    }
    f.render_widget(
        Paragraph::new(hints(&[
            ("h/l", "day"),
            ("j/k", "week"),
            ("enter", "open day"),
            ("e", "event"),
            ("t", "today"),
            ("r", "refresh"),
        ])),
        rows[1],
    );
}

fn draw_month(f: &mut Frame, app: &App, area: Rect) {
    let today = Local::now().date_naive();
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    let heads = Layout::horizontal([Constraint::Ratio(1, 7); 7])
        .spacing(2)
        .split(rows[0]);
    for (i, cell) in heads.iter().enumerate() {
        f.render_widget(
            Paragraph::new(Span::styled(DAYS[i], Style::default().fg(MUTED))),
            *cell,
        );
    }

    let first = app.calendar_date.with_day(1).unwrap_or(app.calendar_date);
    let offset = first.weekday().num_days_from_sunday() as i64;
    let weeks = Layout::vertical([Constraint::Ratio(1, 6); 6]).split(rows[1]);
    for (week, week_area) in weeks.iter().enumerate() {
        let cols = Layout::horizontal([Constraint::Ratio(1, 7); 7])
            .spacing(2)
            .split(*week_area);
        for (day, cell) in cols.iter().enumerate() {
            let date = first + CDuration::days(week as i64 * 7 + day as i64 - offset);
            let in_month = date.month() == app.calendar_date.month();
            let selected = date == app.calendar_date;
            let is_today = date == today;
            let width = cell.width as usize;

            let mut lines = vec![rule(cell.width, if selected { TEXT } else { FAINT })];
            let num_style = if selected {
                pill()
            } else if is_today {
                Style::default().fg(NOW).add_modifier(Modifier::BOLD)
            } else if in_month {
                Style::default().fg(SUB)
            } else {
                Style::default().fg(FAINT)
            };
            let mut head = vec![Span::styled(format!(" {} ", date.day()), num_style)];
            if is_today {
                head.push(Span::styled("●", Style::default().fg(NOW)));
            }
            lines.push(Line::from(head));

            if in_month {
                let agenda = day_agenda(app, date);
                let all_day = all_day_tasks_on(&app.data.tasks, date).len();
                let room = (cell.height as usize).saturating_sub(2);
                let total = agenda.len() + all_day;
                let shown = if total > room { room.saturating_sub(1) } else { total.min(room) };
                for (index, item) in agenda.iter().enumerate().take(shown) {
                    let (title, _) = entry_text(item, width.saturating_sub(2));
                    lines.push(Line::from(vec![
                        Span::styled("▌ ", Style::default().fg(entry_color(app, index))),
                        Span::styled(title, Style::default().fg(if selected { TEXT } else { SUB })),
                    ]));
                }
                let hidden = total.saturating_sub(shown.min(agenda.len()));
                if hidden > 0 && shown < room + 1 {
                    let what = if hidden == all_day && shown >= agenda.len() {
                        format!("○ {} all day", all_day)
                    } else {
                        format!("+{} more", hidden)
                    };
                    lines.push(Line::from(Span::styled(what, Style::default().fg(MUTED))));
                }
            }
            f.render_widget(Paragraph::new(lines), *cell);
        }
    }
    f.render_widget(
        Paragraph::new(hints(&[
            ("h/l", "day"),
            ("j/k", "week"),
            ("[/]", "month"),
            ("enter", "open day"),
            ("t", "today"),
        ])),
        rows[2],
    );
}
