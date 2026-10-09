//! Schema.org `Event` JSON-LD for pages that carry `start:`.
//!
//! An event page gets this block instead of the `Article` one: Google's Event
//! guidance wants one primary type per page, and an event's own fields
//! (`startDate`, `location`, `eventStatus`) have no `Article` counterpart.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use moss_core::event::EventTime;
use serde_json::{json, Map, Value};

use crate::build::feeds::calendar::zone;
use crate::build::page::meta::{ld_script, CoverRef};
use crate::build::site_url::SiteUrl;
use crate::build::types::EventFields;

/// UTC offset of the IANA zone `name` at the local wall-clock time `local`, as
/// `+05:30`, or `None` when the name is unknown. Adapts chrono's local time to
/// the calendar zone lookup, which takes the parsed event time.
pub(crate) fn utc_offset_for(name: &str, local: NaiveDateTime) -> Option<String> {
    let at = EventTime::DateTime(
        local.year() as u16,
        local.month() as u8,
        local.day() as u8,
        local.hour() as u8,
        local.minute() as u8,
    );
    zone::utc_offset(name, &at)
}

/// `eventStatus` for a `status:` value; absent or unknown is scheduled.
fn event_status(status: Option<&str>) -> &'static str {
    match status.map(str::trim) {
        Some("cancelled") => "https://schema.org/EventCancelled",
        Some("postponed") => "https://schema.org/EventPostponed",
        Some("moved-online") => "https://schema.org/EventMovedOnline",
        Some("rescheduled") => "https://schema.org/EventRescheduled",
        _ => "https://schema.org/EventScheduled",
    }
}

/// One `startDate`/`endDate` value: date only for an all-day value, otherwise
/// the local date-time, with the zone's offset when `offset_for` knows it.
fn schema_time(time: &EventTime, zone: Option<&str>) -> Option<String> {
    match *time {
        EventTime::Date(y, m, d) => Some(format!("{y:04}-{m:02}-{d:02}")),
        EventTime::DateTime(y, mo, d, h, mi) => {
            let local = NaiveDate::from_ymd_opt(i32::from(y), u32::from(mo), u32::from(d))?
                .and_hms_opt(u32::from(h), u32::from(mi), 0)?;
            let text = local.format("%Y-%m-%dT%H:%M").to_string();
            match zone.and_then(|z| utc_offset_for(z, local)) {
                Some(off) => Some(format!("{text}{off}")), // `off` is `+08:00`
                None => Some(text),
            }
        }
    }
}

/// Build the JSON-LD block, or `None` when `start` does not parse (the page
/// then keeps its ordinary markup).
#[allow(clippy::too_many_arguments)]
pub fn build_event_json_ld(
    ev: &EventFields,
    places: &[String],
    title: &str,
    description: &str,
    url: &str,
    site_name: &str,
    cover: Option<&CoverRef>,
    tags: &[String],
    site_url: &SiteUrl,
) -> Option<String> {
    let mut o = Map::new();
    o.insert("@context".into(), json!("https://schema.org"));
    o.insert("@type".into(), json!("Event"));
    o.insert("name".into(), json!(title));
    o.insert("url".into(), json!(url));
    if !description.is_empty() {
        o.insert("description".into(), json!(description));
    }
    let zone = ev.timezone.as_deref();
    o.insert("startDate".into(), json!(schema_time(&ev.start, zone)?));
    if let Some(end) = ev.end.as_ref().and_then(|e| schema_time(e, zone)) {
        o.insert("endDate".into(), json!(end));
    }
    o.insert("eventStatus".into(), json!(event_status(ev.status.as_deref())));

    let mut locations: Vec<Value> = places
        .iter()
        .map(|p| json!({"@type": "Place", "name": p}))
        .collect();
    if let Some(online) = ev.online.as_deref() {
        locations.push(json!({"@type": "VirtualLocation", "url": online}));
    }
    let mode = match (ev.online.is_some(), !places.is_empty()) {
        (true, true) => Some("MixedEventAttendanceMode"),
        (true, false) => Some("OnlineEventAttendanceMode"),
        (false, true) => Some("OfflineEventAttendanceMode"),
        (false, false) => None,
    };
    if let Some(mode) = mode {
        o.insert("eventAttendanceMode".into(), json!(format!("https://schema.org/{mode}")));
    }
    match locations.len() {
        0 => {}
        1 => {
            o.insert("location".into(), locations.remove(0));
        }
        _ => {
            o.insert("location".into(), Value::Array(locations));
        }
    }
    if let Some(tickets) = ev.tickets.as_deref() {
        o.insert("offers".into(), json!({"@type": "Offer", "url": tickets}));
    }
    if let Some(c) = cover {
        o.insert("image".into(), json!(c.to_meta_url(site_url)));
    }
    if !site_name.is_empty() {
        o.insert("organizer".into(), json!({"@type": "Organization", "name": site_name}));
    }

    if !tags.is_empty() {
        o.insert("keywords".into(), json!(tags));
    }
    Some(ld_script(&Value::Object(o)))
}

/// The block for `doc` when it is an event page of any layout. Reads the
/// fields the pipeline already parsed into `doc.event`.
pub fn for_page(
    doc: &crate::build::types::ParsedDocument,
    description: &str,
    site_name: &str,
    cover: Option<&CoverRef>,
    site_url: &SiteUrl,
) -> Result<Option<String>, String> {
    let Some(ev) = doc.event.as_ref() else { return Ok(None) };
    let url = crate::build::page::meta::page_meta_url(doc, site_url)?;
    Ok(build_event_json_ld(
        ev, &doc.location, &doc.label, description, &url, site_name, cover,
        doc.tags.as_deref().unwrap_or(&[]), site_url,
    ))
}

#[cfg(test)]
#[path = "event_json_ld_tests.rs"]
mod tests;
