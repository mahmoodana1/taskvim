//! The dashboard view: task sidebar, greeting banner, and the task-row
//! rendering used by the sidebar's list.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, DashRow, Pane, View};
use crate::model::{DashSection, Task};
use crate::ui::courses::draw_courses;
use crate::ui::heatmap::{draw_daily_goal, draw_heatmap, draw_priority, draw_week};
use crate::ui::theme::{
    due_span, ellipsize, fit_span, hints, palette, panel, repeat_span, section_color, DIM, FAINT,
    GREEN, MUTED, SEL_BG, TEXT,
};

pub fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    // Panel heights, in rows: a title row, the content, and a hint row.
    const NOW: u16 = 3;
    const GOAL: u16 = 3;
    const HEATMAP: u16 = 11; // month row + 7 days + totals + title + hints
    const WEEK: u16 = 6; //     3 bar rows + labels + totals + title
    // One blank row between stacked panels so each reads as its own block.
    const GAP: u16 = 1;
    // The heatmap is all-or-nothing: without a month row and all seven day
    // rows it means nothing; only its totals line can go.
    const HEATMAP_MIN: u16 = 10;

    // Same breathing room at the edges as the calendar.
    let area = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..area
    };

    let courses_h = (app.visible_courses().len() as u16 + 2).clamp(3, 12);

    let week_h = if area.height >= 24 { WEEK + GAP } else { 0 };
    let fixed = NOW + GAP + GOAL + GAP + courses_h;
    let room = area
        .height
        .saturating_sub(week_h)
        .saturating_sub(fixed + GAP);
    let heat_h = if room >= HEATMAP_MIN { room.min(HEATMAP) } else { 0 };
    let heat_gap = if heat_h > 0 { GAP } else { 0 };

    // Size the main region to whichever column actually needs more, so the
    // bottom band follows the content instead of being pinned to the screen
    // edge with a hole above it.
    let sidebar_need = app.dash_rows().len() as u16 + 2; // list + title + footer
    let rail_need = fixed + heat_h + heat_gap;
    let main_h = sidebar_need
        .max(rail_need)
        .min(area.height.saturating_sub(week_h));

    let outer = Layout::vertical([
        Constraint::Length(main_h),
        Constraint::Length(week_h),
        Constraint::Min(0), // slack, below everything
    ])
    .split(area);

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .spacing(3)
        .split(outer[0]);

    // Give the sidebar only the rows it needs, so its footer hints sit under
    // the last section instead of floating at the bottom of a stretched column.
    let sidebar = Layout::vertical([
        Constraint::Length(sidebar_need.min(main_h)),
        Constraint::Min(0),
    ])
    .split(cols[0]);
    draw_task_sidebar(f, app, sidebar[0]);

    let rail = Layout::vertical([
        Constraint::Length(NOW),
        Constraint::Length(GAP),
        Constraint::Length(GOAL),
        Constraint::Length(GAP),
        Constraint::Length(heat_h),
        Constraint::Length(heat_gap),
        Constraint::Length(courses_h),
        Constraint::Min(0),
    ])
    .split(cols[1]);

    draw_priority(f, app, rail[0]);
    draw_daily_goal(f, app, rail[2]);
    if heat_h > 0 {
        draw_heatmap(f, app, rail[4]);
    }
    draw_courses(f, app, rail[6]);

    if week_h > 0 {
        let week = Rect {
            y: outer[1].y + GAP,
            height: outer[1].height - GAP,
            ..outer[1]
        };
        draw_week(f, app, week);
    }
}

/// The content spans for one dashboard task: project tag, title, and the
/// trailing badge (cadence or due date — a task never has both). The caller
/// positions and pads these; `task_spans` does not decide layout.
pub fn task_spans(app: &App, t: &Task) -> (Vec<Span<'static>>, Option<Span<'static>>) {
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

    // A task has a repeat or a due date, never both.
    let badge = if t.completed {
        None
    } else {
        repeat_span(&t.repeat).or_else(|| due_span(&t.due))
    };
    (spans, badge)
}

/// `important                     2` — the coloured name, count flush right.
/// Alignment rather than a rule carries the structure.
fn section_head(sec: DashSection, count: usize, width: usize, color: Color) -> ListItem<'static> {
    let title = sec.title().to_lowercase();
    let tail = format!("{} ", count);
    let pad = width.saturating_sub(title.chars().count() + tail.chars().count());
    ListItem::new(Line::from(vec![
        Span::styled(title, Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::raw(" ".repeat(pad)),
        Span::styled(tail, Style::default().fg(MUTED)),
    ]))
}

/// An empty section's single placeholder row.
fn section_fill() -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled("  nothing here", Style::default().fg(FAINT))))
}

/// `  buy groceries          ↻ daily ` — content left, badge flush right.
/// All measurements use ratatui's Unicode display width, never byte or char
/// counts, because the badges carry wide glyphs (`⚠`, `↻`).
fn task_row(app: &App, t: &Task, width: usize) -> ListItem<'static> {
    /// Columns the title keeps even when the course tag would rather have them.
    const MIN_TITLE: usize = 10;
    const INDENT: usize = 2;

    let (mut content, badge) = task_spans(app, t);
    // Drop the badge outright rather than truncate it when the column is too
    // narrow to hold both — a task's name matters more than its date.
    let badge = badge.filter(|b| b.width() + MIN_TITLE + INDENT + 1 <= width);
    let badge_w = badge.as_ref().map(|b| b.width()).unwrap_or(0);
    let avail = width.saturating_sub(INDENT + badge_w + 1);

    // The title is always the last span. Budget it first, so a long
    // "[Course - Section]" tag can never squeeze the title off the row.
    let title = content.pop().unwrap_or_else(|| Span::raw(""));
    let tag_w: usize = content.iter().map(|s| s.width()).sum();
    let title_budget = avail.saturating_sub(tag_w).max(MIN_TITLE.min(avail));
    let tag_budget = avail.saturating_sub(title_budget);

    let mut line: Vec<Span<'static>> = vec![Span::raw(" ".repeat(INDENT))];
    let mut used = 0usize;
    for s in content {
        let fitted = fit_span(s, tag_budget.saturating_sub(used));
        used += fitted.width();
        if fitted.width() > 0 {
            line.push(fitted);
        }
    }
    let fitted_title = fit_span(title, avail.saturating_sub(used));
    used += fitted_title.width();
    line.push(fitted_title);
    line.push(Span::raw(" ".repeat(width.saturating_sub(INDENT + used + badge_w + 1))));
    if let Some(b) = badge {
        line.push(b);
    }
    line.push(Span::raw(" "));
    ListItem::new(Line::from(line))
}

pub fn draw_task_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Tasks && app.view == View::Dashboard;
    let done_today = app.dash_completed_today();
    // 1/2/3, t and dd all act on the task under the cursor — hide them when
    // the cursor is sitting on a section header instead.
    let on_task = !active || app.selected_task().is_some();
    let mut keys: Vec<(&str, &str)> = vec![("a", "add")];
    if on_task {
        keys.extend([("⏎", "done"), ("1/2/3", "section"), ("t", "due"), ("dd", "unlink")]);
    }
    if done_today == 0 || !on_task {
        keys.push(("r", "random"));
    }
    let mut footer = hints(&keys);
    if done_today > 0 {
        footer.spans.push(Span::styled(
            format!("   {} done today", done_today),
            Style::default().fg(GREEN),
        ));
    }

    let count = app.dash_ids().len().to_string();
    let inner = panel(f, area, "tasks", &count, Some(footer), active);

    let width = inner.width as usize;
    let rows = app.dash_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            DashRow::SectionTop(sec) => {
                let count = app
                    .dash_ids()
                    .iter()
                    .filter(|id| {
                        app.task(**id)
                            .map(|t| t.dash_section == *sec)
                            .unwrap_or(false)
                    })
                    .count();
                section_head(*sec, count, width, section_color(*sec))
            }
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => task_row(app, t, width),
                None => ListItem::new(Line::from("")),
            },
            DashRow::SectionFill(_) => section_fill(),
            DashRow::SectionBottom(_) => ListItem::new(Line::from("")),
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
        f.render_widget(Paragraph::new(msg).style(Style::default().fg(MUTED)), inner);
        return;
    }

    let list = List::new(items).highlight_style(
        Style::default().bg(SEL_BG).fg(TEXT).add_modifier(Modifier::BOLD),
    );
    let mut state = ListState::default();
    if active {
        state.select(Some(app.task_sel.min(rows.len().saturating_sub(1))));
    }
    f.render_stateful_widget(list, inner, &mut state);
}
