//! schema.org `Event` mapped onto moss's event frontmatter fields.
//!
//! The page's own published date stays in `date`; an event only ever fills
//! `start`, `end`, `location`, `status`, `tickets` and `online`.

use serde_json::Value;

use super::{entry_string, has_type};

/// Event fields ready for frontmatter, keyed like moss-core's event builtins.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventMetadata {
    /// Venue-local `YYYY-MM-DD` or `YYYY-MM-DD HH:MM`.
    pub start: Option<String>,
    /// Same forms as `start`.
    pub end: Option<String>,
    /// Venue name (`Place.name`, else the address's first line, else the bare string).
    pub location: Option<String>,
    /// `cancelled`, `postponed`, `moved-online` or `rescheduled`; absent means scheduled.
    pub status: Option<String>,
    /// First offer URL.
    pub tickets: Option<String>,
    /// URL of a `VirtualLocation`.
    pub online: Option<String>,
}

/// schema.org `Event` and its standard subtypes
/// (https://schema.org/Event#subtypes).
const EVENT_TYPES: &[&str] = &[
    "Event",
    "BusinessEvent",
    "ChildrensEvent",
    "ComedyEvent",
    "CourseInstance",
    "DanceEvent",
    "DeliveryEvent",
    "EducationEvent",
    "ExhibitionEvent",
    "Festival",
    "FoodEvent",
    "Hackathon",
    "LiteraryEvent",
    "MusicEvent",
    "PublicationEvent",
    "SaleEvent",
    "ScreeningEvent",
    "SocialEvent",
    "SportsEvent",
    "TheaterEvent",
    "VisualArtsEvent",
];

/// Read the first `Event` (or subtype) among the page's JSON-LD entries.
pub fn derive_event(entries: &[Value]) -> EventMetadata {
    let Some(event) = entries.iter().find(|e| has_type(e, EVENT_TYPES)) else {
        return EventMetadata::default();
    };
    let (location, online) = pick_places(event);
    EventMetadata {
        start: entry_string(event, "startDate").and_then(|s| wall_clock(&s)),
        end: entry_string(event, "endDate").and_then(|s| wall_clock(&s)),
        location,
        status: entry_string(event, "eventStatus").and_then(|s| status_word(&s)),
        tickets: pick_tickets(event),
        online,
    }
}

/// Keep the wall-clock as written, in moss's forms. An offset or `Z` is
/// dropped, never applied: it cannot name a zone. Anything moss-core's parser
/// rejects is not written.
fn wall_clock(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let normalized = match raw.split_once(['T', ' ']) {
        None => raw.to_string(),
        Some((date, time)) => {
            let hhmm = time.get(..5)?;
            format!("{date} {hhmm}")
        }
    };
    moss_core::event::EventTime::parse(&normalized).ok()?;
    Some(normalized)
}

fn status_word(raw: &str) -> Option<String> {
    let name = raw.rsplit('/').next().unwrap_or(raw);
    let word = match name {
        "EventCancelled" => "cancelled",
        "EventPostponed" => "postponed",
        "EventMovedOnline" => "moved-online",
        "EventRescheduled" => "rescheduled",
        _ => return None,
    };
    Some(word.to_string())
}

/// `offers` is an object or an array; the first `url` is the ticket page.
fn pick_tickets(event: &Value) -> Option<String> {
    match event.get("offers")? {
        Value::Array(items) => items.iter().find_map(|o| entry_string(o, "url")),
        offer => entry_string(offer, "url"),
    }
}

/// `location` is a `Place`, a `VirtualLocation`, a bare string, or an array
/// mixing them (a hybrid event): the first venue and the first online URL.
fn pick_places(event: &Value) -> (Option<String>, Option<String>) {
    let Some(loc) = event.get("location") else {
        return (None, None);
    };
    let items = match loc {
        Value::Array(items) => items.as_slice(),
        one => std::slice::from_ref(one),
    };
    let mut venue = None;
    let mut online = None;
    for item in items {
        if has_type(item, &["VirtualLocation"]) {
            online = online.or_else(|| entry_string(item, "url"));
        } else {
            venue = venue.or_else(|| location_name(item));
        }
    }
    (venue, online)
}

fn location_name(loc: &Value) -> Option<String> {
    match loc {
        Value::String(s) => {
            let s = moss_core::html_entities::decode(s.trim());
            (!s.is_empty()).then_some(s)
        }
        Value::Object(_) => {
            entry_string(loc, "name").or_else(|| loc.get("address").and_then(address_first_line))
        }
        _ => None,
    }
}

/// First line of a schema.org `address`: a bare string (street, then
/// city/region/zip on following lines) or a `PostalAddress` object, whose
/// first line is its `streetAddress`.
fn address_first_line(addr: &Value) -> Option<String> {
    match addr {
        Value::String(s) => {
            let first = s.split('\n').next().unwrap_or(s).trim();
            (!first.is_empty()).then(|| moss_core::html_entities::decode(first))
        }
        Value::Object(_) => entry_string(addr, "streetAddress"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
