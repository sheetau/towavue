use super::SearchDate;

#[derive(Clone, Debug)]
pub(super) struct DateRange {
    start: i64,
    end: i64,
    invert: bool,
    unknown: bool,
}

impl DateRange {
    pub fn parse(text: &str, today: Option<SearchDate>) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        if text == "unknown" {
            return Some(Self {
                start: 0,
                end: 0,
                invert: false,
                unknown: true,
            });
        }
        let (operator, value) = ["!=", ">=", "<=", "==", "!", ">", "<", "="]
            .into_iter()
            .find_map(|op| text.strip_prefix(op).map(|value| (op, value)))
            .unwrap_or(("", &text));
        let (start, end) = if let Some((first, last)) = value.split_once("..") {
            let start = if first.is_empty() {
                i64::MIN
            } else {
                period(first, today)?.0
            };
            let end = if last.is_empty() {
                i64::MAX
            } else {
                period(last, today)?.1
            };
            if start > end || !operator.is_empty() {
                return None;
            }
            (start, end)
        } else {
            period(value, today)?
        };
        let (start, end) = match operator {
            ">" => (end.checked_add(1)?, i64::MAX),
            ">=" => (start, i64::MAX),
            "<" => (i64::MIN, start.checked_sub(1)?),
            "<=" => (i64::MIN, end),
            _ => (start, end),
        };
        Some(Self {
            start,
            end,
            invert: matches!(operator, "!" | "!="),
            unknown: false,
        })
    }

    pub fn matches(&self, date: Option<SearchDate>) -> bool {
        if self.unknown {
            return date.is_none();
        }
        date.and_then(day)
            .is_some_and(|day| ((self.start..=self.end).contains(&day)) != self.invert)
    }
}

fn period(text: &str, today: Option<SearchDate>) -> Option<(i64, i64)> {
    let named = matches!(
        text,
        "today"
            | "yesterday"
            | "thisweek"
            | "lastweek"
            | "thismonth"
            | "lastmonth"
            | "thisyear"
            | "lastyear"
    );
    if named {
        let (year, month, _) = today?;
        let current = day(today?)?;
        return match text {
            "today" => Some((current, current)),
            "yesterday" => Some((current - 1, current - 1)),
            "thisweek" | "lastweek" => {
                // 1970-01-01 was Thursday; calendar weeks start on Monday.
                let start =
                    current - (current + 3).rem_euclid(7) - if text == "lastweek" { 7 } else { 0 };
                Some((start, start + 6))
            }
            "thismonth" => month_period(year, month),
            "lastmonth" if month == 1 => month_period(year.checked_sub(1)?, 12),
            "lastmonth" => month_period(year, month - 1),
            "thisyear" => Some((day((year, 1, 1))?, day((year, 12, 31))?)),
            "lastyear" => Some((day((year.checked_sub(1)?, 1, 1))?, day((year - 1, 12, 31))?)),
            _ => unreachable!(),
        };
    }
    let text = text.replace('/', "-");
    let parts: Vec<_> = text
        .split('-')
        .map(str::parse::<u16>)
        .collect::<Result<_, _>>()
        .ok()?;
    match parts.as_slice() {
        [year] if text.len() == 4 => Some((day((*year, 1, 1))?, day((*year, 12, 31))?)),
        [year, month] => month_period(*year, *month),
        [year, month, date] => {
            let value = day((*year, *month, *date))?;
            Some((value, value))
        }
        _ => None,
    }
}

fn month_period(year: u16, month: u16) -> Option<(i64, i64)> {
    Some((
        day((year, month, 1))?,
        day((year, month, month_days(year, month)?))?,
    ))
}

fn month_days(year: u16, month: u16) -> Option<u16> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) {
                29
            } else {
                28
            }
        }
        _ => return None,
    })
}

fn day((year, month, date): SearchDate) -> Option<i64> {
    if !(1..=9999).contains(&year) || date == 0 || date > month_days(year, month)? {
        return None;
    }
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * month + 2) / 5 + i64::from(date) - 1;
    Some(era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468)
}
