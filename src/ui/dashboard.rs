//! The dashboard view: task sidebar, greeting banner, and the task-row
//! rendering used by the sidebar's list.

use chrono::{Local, Timelike};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, DashRow, Pane, View};
use crate::model::{DashSection, Task};
use crate::ui::courses::draw_courses;
use crate::ui::heatmap::{draw_daily_goal, draw_heatmap, draw_priority};
use crate::ui::theme::{
    due_span, ellipsize, palette, repeat_span, section_color, ACCENT, DIM, SEL_BG,
};

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

/// Truncate a span to at most `max` display columns, appending `…` if cut.
/// Measures width per character while building, so a wide glyph can never push
/// the result past `max` — truncating by char count and measuring afterwards
/// silently overshoots and misaligns the box.
fn fit_span(s: Span<'static>, max: usize) -> Span<'static> {
    if s.width() <= max {
        return s;
    }
    if max == 0 {
        return Span::styled(String::new(), s.style);
    }
    let budget = max - 1; // `…` is one column wide
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.content.chars() {
        let cw = Span::raw(ch.to_string()).width();
        if w + cw > budget {
            break;
        }
        w += cw;
        out.push(ch);
    }
    out.push('…');
    Span::styled(out, s.style)
}

/// `┌ IMPORTANT ─────────── 2 ┐`
///
/// Always renders exactly `width` columns: the title yields to the count, and
/// the count yields to the corners, so a narrow column truncates rather than
/// overflowing past the panel edge.
fn box_top(sec: DashSection, count: usize, width: usize, color: Color) -> ListItem<'static> {
    if width < 2 {
        return ListItem::new(Line::from(Span::styled(
            "─".repeat(width),
            Style::default().fg(color),
        )));
    }
    let inner = width - 2; // between the two corners

    let tail_full = format!(" {} ", count);
    let tail: String = tail_full.chars().take(inner).collect();
    let tail_w = tail.chars().count();

    // Leave one rule character between title and count when there's room.
    let title_room = inner.saturating_sub(tail_w + 1);
    let title_full = format!(" {} ", sec.title().to_uppercase());
    let title: String = if title_full.chars().count() <= title_room {
        title_full
    } else {
        title_full.chars().take(title_room).collect()
    };
    let rule = inner.saturating_sub(title.chars().count() + tail_w);

    ListItem::new(Line::from(vec![
        Span::styled(
            format!("┌{}", title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(rule), Style::default().fg(color)),
        Span::styled(tail, Style::default().fg(DIM)),
        Span::styled("┐", Style::default().fg(color)),
    ]))
}

/// `└─────────────────────────┘`
fn box_bottom(width: usize, color: Color) -> ListItem<'static> {
    ListItem::new(Line::from(Span::styled(
        format!("└{}┘", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(color),
    )))
}

/// `│                         │` — an empty section's interior.
fn box_fill(width: usize, color: Color) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled("│", Style::default().fg(color)),
        Span::raw(" ".repeat(width.saturating_sub(2))),
        Span::styled("│", Style::default().fg(color)),
    ]))
}

/// `│ buy groceries    ↻ daily │` — content left, badge right, padded so the
/// closing bar always lands in the same column. All measurements use ratatui's
/// Unicode display width, never byte or char counts, because the badges carry
/// wide glyphs (`⚠`, `↻`).
fn box_task(app: &App, t: &Task, width: usize, color: Color) -> ListItem<'static> {
    /// Columns the title keeps even when the course tag would rather have them.
    const MIN_TITLE: usize = 10;

    let inner = width.saturating_sub(2);
    let (mut content, badge) = task_spans(app, t);
    // Drop the badge outright rather than truncate it when the column is too
    // narrow to hold both — a task's name matters more than its date, and a
    // half-rendered "⚠ was due Aug…" is worse than none.
    let badge = badge.filter(|b| b.width() + MIN_TITLE + 2 <= inner);
    let badge_w = badge.as_ref().map(|b| b.width()).unwrap_or(0);
    // 1 leading space + content + pad + badge + 1 trailing space
    let avail = inner.saturating_sub(badge_w + 2);

    // The title is always the last span. Budget it first, so a long
    // "[Course - Section]" tag can never squeeze the title off the row.
    let title = content.pop().unwrap_or_else(|| Span::raw(""));
    let tag_w: usize = content.iter().map(|s| s.width()).sum();
    let title_budget = avail.saturating_sub(tag_w).max(MIN_TITLE.min(avail));
    let tag_budget = avail.saturating_sub(title_budget);

    let mut line: Vec<Span<'static>> = vec![
        Span::styled("│", Style::default().fg(color)),
        Span::raw(" "),
    ];
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

    // `used <= avail` by construction, so this padding is always exact and the
    // closing bar always lands in the same column.
    line.push(Span::raw(" ".repeat(inner.saturating_sub(used + badge_w + 2))));
    if let Some(b) = badge {
        line.push(b);
    }
    line.push(Span::raw(" "));
    line.push(Span::styled("│", Style::default().fg(color)));
    ListItem::new(Line::from(line))
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
    // Borderless: the section boxes ARE the panel. The block is kept only for
    // its title and footer hints — nesting a second border around the boxes
    // wastes four columns and reads as heavy.
    let block = Block::default()
        .borders(Borders::NONE)
        .title(Span::styled(
            format!(" tasks ({}) ", app.dash_ids().len()),
            Style::default().fg(border).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(footer, Style::default().fg(DIM))));

    let width = area.width as usize;
    let rows = app.dash_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            DashRow::SectionTop(sec) => {
                let count = app
                    .dash_ids()
                    .iter()
                    .filter(|id| app.task(**id).map(|t| t.dash_section == *sec).unwrap_or(false))
                    .count();
                box_top(*sec, count, width, section_color(*sec))
            }
            DashRow::Task(id) => match app.task(*id) {
                Some(t) => box_task(app, t, width, section_color(t.dash_section)),
                None => ListItem::new(Line::from("")),
            },
            DashRow::SectionFill(sec) => box_fill(width, section_color(*sec)),
            DashRow::SectionBottom(sec) => box_bottom(width, section_color(*sec)),
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
