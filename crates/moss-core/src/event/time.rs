//! Event start/end values: a local date (all-day) or a local date-time.
//!
//! Venue-local wall-clock time, no offset and no `Z`: an event "at 14:00" is at
//! 14:00 where it happens, whatever zone the reader or the build machine is in.
//! moss-core has no time-zone database (see `date.rs`), so this module only
//! parses and orders; the IANA zone name travels separately as a string.

/// A parsed `start:` or `end:` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventTime {
    /// `YYYY-MM-DD`: an all-day value.
    Date(u16, u8, u8),
    /// `YYYY-MM-DD HH:MM` or `YYYY-MM-DDTHH:MM`.
    DateTime(u16, u8, u8, u8, u8),
}

impl EventTime {
    /// Parse `YYYY-MM-DD`, `YYYY-MM-DD HH:MM` or `YYYY-MM-DDTHH:MM`.
    pub fn parse(s: &str) -> Result<EventTime, String> {
        let s = s.trim();
        let bad = || format!("'{s}' is not a date (YYYY-MM-DD) or a date and time (YYYY-MM-DD HH:MM)");
        let (date, time) = match s.split_once([' ', 'T']) {
            Some((d, t)) => (d, Some(t.trim())),
            None => (s, None),
        };

        let (y, m, d) = crate::date::parse_ymd(date).ok_or_else(bad)?;

        let Some(time) = time else {
            return Ok(EventTime::Date(y, m, d));
        };
        let (h, min) = time.split_once(':').ok_or_else(bad)?;
        let h = crate::date::fixed_digits(h, 2).ok_or_else(bad)?;
        let min = crate::date::fixed_digits(min, 2).ok_or_else(bad)?;
        if h > 23 || min > 59 {
            return Err(bad());
        }
        Ok(EventTime::DateTime(y, m, d, h as u8, min as u8))
    }

    pub fn is_all_day(&self) -> bool {
        matches!(self, EventTime::Date(..))
    }

    /// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`. Lexicographic order is chronological,
    /// and an all-day value sorts before a timed one on the same day.
    pub fn sort_key(&self) -> String {
        match *self {
            EventTime::Date(y, m, d) => format!("{y:04}-{m:02}-{d:02}"),
            EventTime::DateTime(y, m, d, h, min) => {
                format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}")
            }
        }
    }

    /// Year, month and day.
    pub fn ymd(&self) -> (u16, u8, u8) {
        match *self {
            EventTime::Date(y, m, d) | EventTime::DateTime(y, m, d, _, _) => (y, m, d),
        }
    }

    /// Hour and minute; `None` for an all-day value.
    pub fn hm(&self) -> Option<(u8, u8)> {
        match *self {
            EventTime::DateTime(_, _, _, h, m) => Some((h, m)),
            EventTime::Date(..) => None,
        }
    }

    /// The calendar day, as `YYYY-MM-DD`.
    pub fn day_key(&self) -> String {
        match *self {
            EventTime::Date(y, m, d) | EventTime::DateTime(y, m, d, _, _) => {
                format!("{y:04}-{m:02}-{d:02}")
            }
        }
    }
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
