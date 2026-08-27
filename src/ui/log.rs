//! The completed-history log view and the per-day completion detail view.

use chrono::Local;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{palette, ACCENT, DIM, SEL_BG};

pub fn draw_log(f: &mut Frame, app: &App, area: Rect) {
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
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
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

            let count_span = Span::styled(format!("  {:>3} done", count), Style::default().fg(DIM));

            let filled = ((*count as f64 / max_daily as f64) * bar_w as f64).round() as usize;
            let filled = filled.min(bar_w);
            let bar_color = if met {
                Color::LightGreen
            } else {
                Color::Yellow
            };
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

pub fn draw_daylog(f: &mut Frame, app: &App, area: Rect) {
    let Some(date) = &app.selected_date else {
        return;
    };
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
                spans.push(Span::styled(label, Style::default().fg(palette(e.color))));
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
