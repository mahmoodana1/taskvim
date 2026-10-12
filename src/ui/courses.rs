//! Courses & projects list, and the course detail modal (sections + tasks).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, Pane, Row, View};
use crate::ui::theme::{
    due_span, fit_span, hints, palette, panel, repeat_span, DIM, FAINT, PALETTE, ROSE, SEL_BG, TEXT,
};

pub fn draw_courses(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Courses && app.view == View::Dashboard;

    let courses = app.visible_courses();
    let inner_h = area.height.saturating_sub(2) as usize;
    let max_visible = inner_h; // one row per course
                                   // "3 of 7 ▾" reads as "there is more below"; a bare count does not.
    let extra = if courses.len() > max_visible {
        format!("{} of {} ▾", max_visible, courses.len())
    } else {
        courses.len().to_string()
    };
    let hint = hints(&[
        ("⏎", "open"),
        ("a", "new"),
        ("x", "archive"),
        ("X", "archived"),
        ("e", "rename"),
        ("dd", "delete"),
    ]);
    let inner = panel(f, area, "courses", &extra, Some(hint), active);

    if app.visible_courses().is_empty() {
        f.render_widget(
            // No leading newline: the panel is sized to its content, so a
            // blank first line would push the message out of view entirely.
            Paragraph::new("no courses yet — press a to create one").style(Style::default().fg(DIM)),
            inner,
        );
        return;
    }

    // Columns: bar + name, progress line, done/total, overdue. Every row
    // shares the same column edges so the list reads as a table.
    const BAR: usize = 12;
    const COUNT: usize = 7; // "100/100"
    const OVERDUE: usize = 5; // "  ⚠ 9"
    let width = inner.width as usize;
    let name_w = width.saturating_sub(2 + 2 + BAR + 2 + COUNT + OVERDUE + 1).max(6);

    let items: Vec<ListItem> = courses
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
            let filled = (pct * BAR as f64).round() as usize;
            let name = fit_span(
                Span::styled(p.name.clone(), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
                name_w,
            );
            let name_pad = name_w.saturating_sub(name.width());
            let mut spans = vec![
                Span::styled("▌ ", Style::default().fg(color)),
                name,
                Span::raw(" ".repeat(name_pad + 2)),
                Span::styled("━".repeat(filled), Style::default().fg(color)),
                Span::styled("─".repeat(BAR - filled), Style::default().fg(FAINT)),
                Span::styled(
                    format!("  {:>width$}", format!("{}/{}", done, total), width = COUNT),
                    Style::default().fg(DIM),
                ),
            ];
            if overdue > 0 {
                spans.push(Span::styled(
                    format!("{:>width$}", format!("⚠ {}", overdue), width = OVERDUE),
                    Style::default().fg(ROSE),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));

    let mut state = ListState::default();
    if active {
        state.select(Some(
            app.course_sel
                .min(app.visible_courses().len().saturating_sub(1)),
        ));
    }
    f.render_stateful_widget(list, inner, &mut state);
}

/// A floating modal's frame: one rounded, faint outline so it reads as glass
/// lifted over the dashboard rather than another box in it.
fn modal_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(FAINT))
        .padding(Padding::horizontal(1))
}

/// Hint line for a modal's bottom edge, spaced off the rounded corners.
fn modal_hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut line = hints(pairs);
    line.spans.insert(0, Span::raw(" "));
    line.spans.push(Span::raw(" "));
    line
}

/// `── LECTURES ──────────────────────── 2`
///
/// The modal already has a border, so its sections use a rule rather than a
/// second box — nesting borders is what the dashboard deliberately avoids.
/// Same vocabulary though: uppercase title, rule, trailing count.
fn section_rule(name: &str, count: usize, width: usize, color: Color) -> ListItem<'static> {
    let title = format!(" {} ", name.to_lowercase());
    let tail = format!(" {} ", count);
    let lead = 2usize; // the "──" that opens the rule
    let rule = width.saturating_sub(lead + title.chars().count() + tail.chars().count());
    ListItem::new(Line::from(vec![
        Span::styled("──", Style::default().fg(FAINT)),
        Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(rule), Style::default().fg(FAINT)),
        Span::styled(tail, Style::default().fg(DIM)),
    ]))
}

/// `  ↗ Lecture 8                ⚠ was due Aug 20` — badge flush right, title
/// truncated to fit. Mirrors the dashboard's task rows without the side bars.
fn task_row(
    marker: Span<'static>,
    title: Span<'static>,
    badge: Option<Span<'static>>,
    width: usize,
) -> ListItem<'static> {
    let indent = 2usize;
    let badge_w = badge.as_ref().map(|b| b.width()).unwrap_or(0);
    let avail = width.saturating_sub(indent + marker.width() + badge_w);
    let title = fit_span(title, avail);
    let pad = width.saturating_sub(indent + marker.width() + title.width() + badge_w);

    let mut spans = vec![Span::raw(" ".repeat(indent)), marker, title];
    spans.push(Span::raw(" ".repeat(pad)));
    if let Some(b) = badge {
        spans.push(b);
    }
    ListItem::new(Line::from(spans))
}

pub fn draw_course_modal(f: &mut Frame, app: &App, area: Rect) {
    let Some(pid) = app.cur_course else { return };
    let Some(p) = app.project(pid) else { return };
    let color = palette(p.color);
    let (done, total) = app.project_stats(pid);

    let rows = app.course_rows(pid);

    let w = area.width.saturating_sub(8).min(90).max(40);
    // Size to content rather than always filling the screen: 2 border rows plus
    // one per row, capped by the space available.
    let content_h = rows.len() as u16 + 2;
    let h = content_h.clamp(6, area.height.saturating_sub(4).min(32).max(6));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let block = modal_block()
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", p.name),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{}/{} done ", done, total), Style::default().fg(DIM)),
        ]))
        .title_bottom(
            modal_hints(&[
                ("a", "task"),
                ("S", "section"),
                ("⏎", "done"),
                ("p", "link"),
                ("t", "due"),
                ("e", "rename"),
                ("m", "move"),
                ("J/K", "reorder"),
                ("dd", "del"),
                ("q", "close"),
            ]),
        );

    // 2 borders + 2 padding columns + a 1-column right margin so badges and
    // counts do not butt against the frame.
    let inner_w = popup.width.saturating_sub(5) as usize;

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
                section_rule(&name, n, inner_w, color)
            }
            Row::NoSection => {
                let n = app
                    .data
                    .tasks
                    .iter()
                    .filter(|t| t.project == Some(pid) && t.section.is_none())
                    .count();
                section_rule("unsectioned", n, inner_w, DIM)
            }
            Row::Task(tid) => {
                let Some(t) = app.task(*tid) else {
                    return ListItem::new(Line::from(""));
                };
                let style = if t.completed {
                    Style::default().fg(DIM).add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                let marker = if app.is_pinned(*tid) {
                    Span::styled("↗ ", Style::default().fg(PALETTE[2]))
                } else {
                    Span::styled("○ ", style)
                };
                let text = if t.completed {
                    format!("✓ {}", t.text)
                } else {
                    t.text.clone()
                };
                // A task has a repeat or a due date, never both.
                let badge = if t.completed {
                    None
                } else {
                    repeat_span(&t.repeat).or_else(|| due_span(&t.due))
                };
                task_row(marker, Span::styled(text, style), badge, inner_w)
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

/// The archive: a modal listing archived courses, opened with `X`.
pub fn draw_archived_modal(f: &mut Frame, app: &App, area: Rect) {
    let courses = app.archived_courses();

    let w = area.width.saturating_sub(8).min(70).max(36);
    // 3 = border + one row + border, which also fits the empty-state message.
    let h = (courses.len() as u16 + 2).clamp(3, area.height.saturating_sub(4).max(3));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);

    let block = modal_block()
        .title(Line::from(vec![
            Span::styled(" archived ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(
                    "{} course{} ",
                    courses.len(),
                    if courses.len() == 1 { "" } else { "s" }
                ),
                Style::default().fg(DIM),
            ),
        ]))
        .title_bottom(modal_hints(&[("x", "restore"), ("⏎", "open"), ("q", "close")]));

    if courses.is_empty() {
        f.render_widget(
            Paragraph::new(" nothing archived — press x on a course to archive it")
                .style(Style::default().fg(DIM))
                .block(block),
            popup,
        );
        return;
    }

    let items: Vec<ListItem> = courses
        .iter()
        .map(|p| {
            let (done, total) = app.project_stats(p.id);
            ListItem::new(Line::from(vec![
                Span::styled("  ○ ", Style::default().fg(DIM)),
                Span::styled(
                    p.name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("   {}/{} done", done, total),
                    Style::default().fg(DIM),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(SEL_BG).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.archived_sel.min(courses.len().saturating_sub(1))));
    f.render_stateful_widget(list, popup, &mut state);
}
