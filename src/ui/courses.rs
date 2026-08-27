//! Courses & projects list, and the course detail modal (sections + tasks).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, Pane, Row, View};
use crate::ui::theme::{due_span, palette, repeat_span, ACCENT, DIM, SEL_BG};

pub fn draw_courses(f: &mut Frame, app: &App, area: Rect) {
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

pub fn draw_course_modal(f: &mut Frame, app: &App, area: Rect) {
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
                    // A task has a repeat or a due date, never both.
                    let badge = repeat_span(&t.repeat).or_else(|| due_span(&t.due));
                    if let Some(s) = badge {
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
