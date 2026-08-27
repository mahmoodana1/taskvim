//! Add-line parsing: `@course`, `!due` and `#section` suffixes. Pure — no
//! ratatui, no App — so it can be unit-tested directly.

use crate::due::parse_due;
use crate::model::{DashSection, Project};

/// Everything an add-line can specify. Fields left `None` mean "not stated" —
/// the caller decides the default (e.g. the section under the cursor).
#[derive(Debug, Default, PartialEq)]
pub struct ParsedTask {
    pub text: String,
    pub project: Option<u64>,
    pub due: Option<String>,
    pub section: Option<DashSection>,
}

fn match_section(name: &str) -> Option<DashSection> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    DashSection::ALL
        .into_iter()
        .find(|s| s.title().to_lowercase().starts_with(&n))
}

/// Strip trailing `!due`, `@course` and `#section` tokens in any order.
/// A token that doesn't resolve — an unknown course, an unparseable date, a
/// `#4` that isn't a section — is left in the text as literal content.
pub fn parse_task_input(projects: &[Project], buf: &str) -> ParsedTask {
    let mut text = buf.trim().to_string();
    let mut project = None;
    let mut due = None;
    let mut section = None;

    // One pass per possible token, plus one to settle.
    for _ in 0..4 {
        let before = text.clone();

        if due.is_none() {
            if let Some(tok) = text.split_whitespace().last() {
                if let Some(spec) = tok.strip_prefix('!') {
                    if let Ok(d) = parse_due(spec) {
                        due = d;
                        let cut = text.rfind('!').unwrap();
                        text = text[..cut].trim().to_string();
                    }
                }
            }
        }

        if section.is_none() {
            if let Some(tok) = text.split_whitespace().last() {
                if let Some(name) = tok.strip_prefix('#') {
                    if let Some(s) = match_section(name) {
                        section = Some(s);
                        let cut = text.rfind('#').unwrap();
                        text = text[..cut].trim().to_string();
                    }
                }
            }
        }

        if project.is_none() {
            if let Some(pos) = text.rfind('@') {
                let name = text[pos + 1..].trim().to_lowercase();
                if !name.is_empty() && pos > 0 {
                    if let Some(p) = projects
                        .iter()
                        .find(|p| p.name.to_lowercase().starts_with(&name))
                    {
                        project = Some(p.id);
                        text = text[..pos].trim().to_string();
                    }
                }
            }
        }

        if text == before {
            break;
        }
    }

    ParsedTask { text, project, due, section }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DashSection, Project};

    fn projects() -> Vec<Project> {
        vec![
            Project { id: 7, name: "Algorithms 2".into(), color: 0 },
            Project { id: 8, name: "Statistics".into(), color: 1 },
        ]
    }

    #[test]
    fn plain_text_parses_to_itself() {
        let p = parse_task_input(&projects(), "buy groceries");
        assert_eq!(p.text, "buy groceries");
        assert_eq!(p.project, None);
        assert_eq!(p.due, None);
        assert_eq!(p.section, None);
    }

    #[test]
    fn section_suffix_is_prefix_matched() {
        assert_eq!(
            parse_task_input(&projects(), "buy milk #general").section,
            Some(DashSection::General)
        );
        assert_eq!(
            parse_task_input(&projects(), "buy milk #i").section,
            Some(DashSection::Important)
        );
        assert_eq!(
            parse_task_input(&projects(), "buy milk #a").section,
            Some(DashSection::Agent)
        );
        assert_eq!(parse_task_input(&projects(), "buy milk #i").text, "buy milk");
    }

    #[test]
    fn tokens_combine_in_any_order() {
        let a = parse_task_input(&projects(), "fix undo bug @algo !2026-09-01 #important");
        assert_eq!(a.text, "fix undo bug");
        assert_eq!(a.project, Some(7));
        assert_eq!(a.due, Some("2026-09-01".to_string()));
        assert_eq!(a.section, Some(DashSection::Important));

        let b = parse_task_input(&projects(), "fix undo bug #important !2026-09-01 @algo");
        assert_eq!(b.text, "fix undo bug");
        assert_eq!(b.project, Some(7));
        assert_eq!(b.due, Some("2026-09-01".to_string()));
        assert_eq!(b.section, Some(DashSection::Important));
    }

    #[test]
    fn unknown_section_is_left_as_literal_text() {
        let p = parse_task_input(&projects(), "read chapter #4");
        assert_eq!(p.text, "read chapter #4");
        assert_eq!(p.section, None);
    }

    #[test]
    fn unknown_course_is_left_as_literal_text() {
        let p = parse_task_input(&projects(), "email bob@example.com");
        assert_eq!(p.text, "email bob@example.com");
        assert_eq!(p.project, None);
    }
}
