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
    // Same vocabulary as the dashboard's section boxes and the course modal's
    // section rules: an uppercase title opening a rule.
    let full = area.width.saturating_sub(2) as usize;
    let hdr = |t: &'static str| {
        let rule = full.saturating_sub(t.chars().count() + 6);
        Line::from(vec![
            Span::styled(
                format!(" ── {} ", t),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(rule), Style::default().fg(DIM)),
        ])
    };
    let lines = vec![
        hdr("MOTIONS (everywhere)"),
        key("j k gg G", "move · jump to top / bottom"),
        key("h l Tab", "switch pane · Tab in a course = next course"),
        key("J / K", "reorder task / section"),
        Line::from(""),
        hdr("DASHBOARD  (sections: Important · General · Agent)"),
        key("a", "add task into the section at the cursor"),
        key("1 2 3", "move task to Important / General / Agent"),
        key("~", "set how often this task repeats"),
        key("⏎ / Space", "complete selected task · on a course: OPEN it"),
        key(
            "c",
            "make selected task the CURRENT PRIORITY (top of Important)",
        ),
        key(
            "dd",
            "unlink course task from dashboard (D deletes for real)",
        ),
        key("r", "pull a random backlog task → current priority"),
        key("t / e", "set due date · rename"),
        key("+ -  [ ]", "daily goal · heatmap year"),
        key("o", "open history log"),
        key(
            "u / Ctrl-R",
            "undo · redo the last action (deletion, completion, add…)",
        ),
        Line::from(""),
        hdr("INSIDE A COURSE (⏎ on a course card)"),
        key("S", "add a section (subtitle): Lectures, Labs, …"),
        key("a", "add task under the section at the cursor"),
        key("p", "link/unlink task ↗ the main dashboard"),
        key("c", "make task the CURRENT PRIORITY (link → dashboard top)"),
        key("m / M", "move task to next / previous section"),
        key(
            "t / e / R",
            "due date · rename task or section · rename course",
        ),
        key("⏎ / dd", "complete · delete (section keeps its tasks)"),
        Line::from(""),
        hdr("ADD LINE  ('a') — @course !due #section ~repeat, in any order"),
        key(
            "example",
            "lab report @algo !+3 #important  ·  water plants ~3d #agent",
        ),
        Line::from(""),
        hdr("PICKERS  —  t: due date  ·  ~: repeat  (both: c custom, Esc cancel)"),
        key(
            "t  picker",
            "1 today · 2 tomorrow · 3 +3d · 4 +1w · 5 +2w · 6 +1mo · 0 clear",
        ),
        key(
            "t  formats",
            "2026-07-10 · 7-10 · +3d · +2w · +1mo · today · tomorrow · none",
        ),
        key(
            "~  picker",
            "1 daily · 2 every 3d · 3 weekly · 4 every 2w · 5 monthly · 0 off",
        ),
        key(
            "~  formats",
            "3d · 2w · 1mo · daily · weekly · monthly · none",
        ),
        key(
            "habits",
            "no due date, never overdue; hide once done, return next period",
        ),
        Line::from(""),
        hdr("EX COMMANDS"),
        key(
            ":w :q :wq",
            "save / quit · :log :db history / dashboard · :goal N",
        ),
        key("", ":year YYYY · :clearlog · :help"),
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
