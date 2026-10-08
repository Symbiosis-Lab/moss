//! Event fields: a page becomes an event by carrying `start:`.
//!
//! Pure functions over the field strings; no I/O. `date` keeps meaning "posted"
//! and is never read as the event time.

pub mod time;
pub mod validation;

pub use time::EventTime;

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
