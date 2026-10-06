//! Event fields: a page becomes an event by carrying `start:`.
//!
//! Pure functions over the field strings; no I/O. `date` keeps meaning "posted"
//! and is never read as the event time.

pub mod time;
pub mod validation;

pub use time::EventTime;

/// The value a listing sorts a page by: the event's `start` when it has one,
/// else its `date`. Both are returned in a form whose lexicographic order is
/// chronological (`date` is kept as written, so a year-only or year-month date
/// sorts by its leading digits as it does elsewhere). An unparseable `start`
/// is ignored rather than trusted.
///
/// `date` is returned as written (trimmed; empty is `None`). There is no
/// filename or ctime fallback here, so swapping a sort accessor that reads the
/// raw `date` field for this function preserves its order. A listing-date helper
/// that does have those fallbacks must pass its resolved date in as `date`, not
/// be replaced by this.
pub fn when(start: Option<&str>, date: Option<&str>) -> Option<String> {
    if let Some(t) = start.and_then(|s| EventTime::parse(s).ok()) {
        return Some(t.sort_key());
    }
    date.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string)
}

/// `end` must not be before `start`. An all-day `end` is inclusive: it covers
/// the whole of that day, so `end: 2026-11-03` is on or after any start on the
/// 3rd. Equal values are accepted.
pub fn check_end_after_start(start: &str, end: &str) -> Result<(), String> {
    let (start, end) = (EventTime::parse(start)?, EventTime::parse(end)?);
    let ok = if end.is_all_day() {
        end.day_key() >= start.day_key()
    } else {
        end.sort_key() >= start.sort_key()
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "end '{}' is before start '{}'",
            end.sort_key(),
            start.sort_key()
        ))
    }
}
