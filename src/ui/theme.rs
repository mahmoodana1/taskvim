//! Colour palette and small display helpers (ellipsizing, due-date badges)
//! shared across the UI modules.

use chrono::{Duration as CDuration, Local, NaiveDate};
use ratatui::style::{Color, Modifier, Style};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::model::{DashSection, Repeat};

// Plain black glass: neutral greys with no blue or purple cast for chrome,
// soft pastels only for content. Shared by every view.
pub const TEXT: Color = Color::Rgb(222, 222, 222);
pub const SUB: Color = Color::Rgb(150, 150, 150);
pub const MUTED: Color = Color::Rgb(96, 96, 96);
pub const FAINT: Color = Color::Rgb(44, 44, 44);
pub const WARM: Color = Color::Rgb(250, 179, 135);
pub const ROSE: Color = Color::Rgb(235, 160, 172);
pub const SAND: Color = Color::Rgb(249, 226, 175);
pub const GREEN: Color = Color::Rgb(166, 227, 161);

// Ordered so warm and cool alternate: neighbouring entries always land on
// visibly different hues. Length must stay in sync with `model::COLOR_COUNT`.
pub const PALETTE: [Color; 6] = [
    Color::Rgb(137, 180, 250), // blue
    WARM,                      // peach
    Color::Rgb(148, 226, 213), // teal
    ROSE,                      // rose
    GREEN,                     // green
    SAND,                      // sand
];

pub const ACCENT: Color = TEXT;
pub const DIM: Color = MUTED;
pub const SEL_BG: Color = Color::Rgb(40, 40, 40);

/// `key what   key what` — keys in grey, their meaning one step fainter.
pub fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![];
    for (i, (key, what)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), Style::default().fg(SUB)));
        spans.push(Span::styled(format!(" {}", what), Style::default().fg(MUTED)));
    }
    Line::from(spans)
}

/// A borderless panel: `title  extra ──────` on the top row, optional hints on
/// the bottom row. Takes the same two rows a bordered block would, so panel
/// heights stay as they were. Returns the content area between them.
pub fn panel(
    f: &mut Frame,
    area: Rect,
    title: &str,
    extra: &str,
    hint: Option<Line<'static>>,
    active: bool,
) -> Rect {
    if area.height == 0 {
        return area;
    }
    let (title_style, rule_color) = if active {
        (Style::default().fg(TEXT).add_modifier(Modifier::BOLD), SUB)
    } else {
        (Style::default().fg(SUB), FAINT)
    };
    let used = title.chars().count() + if extra.is_empty() { 0 } else { extra.chars().count() + 2 } + 1;
    let mut head = vec![Span::styled(title.to_string(), title_style)];
    if !extra.is_empty() {
        head.push(Span::styled(format!("  {}", extra), Style::default().fg(MUTED)));
    }
    head.push(Span::raw(" "));
    head.push(Span::styled(
        "─".repeat((area.width as usize).saturating_sub(used)),
        Style::default().fg(rule_color),
    ));
    f.render_widget(Paragraph::new(Line::from(head)), Rect { height: 1, ..area });

    let mut inner = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    if let Some(hint) = hint {
        if inner.height >= 2 {
            inner.height -= 1;
            let row = Rect {
                y: inner.y + inner.height,
                height: 1,
                ..inner
            };
            f.render_widget(Paragraph::new(hint), row);
        }
    }
    inner
}

pub fn palette(idx: usize) -> Color {
    PALETTE[idx % PALETTE.len()]
}

/// Each dashboard section owns a colour, used on its box border and title.
/// Section colour is chrome only — task text keeps status colour (overdue red,
/// due-today amber, habits dim) so the two systems never fight for the same
/// glyph.
pub fn section_color(sec: DashSection) -> Color {
    match sec {
        DashSection::Important => ROSE,
        DashSection::General => PALETTE[0],
        DashSection::Agent => PALETTE[2],
    }
}

pub fn due_span(due: &Option<String>) -> Option<Span<'static>> {
    let d = NaiveDate::parse_from_str(due.as_deref()?, "%Y-%m-%d").ok()?;
    let today = Local::now().date_naive();
    let (txt, style) = if d < today {
        (
            format!("  ⚠ was due {}", d.format("%b %d")),
            Style::default().fg(ROSE),
        )
    } else if d == today {
        (String::from("  ● due today"), Style::default().fg(WARM))
    } else if d == today + CDuration::days(1) {
        (String::from("  due tomorrow"), Style::default().fg(SAND))
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

/// Truncate a span to at most `max` display columns, appending `…` if cut.
/// Measures width per character while building, so a wide glyph can never push
/// the result past `max` — truncating by char count and measuring afterwards
/// silently overshoots and misaligns the box.
pub fn fit_span(s: Span<'static>, max: usize) -> Span<'static> {
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

pub fn is_rtl(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// Terminals draw one cell per char in logical order and do no bidi, so
/// Hebrew/Arabic would come out mirrored. Reverse each right-to-left run
/// (neutrals between two RTL chars included) so it reads correctly, and leave
/// Latin text, digits and times untouched (left-to-right).
pub fn visual(s: &str) -> String {
    if !s.chars().any(is_rtl) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if !is_rtl(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        let mut j = i + 1;
        while j < chars.len() {
            if is_rtl(chars[j]) {
                end = j + 1;
            } else if !(chars[j].is_whitespace() || matches!(chars[j], '-' | '\'' | '"' | '.' | ',' | '/')) {
                break;
            }
            j += 1;
        }
        out.extend(chars[i..end].iter().rev());
        i = end;
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod visual_tests {
    use super::visual;

    #[test]
    fn latin_untouched() {
        assert_eq!(visual("CS101 09:00-10:30"), "CS101 09:00-10:30");
    }

    #[test]
    fn hebrew_run_reversed_digits_kept() {
        assert_eq!(visual("שלום עולם 10:00"), "םלוע םולש 10:00");
        assert_eq!(visual("Math מתמטיקה"), "Math הקיטמתמ");
    }
}
