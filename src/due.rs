use chrono::{Datelike, Duration as CDuration, Local, Months, NaiveDate};

/// Accepts: "" / none / today / tomorrow / +N / YYYY-MM-DD / MM-DD / M/D
pub fn parse_due(spec: &str) -> Result<Option<String>, ()> {
    let s = spec.trim().to_lowercase();
    let today = Local::now().date_naive();
    if s.is_empty() || s == "none" || s == "clear" || s == "-" {
        return Ok(None);
    }
    if s == "today" {
        return Ok(Some(today.format("%Y-%m-%d").to_string()));
    }
    if s == "tomorrow" || s == "tmr" {
        return Ok(Some((today + CDuration::days(1)).format("%Y-%m-%d").to_string()));
    }
    // Bare unit shortcuts: "w" = +1 week, "mo" = +1 month, "d" = +1 day, "y" = +1 year.
    let bare = match s.as_str() {
        "d" => Some(today + CDuration::days(1)),
        "w" => Some(today + CDuration::days(7)),
        "mo" | "m" => today.checked_add_months(Months::new(1)),
        "y" => today.checked_add_months(Months::new(12)),
        _ => None,
    };
    if let Some(date) = bare {
        return Ok(Some(date.format("%Y-%m-%d").to_string()));
    }
    // Relative offsets: +N, +Nd, +Nw, +Nmo, +Ny (leading '+' optional for these units).
    let stripped = s.strip_prefix('+').unwrap_or(&s);
    let try_relative = |stripped: &str| -> Option<NaiveDate> {
        let (num_str, unit) = if let Some(rest) = stripped.strip_suffix("mo") {
            (rest, "mo")
        } else if let Some(rest) = stripped.strip_suffix('w') {
            (rest, "w")
        } else if let Some(rest) = stripped.strip_suffix('d') {
            (rest, "d")
        } else if let Some(rest) = stripped.strip_suffix('y') {
            (rest, "y")
        } else {
            (stripped, "d")
        };
        let n: i64 = num_str.parse().ok()?;
        match unit {
            "d" => Some(today + CDuration::days(n)),
            "w" => Some(today + CDuration::days(n * 7)),
            "mo" => {
                if n >= 0 {
                    today.checked_add_months(Months::new(n as u32))
                } else {
                    today.checked_sub_months(Months::new((-n) as u32))
                }
            }
            "y" => {
                if n >= 0 {
                    today.checked_add_months(Months::new((n as u32) * 12))
                } else {
                    today.checked_sub_months(Months::new(((-n) as u32) * 12))
                }
            }
            _ => None,
        }
    };
    if s.starts_with('+') {
        return match try_relative(stripped) {
            Some(d) => Ok(Some(d.format("%Y-%m-%d").to_string())),
            None => Err(()),
        };
    }
    // Relative without '+' — only if it ends in a unit letter (avoid clobbering "12-25").
    if s.ends_with('w') || s.ends_with("mo") || s.ends_with('d') || s.ends_with('y') {
        if let Some(d) = try_relative(stripped) {
            return Ok(Some(d.format("%Y-%m-%d").to_string()));
        }
    }
    let norm = s.replace('/', "-");
    let parts: Vec<&str> = norm.split('-').collect();
    let (y, m, d) = match parts.len() {
        3 => (
            parts[0].parse::<i32>().map_err(|_| ())?,
            parts[1].parse::<u32>().map_err(|_| ())?,
            parts[2].parse::<u32>().map_err(|_| ())?,
        ),
        2 => (
            today.year(),
            parts[0].parse::<u32>().map_err(|_| ())?,
            parts[1].parse::<u32>().map_err(|_| ())?,
        ),
        _ => return Err(()),
    };
    match NaiveDate::from_ymd_opt(y, m, d) {
        Some(date) => Ok(Some(date.format("%Y-%m-%d").to_string())),
        None => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        Local::now().date_naive()
    }

    fn ymd(d: NaiveDate) -> String {
        d.format("%Y-%m-%d").to_string()
    }

    #[test]
    fn parse_due_clears_on_none_forms() {
        for s in ["", "none", "clear", "-", "  NONE  "] {
            assert_eq!(parse_due(s), Ok(None), "input: {:?}", s);
        }
    }

    #[test]
    fn parse_due_keywords() {
        assert_eq!(parse_due("today"), Ok(Some(ymd(today()))));
        assert_eq!(
            parse_due("tomorrow"),
            Ok(Some(ymd(today() + CDuration::days(1))))
        );
        assert_eq!(parse_due("tmr"), Ok(Some(ymd(today() + CDuration::days(1)))));
    }

    #[test]
    fn parse_due_relative_offsets() {
        assert_eq!(parse_due("+3"), Ok(Some(ymd(today() + CDuration::days(3)))));
        assert_eq!(parse_due("+3d"), Ok(Some(ymd(today() + CDuration::days(3)))));
        assert_eq!(parse_due("+2w"), Ok(Some(ymd(today() + CDuration::days(14)))));
        assert_eq!(parse_due("2w"), Ok(Some(ymd(today() + CDuration::days(14)))));
    }

    #[test]
    fn parse_due_bare_unit_shortcuts() {
        assert_eq!(parse_due("d"), Ok(Some(ymd(today() + CDuration::days(1)))));
        assert_eq!(parse_due("w"), Ok(Some(ymd(today() + CDuration::days(7)))));
    }

    #[test]
    fn parse_due_absolute_dates() {
        assert_eq!(parse_due("2026-07-10"), Ok(Some("2026-07-10".to_string())));
        assert_eq!(parse_due("2026/07/10"), Ok(Some("2026-07-10".to_string())));
        let expected = format!("{}-12-25", today().year());
        assert_eq!(parse_due("12-25"), Ok(Some(expected)));
    }

    #[test]
    fn parse_due_rejects_garbage() {
        assert_eq!(parse_due("banana"), Err(()));
        assert_eq!(parse_due("2026-13-45"), Err(()));
        assert_eq!(parse_due("1-2-3-4"), Err(()));
    }

    #[test]
    fn parse_due_bare_month_year_shortcuts() {
        // "mo"/"m" and "y" go through checked_add_months rather than day arithmetic, so the
        // expected value must be computed the same way to stay correct near month-end dates
        // (e.g. Jan 31 + 1mo clamps to Feb 28/29) and on any date the suite happens to run.
        let expected_mo = ymd(today()
            .checked_add_months(Months::new(1))
            .expect("checked_add_months(1) does not overflow for any realistic 'today'"));
        assert_eq!(parse_due("mo"), Ok(Some(expected_mo.clone())));
        assert_eq!(parse_due("m"), Ok(Some(expected_mo)));

        let expected_y = ymd(today()
            .checked_add_months(Months::new(12))
            .expect("checked_add_months(12) does not overflow for any realistic 'today'"));
        assert_eq!(parse_due("y"), Ok(Some(expected_y)));

        // Contract note (not exercised here, and not exercisable from a test): if
        // `today.checked_add_months(..)` ever returned None — only possible if "today" were
        // within about a year of chrono's NaiveDate upper bound (~262,143 CE) — `bare` would
        // be None, the code would fall through past the bare-shortcut block, past the
        // relative-offset parser (which also fails on "mo"/"y" since there's no leading digit
        // to parse), and into the date-literal parser, which sees a single dash-free part and
        // returns Err(()). `parse_due` hardcodes `today` via `Local::now()` with no injection
        // point, so this branch cannot be triggered from a test without changing the function.
    }

    #[test]
    fn parse_due_month_year_and_negative_offsets() {
        // "+Nmo" / "+Ny" route through the same checked_add_months path as the bare
        // shortcuts above.
        let expected_2mo = ymd(today()
            .checked_add_months(Months::new(2))
            .expect("checked_add_months(2) does not overflow for any realistic 'today'"));
        assert_eq!(parse_due("+2mo"), Ok(Some(expected_2mo)));

        let expected_1y = ymd(today()
            .checked_add_months(Months::new(12))
            .expect("checked_add_months(12) does not overflow for any realistic 'today'"));
        assert_eq!(parse_due("+1y"), Ok(Some(expected_1y)));

        // Negative offsets are accepted even without a leading '+': the no-prefix path only
        // checks for a trailing unit letter, not a leading sign, so "-3d" reaches
        // try_relative via the `s.ends_with('d')` branch and "-3".parse::<i64>() succeeds.
        assert_eq!(parse_due("-3d"), Ok(Some(ymd(today() - CDuration::days(3)))));

        // Negative month offsets go through checked_sub_months.
        let expected_neg_1mo = ymd(today()
            .checked_sub_months(Months::new(1))
            .expect("checked_sub_months(1) does not underflow for any realistic 'today'"));
        assert_eq!(parse_due("-1mo"), Ok(Some(expected_neg_1mo)));
    }
}
