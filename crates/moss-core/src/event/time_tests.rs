use super::*;
use crate::event::{check_end_after_start, when};

#[test]
fn bare_date_is_all_day() {
    let t = EventTime::parse("2026-11-01").unwrap();
    assert!(t.is_all_day());
    assert_eq!(t.sort_key(), "2026-11-01");
}

#[test]
fn space_and_t_separators_agree() {
    let a = EventTime::parse("2026-11-01 14:00").unwrap();
    let b = EventTime::parse("2026-11-01T14:00").unwrap();
    assert_eq!(a, b);
    assert!(!a.is_all_day());
    assert_eq!(a.sort_key(), "2026-11-01T14:00");
}

#[test]
fn malformed_values_are_rejected() {
    for bad in [
        "", "tomorrow", "2026-11", "2026-13-01", "2026-02-30", "2026-11-01 25:00",
        "2026-11-01 14:60", "2026-11-01 14", "2026-11-01T14:00Z", "2026-11-01T14:00+08:00",
        "2026-1-1", "0000-01-01", "2026-11-01 14:00:00",
    ] {
        assert!(EventTime::parse(bad).is_err(), "{bad:?} should be rejected");
    }
    assert!(EventTime::parse("2028-02-29").is_ok());
}

#[test]
fn all_day_sorts_before_timed_on_the_same_day() {
    let all_day = EventTime::parse("2026-11-01").unwrap().sort_key();
    let early = EventTime::parse("2026-11-01 00:00").unwrap().sort_key();
    let next = EventTime::parse("2026-11-02").unwrap().sort_key();
    assert!(all_day < early && early < next);
}

#[test]
fn end_before_start_is_an_error() {
    assert!(check_end_after_start("2026-11-02", "2026-11-01").is_err());
    assert!(check_end_after_start("2026-11-01 14:00", "2026-11-01 13:59").is_err());
}

#[test]
fn inclusive_and_equal_ends_are_accepted() {
    assert!(check_end_after_start("2026-11-01", "2026-11-03").is_ok());
    assert!(check_end_after_start("2026-11-01", "2026-11-01").is_ok());
    assert!(check_end_after_start("2026-11-01 14:00", "2026-11-01 14:00").is_ok());
    // An all-day end covers the whole day, so it is not before a timed start on that day.
    assert!(check_end_after_start("2026-11-03 19:30", "2026-11-03").is_ok());
}

#[test]
fn when_prefers_start_and_falls_back_to_date() {
    assert_eq!(when(Some("2026-11-01 14:00"), Some("2026-09-01")).as_deref(), Some("2026-11-01T14:00"));
    assert_eq!(when(None, Some("2026-09-01")).as_deref(), Some("2026-09-01"));
    assert_eq!(when(Some("garbage"), Some("2026-09")).as_deref(), Some("2026-09"));
    assert_eq!(when(None, None), None);
}
