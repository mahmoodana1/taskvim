//! Due-date parsing. Kept free of ratatui so the date logic stays
//! unit-testable in isolation.

use chrono::{Datelike, Duration as CDuration, Local, Months, NaiveDate};

use crate::model::{Repeat, RepeatUnit};

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

/// Snap a date back to the start of its grid cell: the day itself, its Monday,
/// or the 1st of its month.
#[allow(dead_code)] // consumed by Task 20 (the tick) and Task 22 (the repeat menu)
pub fn align(date: NaiveDate, unit: RepeatUnit) -> NaiveDate {
    match unit {
        RepeatUnit::Day => date,
        RepeatUnit::Week => {
            date - CDuration::days(date.weekday().num_days_from_monday() as i64)
        }
        RepeatUnit::Month => {
            NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
        }
    }
}

/// Whole grid cells from `base` to `target`. Both must already be aligned.
#[allow(dead_code)] // consumed via period_start, by Task 20 (the tick)
fn units_between(base: NaiveDate, target: NaiveDate, unit: RepeatUnit) -> i64 {
    match unit {
        RepeatUnit::Day => (target - base).num_days(),
        RepeatUnit::Week => (target - base).num_days() / 7,
        RepeatUnit::Month => {
            (target.year() as i64 - base.year() as i64) * 12
                + (target.month() as i64 - base.month() as i64)
        }
    }
}

#[allow(dead_code)] // consumed via period_start, by Task 20 (the tick)
fn add_units(base: NaiveDate, n: i64, unit: RepeatUnit) -> NaiveDate {
    match unit {
        RepeatUnit::Day => base + CDuration::days(n),
        RepeatUnit::Week => base + CDuration::days(n * 7),
        RepeatUnit::Month => {
            if n >= 0 {
                base.checked_add_months(Months::new(n as u32)).unwrap_or(base)
            } else {
                base.checked_sub_months(Months::new((-n) as u32)).unwrap_or(base)
            }
        }
    }
}

/// The start of the period `today` falls in, on the fixed grid defined by the
/// repeat's unit, stride and anchor. Ticks land here regardless of whether the
/// task was ever completed.
#[allow(dead_code)] // consumed by Task 20 (the tick)
pub fn period_start(today: NaiveDate, repeat: &Repeat) -> NaiveDate {
    let anchor = NaiveDate::parse_from_str(&repeat.anchor, "%Y-%m-%d").unwrap_or(today);
    let base = align(anchor, repeat.unit);
    let target = align(today, repeat.unit);
    let every = repeat.every.max(1) as i64;
    // div_euclid floors toward negative infinity, so dates before the anchor
    // land on an earlier grid cell instead of rounding toward it.
    let steps = units_between(base, target, repeat.unit).div_euclid(every);
    add_units(base, steps * every, repeat.unit)
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

    use crate::model::{Repeat, RepeatUnit};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn rep(every: u32, unit: RepeatUnit, anchor: &str) -> Repeat {
        Repeat { every, unit, anchor: anchor.to_string() }
    }

    #[test]
    fn daily_period_is_the_day_itself() {
        let r = rep(1, RepeatUnit::Day, "2026-08-01");
        assert_eq!(period_start(d("2026-08-26"), &r), d("2026-08-26"));
        assert_eq!(period_start(d("2026-08-27"), &r), d("2026-08-27"));
    }

    #[test]
    fn every_three_days_strides_from_the_anchor() {
        // anchor Aug 20 -> ticks land on 20, 23, 26, 29
        let r = rep(3, RepeatUnit::Day, "2026-08-20");
        assert_eq!(period_start(d("2026-08-20"), &r), d("2026-08-20"));
        assert_eq!(period_start(d("2026-08-22"), &r), d("2026-08-20"));
        assert_eq!(period_start(d("2026-08-23"), &r), d("2026-08-23"));
        assert_eq!(period_start(d("2026-08-28"), &r), d("2026-08-26"));
        assert_eq!(period_start(d("2026-08-29"), &r), d("2026-08-29"));
    }

    #[test]
    fn weekly_periods_start_on_monday() {
        // 2026-08-26 is a Wednesday; its Monday is 2026-08-24.
        let r = rep(1, RepeatUnit::Week, "2026-08-05");
        assert_eq!(period_start(d("2026-08-26"), &r), d("2026-08-24"));
        assert_eq!(period_start(d("2026-08-24"), &r), d("2026-08-24"));
        assert_eq!(period_start(d("2026-08-30"), &r), d("2026-08-24"));
        assert_eq!(period_start(d("2026-08-31"), &r), d("2026-08-31"));
    }

    #[test]
    fn every_two_weeks_strides_from_the_anchor_monday() {
        // anchor 2026-08-05 (Wed) -> its Monday is 2026-08-03; ticks 03, 17, 31
        let r = rep(2, RepeatUnit::Week, "2026-08-05");
        assert_eq!(period_start(d("2026-08-10"), &r), d("2026-08-03"));
        assert_eq!(period_start(d("2026-08-17"), &r), d("2026-08-17"));
        assert_eq!(period_start(d("2026-08-30"), &r), d("2026-08-17"));
        assert_eq!(period_start(d("2026-08-31"), &r), d("2026-08-31"));
    }

    #[test]
    fn monthly_periods_start_on_the_first() {
        let r = rep(1, RepeatUnit::Month, "2026-01-31");
        assert_eq!(period_start(d("2026-02-15"), &r), d("2026-02-01"));
        assert_eq!(period_start(d("2026-03-01"), &r), d("2026-03-01"));
    }

    #[test]
    fn every_three_months_strides_from_the_anchor_month() {
        // anchor Jan -> ticks Jan, Apr, Jul, Oct
        let r = rep(3, RepeatUnit::Month, "2026-01-10");
        assert_eq!(period_start(d("2026-03-31"), &r), d("2026-01-01"));
        assert_eq!(period_start(d("2026-04-01"), &r), d("2026-04-01"));
        assert_eq!(period_start(d("2026-06-30"), &r), d("2026-04-01"));
        assert_eq!(period_start(d("2026-07-01"), &r), d("2026-07-01"));
    }

    #[test]
    fn dates_before_the_anchor_do_not_panic() {
        let r = rep(3, RepeatUnit::Day, "2026-08-20");
        assert_eq!(period_start(d("2026-08-14"), &r), d("2026-08-14"));
    }

    #[test]
    fn every_zero_is_treated_as_every_one() {
        let r = rep(0, RepeatUnit::Day, "2026-08-20");
        assert_eq!(period_start(d("2026-08-26"), &r), d("2026-08-26"));
    }
}
