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

/// An event's times as a page carries them: `start`, and `end` when it is a
/// valid value not before `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct EventSpan {
    pub start: EventTime,
    pub end: Option<EventTime>,
}

impl EventSpan {
    /// The span a page's `start:` and `end:` give; `None` without a usable `start`.
    pub fn from_fields(start: Option<&str>, end: Option<&str>) -> Option<Self> {
        let start_raw = start?;
        let start = EventTime::parse(start_raw).ok()?;
        let end = end.filter(|end| check_end_after_start(start_raw, end).is_ok()).and_then(|end| EventTime::parse(end).ok());
        Some(Self { start, end })
    }

    /// When the event is over, as wall-clock `YYYY-MM-DDTHH:MM`: an all-day
    /// event at the start of the day after its last day, a timed one at its
    /// `end` (else its `start`). The one definition of "ended": the event is
    /// over from this minute on, and `event-ends.ts` compares the same way.
    pub fn ends_at(&self) -> String {
        let last = self.end.unwrap_or(self.start);
        if !last.is_all_day() {
            return last.sort_key();
        }
        let (y, m, d) = last.ymd();
        let (y, m, d) = (u32::from(y), u32::from(m), u32::from(d));
        let (y, m, d) = if d < crate::date::days_in_month(y, m) { (y, m, d + 1) } else if m < 12 { (y, m + 1, 1) } else { (y + 1, 1, 1) };
        format!("{y:04}-{m:02}-{d:02}T00:00")
    }

    /// [`Self::ends_at`] while the event has not ended at `now`
    /// (`YYYY-MM-DDTHH:MM`), else `None`.
    pub fn ends_if_upcoming(&self, now: &str) -> Option<String> {
        let ends = self.ends_at();
        (now < ends.as_str()).then_some(ends)
    }

    pub fn is_upcoming(&self, now: &str) -> bool {
        self.ends_if_upcoming(now).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_all_day_event_ends_at_midnight_after_its_last_day_across_months_and_leap_years() {
        let ends = |s: &str| EventSpan::from_fields(Some(s), None).unwrap().ends_at();
        assert_eq!(ends("2026-01-31"), "2026-02-01T00:00");
        assert_eq!(ends("2026-02-28"), "2026-03-01T00:00");
        assert_eq!(ends("2028-02-28"), "2028-02-29T00:00");
        assert_eq!(ends("2026-04-30"), "2026-05-01T00:00");
        assert_eq!(ends("2026-12-31"), "2027-01-01T00:00");
    }

    #[test]
    fn an_end_before_the_start_is_dropped() {
        let span = EventSpan::from_fields(Some("2026-10-08 19:00"), Some("2026-10-07")).unwrap();
        assert_eq!(span.end, None);
        assert_eq!(span.ends_at(), "2026-10-08T19:00");
    }
}
