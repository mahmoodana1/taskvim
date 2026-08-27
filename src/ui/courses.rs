//! Courses & projects list, and the course detail modal (sections + tasks).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, Pane, Row, View};
use crate::ui::theme::{due_span, fit_span, palette, repeat_span, ACCENT, DIM, SEL_BG};

pub fn draw_courses(f: &mut Frame, app: &App, area: Rect) {
    let active = app.pane == Pane::Courses && app.view == View::Dashboard;

    let n_projects = app.data.projects.len();
    let inner_h = area.height.saturating_sub(2) as usize;
    let max_visible = inner_h / 2;
    let hidden = n_projects.saturating_sub(max_visible);

    let title = if hidden > 0 {
        format!(" COURSES  (+{} hidden) ", hidden)
    } else {
        String::from(" COURSES ")
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

    if app.visible_courses().is_empty() {
        f.render_widget(
            // No leading newline: the panel is sized to its content, so a
            // blank first line would push the message out of view entirely.
            Paragraph::new(" no courses yet — press a to create one")
                .style(Style::default().fg(DIM))
                .block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .visible_courses()
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
                Span::styled(format!("{}/{}", done, total), Style::default().fg(DIM)),
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
        state.select(Some(
            app.course_sel
                .min(app.visible_courses().len().saturating_sub(1)),
        ));
    }
    f.render_stateful_widget(list, area, &mut state);
}

/// `── LECTURES ──────────────────────── 2`
///
/// The modal already has a border, so its sections use a rule rather than a
/// second box — nesting borders is what the dashboard deliberately avoids.
/// Same vocabulary though: uppercase title, rule, trailing count.
fn section_rule(name: &str, count: usize, width: usize, color: Color) -> ListItem<'static> {
    let title = format!(" {} ", name.to_uppercase());
    let tail = format!(" {} ", count);
    let lead = 2usize; // the "──" that opens the rule
    let rule = width.saturating_sub(lead + title.chars().count() + tail.chars().count());
    ListItem::new(Line::from(vec![
        Span::styled("──", Style::default().fg(color)),
        Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(rule), Style::default().fg(color)),
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

    // 2 borders + a 1-column right margin so badges and counts do not butt
    // against the frame.
    let inner_w = popup.width.saturating_sub(3) as usize;

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
                    Span::styled("↗ ", Style::default().fg(Color::LightCyan))
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
