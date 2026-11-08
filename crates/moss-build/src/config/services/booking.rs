//! `[services.booking]`: what a site offers for booking, published as the
//! `booking` object of `/_moss/commerce.json` for a booking service to read.
//! Amounts are integer cents.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::ConfigFile;

/// The offer in `.moss/config.toml`, read apart from the other services so a
/// mistake in it never costs the site its forms or its other services: a valid
/// offer, or the first problem with it. `None` when the table is absent or switched off.
pub fn read(config: &ConfigFile) -> Option<Result<BookingService, String>> {
    let table = config.section(&["services", "booking"])?;
    let offer: Result<BookingService, String> = table.clone().try_into().map_err(|e| format!("[services.booking]: {e}"));
    if offer.as_ref().is_ok_and(|o| o.enabled == Some(false)) {
        return None;
    }
    Some(offer.and_then(|o| o.problem().map_or(Ok(o), Err)))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BookingService {
    /// Where the booking page calls. It is written into the description too,
    /// so the page and the service read one address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing)]
    pub enabled: Option<bool>,
    pub timezone: String,
    pub currency: String,
    pub services: BTreeMap<String, Offer>,
    pub open: BTreeMap<String, Vec<String>>,
    pub start_every: u32,
    pub notice_hours: u32,
    pub horizon_days: u32,
    pub cancel: Cancel,
    pub pages: Pages,
}

/// One bookable service: hourly (`per_hour` and `hours`) or fixed (`minutes` and `price`).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_hour: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hours: Option<Hours>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// A paid service may take a share as a hold first, and the balance before the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deposit: Option<Deposit>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Deposit {
    pub percent: u32,
    pub hold_hours: u32,
    pub balance_days_before: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hours {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    pub full_before_hours: u32,
    pub late_percent: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pages {
    pub book: String,
    pub done: String,
    pub manage: String,
    /// The page staff open from the link in each booking email, to decline it;
    /// needed when a service takes a deposit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desk: Option<String>,
}

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn id_ok(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn field_ok(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 40 && bytes.next().is_some_and(|b| b.is_ascii_lowercase()) && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `HH:MM` as minutes after midnight; `24:00` only as a window's end.
fn clock(text: &str, end: bool) -> Option<u32> {
    let shaped = text.len() == 5 && text.bytes().enumerate().all(|(i, b)| if i == 2 { b == b':' } else { b.is_ascii_digit() });
    let (h, m): (u32, u32) = (text.get(..2)?.parse().ok()?, text.get(3..)?.parse().ok()?);
    (shaped && m < 60 && (h < 24 || (end && h == 24 && m == 0))).then_some(h * 60 + m)
}

impl BookingService {
    /// The first thing that makes this offer unusable, as the author should read it.
    pub fn problem(&self) -> Option<String> {
        let at = "[services.booking]";
        if let Some(problem) = self.endpoint.as_deref().and_then(|e| super::endpoint_problem("booking", e)) {
            return Some(problem);
        }
        if !crate::build::feeds::calendar::zone::is_known(&self.timezone) {
            return Some(format!("{at} timezone '{}' is not a time zone name, such as \"America/Los_Angeles\"", self.timezone));
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_lowercase()) {
            return Some(format!("{at} currency '{}' must be a lowercase ISO code, such as \"usd\"", self.currency));
        }
        if self.services.is_empty() {
            return Some(format!("{at} lists no services"));
        }
        for (id, s) in &self.services {
            let here = format!("{at} service '{id}'");
            if !id_ok(id) {
                return Some(format!("{here}: an id is 1 to 40 lowercase letters, digits and hyphens"));
            }
            match (s.per_hour, &s.hours, s.minutes, s.price) {
                (Some(_), Some(h), None, None) if h.min >= 1 && h.min <= h.max => {}
                (Some(_), Some(_), None, None) => return Some(format!("{here}: hours.min must be at least 1 and no more than hours.max")),
                (None, None, Some(m), Some(_)) if m > 0 => {}
                _ => return Some(format!("{here}: give per_hour and hours for an hourly service, or minutes and price for a fixed one")),
            }
            if let Some((name, _)) = s.fields.iter().find(|(name, need)| !field_ok(name) || !matches!(need.as_str(), "required" | "optional")) {
                return Some(format!("{here}: field '{name}' needs a name of lowercase letters, digits and underscores, starting with a letter, and the value \"required\" or \"optional\""));
            }
            if let Some(name) = s.labels.keys().find(|name| !s.fields.contains_key(*name)) {
                return Some(format!("{here}: labels.{name} names no field"));
            }
            if let Some(d) = &s.deposit {
                if s.price == Some(0) {
                    return Some(format!("{here}: a free service takes no deposit"));
                }
                if !(1..=99).contains(&d.percent) {
                    return Some(format!("{here}: deposit.percent must be from 1 to 99"));
                }
                if self.pages.desk.is_none() {
                    return Some(format!("{at} pages.desk is needed for a service with a deposit: staff decline a booking from that page"));
                }
            }
        }
        for (day, windows) in &self.open {
            if !DAYS.contains(&day.as_str()) {
                return Some(format!("{at} open.{day}: days are mon, tue, wed, thu, fri, sat and sun"));
            }
            for w in windows {
                let parsed = w.split_once('-').and_then(|(a, b)| Some((clock(a, false)?, clock(b, true)?)));
                if !parsed.is_some_and(|(a, b)| a < b) {
                    return Some(format!("{at} open.{day}: '{w}' must be a window such as \"09:00-17:00\""));
                }
            }
        }
        if self.open.values().all(Vec::is_empty) {
            return Some(format!("{at} open lists no time a booking may start"));
        }
        if self.start_every == 0 || self.horizon_days == 0 {
            return Some(format!("{at} start_every and horizon_days must be more than 0"));
        }
        if self.cancel.late_percent > 100 {
            return Some(format!("{at} cancel.late_percent must be from 0 to 100"));
        }
        let desk = self.pages.desk.iter().map(|p| ("desk", p));
        for (key, path) in [("book", &self.pages.book), ("done", &self.pages.done), ("manage", &self.pages.manage)].into_iter().chain(desk) {
            // `//host/` would send a client who has just paid to another site, and
            // browsers drop tabs and line breaks, so `/\t/host/` would become one.
            if !path.starts_with('/') || path.starts_with("//") || path.chars().any(|c| c == '\\' || c.is_control() || c.is_whitespace()) {
                return Some(format!("{at} pages.{key} '{path}' must be a path on this site, starting with a single /"));
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "booking_tests.rs"]
mod tests;
