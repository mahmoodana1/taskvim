//! The static keybinding help screen.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::ui::theme::{ACCENT, DIM};

pub fn draw_help(f: &mut Frame, area: Rect) {
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
