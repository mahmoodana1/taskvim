//! Add-line parsing: `@course`, `!due` and `#section` suffixes. Pure — no
//! ratatui, no App — so it can be unit-tested directly.

use crate::due::{parse_due, parse_repeat};
use crate::model::{DashSection, Project, Repeat};

/// Everything an add-line can specify. Fields left `None` mean "not stated" —
/// the caller decides the default (e.g. the section under the cursor).
#[derive(Debug, Default, PartialEq)]
pub struct ParsedTask {
    pub text: String,
    pub project: Option<u64>,
    pub due: Option<String>,
    pub section: Option<DashSection>,
    pub repeat: Option<Repeat>,
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

/// Try to strip a trailing `!due` token from `text`. Returns the parsed due
/// date if one was found and consumed.
fn strip_due(text: &mut String) -> Option<Option<String>> {
    let tok = text.split_whitespace().last()?;
    let spec = tok.strip_prefix('!')?;
    let d = parse_due(spec).ok()?;
    let cut = text.rfind('!').unwrap();
    *text = text[..cut].trim().to_string();
    Some(d)
}

/// Try to strip a trailing `#section` token from `text`. Returns the matched
/// section if one was found and consumed.
fn strip_section(text: &mut String) -> Option<DashSection> {
    let tok = text.split_whitespace().last()?;
    let name = tok.strip_prefix('#')?;
    let s = match_section(name)?;
    let cut = text.rfind('#').unwrap();
    *text = text[..cut].trim().to_string();
    Some(s)
}

/// Try to strip a trailing `~spec` token from `text`. Returns the parsed
/// repeat if one was found and consumed. `anchor` is today's date, computed
/// once per `parse_task_input` call rather than re-derived on every attempt.
fn strip_repeat(text: &mut String, anchor: &str) -> Option<Option<Repeat>> {
    let tok = text.split_whitespace().last()?;
    let spec = tok.strip_prefix('~')?;
    let r = parse_repeat(spec, anchor).ok()?;
    let cut = text.rfind('~').unwrap();
    *text = text[..cut].trim().to_string();
    Some(r)
}

/// Try to strip a trailing `@course` token from `text`. Returns the matched
/// project id if one was found and consumed.
fn strip_project(text: &mut String, projects: &[Project]) -> Option<u64> {
    let pos = text.rfind('@')?;
    let name = text[pos + 1..].trim().to_lowercase();
    if name.is_empty() || pos == 0 {
        return None;
    }
    let p = projects
        .iter()
        .find(|p| p.name.to_lowercase().starts_with(&name))?;
    let id = p.id;
    *text = text[..pos].trim().to_string();
    Some(id)
}

/// Strip trailing `!due`, `@course`, `#section` and `~repeat` tokens in any
/// order. A token that doesn't resolve — an unknown course, an unparseable
/// date, a `#4` that isn't a section — is left in the text as literal content.
pub fn parse_task_input(projects: &[Project], buf: &str) -> ParsedTask {
    let mut text = buf.trim().to_string();
    let mut project = None;
    let mut due = None;
    let mut section = None;
    let mut repeat = None;
    // Computed once per call, not per strip attempt: "today" cannot change
    // mid-parse, so re-deriving it on every loop iteration would just be
    // repeated formatting work for the same answer.
    let anchor = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    // Keep stripping trailing tokens as long as some attempt makes progress.
    // Adding a fifth token type is just another `if` in this block — no
    // iteration bound to keep in sync with the number of token kinds.
    loop {
        let len_before = text.len();
        let mut progressed = false;

        if due.is_none() {
            if let Some(d) = strip_due(&mut text) {
                due = d;
                progressed = true;
            }
        }

        if section.is_none() {
            if let Some(s) = strip_section(&mut text) {
                section = Some(s);
                progressed = true;
            }
        }

        if project.is_none() {
            if let Some(p) = strip_project(&mut text, projects) {
                project = Some(p);
                progressed = true;
            }
        }

        if repeat.is_none() {
            if let Some(r) = strip_repeat(&mut text, &anchor) {
                repeat = r;
                progressed = true;
            }
        }

        if !progressed {
            break;
        }
        // A strip that reports progress must always shorten `text` — guard
        // against an infinite loop if that invariant is ever broken.
        debug_assert!(text.len() < len_before);
        if text.len() >= len_before {
            break;
        }
    }

    ParsedTask {
        text,
        project,
        due,
        section,
        repeat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DashSection, Project};

    fn projects() -> Vec<Project> {
        vec![
            Project {
                id: 7,
                name: "Algorithms 2".into(),
                color: 0,
            },
            Project {
                id: 8,
                name: "Statistics".into(),
                color: 1,
            },
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
        assert_eq!(
            parse_task_input(&projects(), "buy milk #i").text,
            "buy milk"
        );
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
    fn a_token_resolves_even_when_it_isnt_trailing_until_a_later_pass() {
        // The section token isn't trailing until the due and project tokens
        // after it are stripped first — this only resolves across multiple
        // passes, unlike the orderings in `tokens_combine_in_any_order` where
        // each token is already last on the pass that finds it.
        let p = parse_task_input(&projects(), "fix undo bug #important @algo !2026-09-01");
        assert_eq!(p.text, "fix undo bug");
        assert_eq!(p.project, Some(7));
        assert_eq!(p.due, Some("2026-09-01".to_string()));
        assert_eq!(p.section, Some(DashSection::Important));
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

    #[test]
    fn repeat_suffix_is_parsed_and_stripped() {
        let p = parse_task_input(&projects(), "water plants ~3d");
        assert_eq!(p.text, "water plants");
        assert_eq!(p.repeat.map(|r| r.label()), Some("every 3d".to_string()));
    }

    #[test]
    fn repeat_combines_with_section() {
        let p = parse_task_input(&projects(), "review flashcards ~daily #agent");
        assert_eq!(p.text, "review flashcards");
        assert_eq!(p.section, Some(DashSection::Agent));
        assert!(p.repeat.is_some());
    }
}
