//! Dates and times as Mychronix shows them: US style, month first (`Oct 17,
//! 2026`), with the clock in 12 hours unless Settings says 24. Every page
//! formats dates through these, so they read the same everywhere.

use aim_xrk::summary::Date;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// "Oct".
pub fn short_month(m: u8) -> &'static str {
    &MONTHS[(m.clamp(1, 12) - 1) as usize][..3]
}

/// 0 for Sunday … 6 for Saturday.
fn weekday(d: Date) -> usize {
    // 1970-01-01 was a Thursday.
    (days(d) + 4).rem_euclid(7) as usize
}

/// "Oct 17".
pub fn month_day(d: Date) -> String {
    format!("{} {}", short_month(d.month), d.day)
}

/// "Sat, Oct 17".
pub fn day_month_day(d: Date) -> String {
    format!("{}, {}", &DAYS[weekday(d)][..3], month_day(d))
}

/// "Sat 10/17".
pub fn day_numeric(d: Date) -> String {
    format!("{} {}/{}", &DAYS[weekday(d)][..3], d.month, d.day)
}

/// "Saturday, October 17, 2026".
pub fn long(d: Date) -> String {
    format!(
        "{}, {} {}, {}",
        DAYS[weekday(d)],
        MONTHS[(d.month.clamp(1, 12) - 1) as usize],
        d.day,
        d.year
    )
}

/// "Oct 17" from a session start like "2026-10-17 14:19:05" ("" when it isn't one).
pub fn stamp_month_day(stamp: &str) -> String {
    stamp_date(stamp).map(month_day).unwrap_or_default()
}

/// "Oct 17, 2026 at 2:19 PM" from a session start like "2026-10-17 14:19:05".
pub fn stamp_words(stamp: &str) -> String {
    let (_, time) = stamp.split_once(' ').unwrap_or((stamp, ""));
    match stamp_date(stamp) {
        Some(d) if time.is_empty() => us_short(d),
        Some(d) => format!("{} at {}", us_short(d), clock_time(time)),
        None => stamp.to_string(),
    }
}

/// The date of a session start like "2026-10-17 14:19:05".
fn stamp_date(stamp: &str) -> Option<Date> {
    let mut p = stamp.get(..10)?.split('-').map(|x| x.parse::<u16>().ok());
    let (year, month, day) = (p.next()??, p.next()??, p.next()??);
    (1..=12).contains(&month).then_some(Date {
        year,
        month: month as u8,
        day: day as u8,
    })
}

/// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
pub fn days(d: Date) -> i64 {
    let (m, dd) = (d.month as i64, d.day as i64);
    let y = d.year as i64 - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `n` days after 1970-01-01.
pub fn from_days(n: i64) -> Date {
    let z = n + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
    Date { year, month, day }
}

/// The Monday of `d`'s week, so a Saturday–Sunday race weekend stays together.
pub fn week_start(d: Date) -> Date {
    let n = days(d);
    // 1970-01-01 was a Thursday (3 days after Monday).
    from_days(n - (n + 3).rem_euclid(7))
}

/// Today on this computer's clock (local time: a race-day evening is still that day).
pub fn today() -> Date {
    let d = jiff::Zoned::now().date();
    Date {
        year: d.year() as u16,
        month: d.month() as u8,
        day: d.day() as u8,
    }
}

/// "10/17/2026" as written in the US, or "2026-10-17" (also "2026-10-7").
pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let (sep, us) = if s.contains('/') {
        ('/', true)
    } else {
        ('-', false)
    };
    let mut p = s.split(sep).map(|x| x.trim().parse::<u16>().ok());
    let (a, b, c) = (p.next()??, p.next()??, p.next()??);
    let (year, month, day) = if us { (c, a, b) } else { (a, b, c) };
    if p.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1990 {
        return None;
    }
    let d = Date {
        year,
        month: month as u8,
        day: day as u8,
    };
    // Reject days the month doesn't have (e.g. 31 September).
    (from_days(days(d)) == d).then_some(d)
}

/// "10/17/2026", for date fields.
pub fn us_numeric(d: Date) -> String {
    format!("{}/{}/{}", d.month, d.day, d.year)
}

/// "Oct 17, 2026".
pub fn us_short(d: Date) -> String {
    format!("{}, {}", month_day(d), d.year)
}

/// "2:19 PM" (or "14:19" on a 24-hour clock, see Settings) from the logger's "14:19:05".
pub fn clock_time(t: &str) -> String {
    format_time(t, crate::settings::clock_24())
}

fn format_time(t: &str, h24: bool) -> String {
    let mut p = t.split(':');
    let (Some(Ok(h)), Some(m)) = (p.next().map(|h| h.trim().parse::<u32>()), p.next()) else {
        return t.to_string();
    };
    let m = m.get(..2).unwrap_or(m);
    if h24 {
        return format!("{h:02}:{m}");
    }
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{h12}:{m} {ampm}")
}

pub fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: u16, month: u8, day: u8) -> Date {
        Date { year, month, day }
    }

    #[test]
    fn days_round_trip() {
        for date in [
            d(1970, 1, 1),
            d(2000, 2, 29),
            d(2026, 10, 10),
            d(2026, 12, 31),
        ] {
            assert_eq!(from_days(days(date)), date);
        }
        assert_eq!(days(d(1970, 1, 2)), 1);
    }

    #[test]
    fn weeks_start_on_monday() {
        // Saturday, October 10 and Sunday, October 11, 2026 share Monday, October 5.
        assert_eq!(week_start(d(2026, 10, 10)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 11)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 5)), d(2026, 10, 5));
        assert_eq!(week_start(d(2026, 10, 12)), d(2026, 10, 12));
    }

    #[test]
    fn parses_dates() {
        assert_eq!(parse_date("2026-10-17"), Some(d(2026, 10, 17)));
        assert_eq!(parse_date(" 2026-1-7 "), Some(d(2026, 1, 7)));
        assert_eq!(parse_date("2026-09-31"), None);
        assert_eq!(parse_date("17/10/2026"), None);
        assert_eq!(parse_date("10/17/2026"), Some(d(2026, 10, 17)));
        assert_eq!(us_numeric(d(2026, 1, 7)), "1/7/2026");
        assert_eq!(us_short(d(2026, 10, 17)), "Oct 17, 2026");
        assert_eq!(format_time("14:19:05", false), "2:19 PM");
        assert_eq!(format_time("14:19:05", true), "14:19");
        assert_eq!(format_time("09:05", true), "09:05");
        assert_eq!(format_time("00:05:00", false), "12:05 AM");
        assert_eq!(format_time("12:30", false), "12:30 PM");
    }

    #[test]
    fn dates_read_the_same_everywhere() {
        let d = Date {
            year: 2026,
            month: 10,
            day: 31,
        };
        assert_eq!(long(d), "Saturday, October 31, 2026");
        assert_eq!(us_short(d), "Oct 31, 2026");
        assert_eq!(month_day(d), "Oct 31");
        assert_eq!(day_month_day(d), "Sat, Oct 31");
        assert_eq!(day_numeric(d), "Sat 10/31");
        assert_eq!(stamp_month_day("2026-10-31 08:58:10"), "Oct 31");
        assert_eq!(
            long(Date {
                year: 2024,
                month: 2,
                day: 29
            }),
            "Thursday, February 29, 2024"
        );
        assert_eq!(
            long(Date {
                year: 2000,
                month: 1,
                day: 1
            }),
            "Saturday, January 1, 2000"
        );
    }
}
