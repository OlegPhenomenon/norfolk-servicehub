//! Timestamp helpers. Storage format: RFC 3339 UTC with milliseconds, e.g. `2026-10-07T03:15:00.000Z`.
//! Local dates (`YYYY-MM-DD`) are in `Pacific/Norfolk`.

use chrono::{DateTime, LocalResult, NaiveDate, NaiveTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;

use crate::error::{AppError, AppResult};

/// The council's time zone.
pub const NORFOLK: Tz = chrono_tz::Pacific::Norfolk;

/// Formats an instant in the storage format (`…T03:15:00.000Z`).
pub fn fmt(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Current wall-clock time in the storage format. Prefer `fmt(state.clock.now())` when a state is at hand.
pub fn now_str() -> String {
    fmt(crate::clock::now())
}

/// Parses any RFC 3339 timestamp into UTC.
pub fn parse(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| AppError::validation_msg(format!("Invalid timestamp: {s}")))
}

/// Converts to Norfolk local time.
pub fn to_local(dt: DateTime<Utc>) -> DateTime<Tz> {
    dt.with_timezone(&NORFOLK)
}

/// Norfolk local calendar date of an instant.
pub fn local_date(dt: DateTime<Utc>) -> NaiveDate {
    to_local(dt).date_naive()
}

/// Formats a local date as `YYYY-MM-DD`.
pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Parses `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::validation_msg(format!("Invalid date (expected YYYY-MM-DD): {s}")))
}

/// The UTC instant of a Norfolk local wall-clock time (e.g. "17:00 on day N").
/// During a DST gap the next valid instant is used; during an overlap the earlier one.
pub fn local_to_utc(date: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
    let naive = date.and_time(time);
    match NORFOLK.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(a, _) => a.with_timezone(&Utc),
        LocalResult::None => {
            let shifted = naive + chrono::Duration::hours(1);
            NORFOLK
                .from_local_datetime(&shifted)
                .earliest()
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
        }
    }
}

/// Human-friendly local rendering, e.g. `7 Oct 2026, 14:15` (for PDFs, emails).
pub fn display_local(dt: DateTime<Utc>) -> String {
    to_local(dt).format("%-d %b %Y, %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_local() {
        let dt = parse("2026-10-07T03:15:00Z").unwrap();
        assert_eq!(fmt(dt), "2026-10-07T03:15:00.000Z");
        // Norfolk is UTC+11 (UTC+12 in DST from first Sunday in October).
        assert_eq!(local_date(parse("2026-10-07T13:30:00Z").unwrap()), parse_date("2026-10-08").unwrap());
        let five_pm = local_to_utc(parse_date("2026-07-01").unwrap(), NaiveTime::from_hms_opt(17, 0, 0).unwrap());
        assert_eq!(fmt(five_pm), "2026-07-01T06:00:00.000Z");
    }
}
