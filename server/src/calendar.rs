//! Business-day arithmetic for the Norfolk Island council calendar: Monday–Friday excluding rows of
//! the `holidays` table (calendar `'norfolk'`). Day 0 is the start date itself (excluded).

use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use sqlx::SqliteConnection;

use crate::error::AppResult;

/// Calendar name used by all council deadlines.
pub const NORFOLK_CALENDAR: &str = "norfolk";

/// Loads all holiday dates of the Norfolk calendar (a few dozen rows).
pub async fn holidays(conn: &mut SqliteConnection) -> AppResult<HashSet<NaiveDate>> {
    let rows: Vec<String> = sqlx::query_scalar("SELECT date FROM holidays WHERE calendar = ?")
        .bind(NORFOLK_CALENDAR)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).collect())
}

/// Pure: weekday and not a holiday.
pub fn is_business_day_in(holidays: &HashSet<NaiveDate>, d: NaiveDate) -> bool {
    !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) && !holidays.contains(&d)
}

/// Pure: the `n`th business day after `start` (start excluded). `n = 0` returns `start`;
/// negative `n` counts backwards.
pub fn add_business_days_in(holidays: &HashSet<NaiveDate>, start: NaiveDate, n: i64) -> NaiveDate {
    let step = if n >= 0 { 1 } else { -1 };
    let mut remaining = n.abs();
    let mut d = start;
    while remaining > 0 {
        d += Duration::days(step);
        if is_business_day_in(holidays, d) {
            remaining -= 1;
        }
    }
    d
}

/// Pure: number of business days in the half-open interval `(from, to]`
/// (0 when `to <= from`). This is the shift applied to a deadline when a pause from `from` ends on `to`.
pub fn business_days_between_in(holidays: &HashSet<NaiveDate>, from: NaiveDate, to: NaiveDate) -> i64 {
    let mut count = 0;
    let mut d = from;
    while d < to {
        d += Duration::days(1);
        if is_business_day_in(holidays, d) {
            count += 1;
        }
    }
    count
}

/// Pure: `d` itself if it is a business day, else the next business day.
pub fn next_business_day_on_or_after_in(holidays: &HashSet<NaiveDate>, d: NaiveDate) -> NaiveDate {
    let mut d = d;
    while !is_business_day_in(holidays, d) {
        d += Duration::days(1);
    }
    d
}

/// Is `d` a Norfolk business day?
pub async fn is_business_day(conn: &mut SqliteConnection, d: NaiveDate) -> AppResult<bool> {
    Ok(is_business_day_in(&holidays(conn).await?, d))
}

/// The `n`th business day after `start_local_date` (start excluded).
pub async fn add_business_days(
    conn: &mut SqliteConnection,
    start_local_date: NaiveDate,
    n: i64,
) -> AppResult<NaiveDate> {
    Ok(add_business_days_in(&holidays(conn).await?, start_local_date, n))
}

/// Business days in `(from, to]`.
pub async fn business_days_between(conn: &mut SqliteConnection, from: NaiveDate, to: NaiveDate) -> AppResult<i64> {
    Ok(business_days_between_in(&holidays(conn).await?, from, to))
}

/// `d` if it is a business day, else the next one (calendar-basis due dates roll forward).
pub async fn next_business_day_on_or_after(conn: &mut SqliteConnection, d: NaiveDate) -> AppResult<NaiveDate> {
    Ok(next_business_day_on_or_after_in(&holidays(conn).await?, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn weekends_are_skipped() {
        let h = HashSet::new();
        // Thu 2026-10-08 + 3 business days = Tue 2026-10-13.
        assert_eq!(add_business_days_in(&h, d("2026-10-08"), 3), d("2026-10-13"));
        // Starting on a Saturday: first business day after is Monday.
        assert_eq!(add_business_days_in(&h, d("2026-10-10"), 1), d("2026-10-12"));
        assert_eq!(add_business_days_in(&h, d("2026-10-10"), 0), d("2026-10-10"));
        assert_eq!(add_business_days_in(&h, d("2026-10-13"), -3), d("2026-10-08"));
    }

    #[test]
    fn holidays_are_skipped() {
        let h: HashSet<_> = [d("2026-10-12")].into_iter().collect();
        assert!(!is_business_day_in(&h, d("2026-10-12")));
        assert_eq!(add_business_days_in(&h, d("2026-10-09"), 1), d("2026-10-13"));
        assert_eq!(next_business_day_on_or_after_in(&h, d("2026-10-10")), d("2026-10-13"));
    }

    #[test]
    fn between_is_exclusive_start_inclusive_end() {
        let h: HashSet<_> = [d("2026-10-12")].into_iter().collect();
        // (Fri 9, Wed 14] = Tue 13, Wed 14 (Mon 12 holiday, weekend skipped).
        assert_eq!(business_days_between_in(&h, d("2026-10-09"), d("2026-10-14")), 2);
        assert_eq!(business_days_between_in(&h, d("2026-10-14"), d("2026-10-14")), 0);
        assert_eq!(business_days_between_in(&h, d("2026-10-14"), d("2026-10-09")), 0);
    }

    #[tokio::test]
    async fn uses_holidays_table() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let mut conn = state.db.acquire().await.unwrap();
        sqlx::query(
            "INSERT INTO holidays (calendar, date, name, source) VALUES ('norfolk', '2026-10-12', 'Test Day', 'demo')",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        assert!(!is_business_day(&mut conn, d("2026-10-12")).await.unwrap());
        assert!(is_business_day(&mut conn, d("2026-10-13")).await.unwrap());
        assert_eq!(add_business_days(&mut conn, d("2026-10-09"), 2).await.unwrap(), d("2026-10-14"));
        assert_eq!(business_days_between(&mut conn, d("2026-10-09"), d("2026-10-14")).await.unwrap(), 2);
    }
}
