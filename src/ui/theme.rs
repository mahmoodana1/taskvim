//! Colour palette and small display helpers (ellipsizing, due-date badges)
//! shared across the UI modules.

use chrono::{Duration as CDuration, Local, NaiveDate};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::model::{DashSection, Repeat};

// Length must stay in sync with `model::COLOR_COUNT`.
pub const PALETTE: [Color; 6] = [
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
    Color::LightCyan,
];

pub const ACCENT: Color = Color::LightMagenta;
pub const DIM: Color = Color::DarkGray;
pub const SEL_BG: Color = Color::Rgb(45, 45, 65);

pub fn palette(idx: usize) -> Color {
    PALETTE[idx % PALETTE.len()]
}

/// Each dashboard section owns a colour, used on its box border and title.
/// Section colour is chrome only — task text keeps status colour (overdue red,
/// due-today amber, habits dim) so the two systems never fight for the same
/// glyph.
pub fn section_color(sec: DashSection) -> Color {
    match sec {
        DashSection::Important => Color::LightRed,
        DashSection::General => Color::LightBlue,
        DashSection::Agent => Color::LightMagenta,
    }
}

pub fn due_span(due: &Option<String>) -> Option<Span<'static>> {
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

/// The cadence badge shown where a due span would otherwise sit — a task can
/// have a repeat or a due date, never both.
pub fn repeat_span(repeat: &Option<Repeat>) -> Option<Span<'static>> {
    let r = repeat.as_ref()?;
    Some(Span::styled(
        format!("  ↻ {}", r.label()),
        Style::default().fg(DIM),
    ))
}

pub fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}
