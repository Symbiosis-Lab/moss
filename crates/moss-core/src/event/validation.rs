//! Validation of the event fields (`start`, `end`, `timezone`).
//!
//! moss-core has no time-zone database, so `timezone` is checked for shape
//! only: an `Area/City` token, not a lookup.

use crate::validation::{Diagnostic, Severity};
use crate::event::{check_end_after_start, EventTime};
use std::collections::HashMap;

fn warn(path: &str, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        message,
        path: Some(path.to_string()),
        line: None,
        column: None,
    }
}

/// `format: "event-time"` — a local date or local date-time.
pub(crate) fn check_event_time(name: &str, s: &str) -> Option<Diagnostic> {
    EventTime::parse(s)
        .err()
        .map(|_| warn(name, format!(
            "field '{name}' has invalid event time '{s}'; expected YYYY-MM-DD or YYYY-MM-DD HH:MM (local time, no offset or Z)"
        )))
}

/// `timezone` looks like an IANA name: letters, digits, `_`, `/`, `+`, `-`,
/// with at least one `/` unless it is a bare name such as `UTC`.
pub(crate) fn check_timezone(s: &str) -> Option<Diagnostic> {
    let ok = !s.is_empty()
        && !s.starts_with('/')
        && !s.ends_with('/')
        && !s.contains("//")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '+' | '-'));
    (!ok).then(|| warn("timezone", format!(
        "field 'timezone' has invalid zone name '{s}'; expected an IANA name such as Asia/Taipei"
    )))
}

/// `end` must not be before `start`. Silent when either is missing or
/// malformed — the per-field check already reports a malformed one.
pub(crate) fn check_end_not_before_start(fm: &HashMap<String, serde_yaml::Value>) -> Option<Diagnostic> {
    let start = fm.get("start")?.as_str()?;
    let end = fm.get("end")?.as_str()?;
    if EventTime::parse(start).is_err() || EventTime::parse(end).is_err() {
        return None;
    }
    check_end_after_start(start, end)
        .err()
        .map(|msg| warn("end", format!("field 'end' is before 'start': {msg}")))
}

#[cfg(test)]
mod tests {
    use crate::schema::builtin_schema;
    use crate::validation::{validate_frontmatter, Diagnostic};
    use std::collections::HashMap;

    fn diags(pairs: &[(&str, &str)]) -> Vec<Diagnostic> {
        let fm: HashMap<String, serde_yaml::Value> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_yaml::Value::String(v.to_string())))
            .collect();
        validate_frontmatter(&fm, &builtin_schema())
            .into_iter()
            .filter(|d| d.path.as_deref().is_some_and(|p| ["start", "end", "timezone", "status"].contains(&p)))
            .collect()
    }

    #[test]
    fn valid_event_fields_produce_no_diagnostic() {
        let d = diags(&[
            ("start", "2026-11-01 14:00"),
            ("end", "2026-11-01 16:30"),
            ("timezone", "Asia/Taipei"),
            ("status", "moved-online"),
        ]);
        assert!(d.is_empty(), "{d:?}");
        assert!(diags(&[("start", "2026-11-01"), ("end", "2026-11-03")]).is_empty());
    }

    #[test]
    fn malformed_start_is_reported() {
        let d = diags(&[("start", "next friday")]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].path.as_deref(), Some("start"));
    }

    #[test]
    fn offset_and_z_are_rejected() {
        for bad in ["2026-11-01T14:00Z", "2026-11-01 14:00+08:00", "2026-11-01T14:00-05:00"] {
            assert_eq!(diags(&[("start", bad)]).len(), 1, "{bad}");
        }
    }

    #[test]
    fn end_before_start_is_reported_once() {
        let d = diags(&[("start", "2026-11-03"), ("end", "2026-11-01")]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].path.as_deref(), Some("end"));
    }

    #[test]
    fn malformed_end_is_reported_without_a_second_ordering_complaint() {
        assert_eq!(diags(&[("start", "2026-11-03"), ("end", "soon")]).len(), 1);
    }

    #[test]
    fn unknown_status_is_reported() {
        assert_eq!(diags(&[("status", "tentative")]).len(), 1);
    }

    #[test]
    fn timezone_shape_is_checked() {
        assert!(diags(&[("timezone", "America/Argentina/Buenos_Aires")]).is_empty());
        assert!(diags(&[("timezone", "Etc/GMT+8")]).is_empty());
        assert!(diags(&[("timezone", "Etc/UTC")]).is_empty());
        assert!(diags(&[("timezone", "Asia/Taipei")]).is_empty());
        for bad in ["Taipei time", "/Asia/Taipei", "Asia//Taipei", "Asia/Taipei/", ""] {
            assert_eq!(diags(&[("timezone", bad)]).len(), 1, "{bad:?}");
        }
    }
}
