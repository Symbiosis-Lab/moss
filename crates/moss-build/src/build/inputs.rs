//! What a build is as of beyond its source files: the moment it runs and the
//! site's time zone, which together fix the build day every past/upcoming
//! decision and edition date reads. Each is read once per build and handed
//! down, so two machines that build the same source on the same day publish
//! the same bytes.

use crate::build::types::ParsedDocument;

/// The moment a build is as of: `SOURCE_DATE_EPOCH` (seconds since 1970, the
/// reproducible-builds convention) when set, otherwise the clock.
pub(crate) fn build_instant() -> jiff::Timestamp {
    instant_from(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref())
}

fn instant_from(source_date_epoch: Option<&str>) -> jiff::Timestamp {
    source_date_epoch
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
        .unwrap_or_else(jiff::Timestamp::now)
}

/// The site's time zone: the home page's `cascade: {timezone: …}`, when it
/// names a zone.
pub(crate) fn site_zone(docs: &[ParsedDocument]) -> Option<&str> {
    let home = docs.iter().find(|d| d.url_path == "index.html")?;
    let zone = home.cascade.as_ref()?.get("timezone")?.as_str()?.trim();
    crate::build::feeds::calendar::zone::is_known(zone).then_some(zone)
}

/// The build's "now" for every past/upcoming decision: the start of the day
/// `instant` falls on in `zone` (UTC when the site names none). A day, not the
/// minute, and never the build machine's own zone, so every machine that builds
/// the same source on the same day decides the same. An event hero alone swaps
/// itself out in the browser at the minute its event ends (`hero_from`).
pub(crate) fn effective_now(instant: jiff::Timestamp, zone: Option<&str>) -> chrono::NaiveDateTime {
    let tz = zone.and_then(crate::build::feeds::calendar::zone::load).unwrap_or(jiff::tz::TimeZone::UTC);
    let day = instant.to_zoned(tz).date();
    chrono::NaiveDate::from_ymd_opt(i32::from(day.year()), day.month() as u32, day.day() as u32)
        .unwrap_or_default()
        .and_time(chrono::NaiveTime::MIN)
}

/// [`effective_now`] for a site with no zone, as of [`build_instant`].
#[cfg(test)]
pub(crate) fn now() -> chrono::NaiveDateTime {
    effective_now(build_instant(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_day_is_the_sites_not_the_machines() {
        // 2026-03-14 02:00 UTC: still the 13th in New York, already the 14th in Tokyo.
        let instant = jiff::Timestamp::from_second(1_773_453_600).unwrap();
        let day = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_time(chrono::NaiveTime::MIN);
        assert_eq!(effective_now(instant, None), day(2026, 3, 14));
        assert_eq!(effective_now(instant, Some("America/New_York")), day(2026, 3, 13));
        assert_eq!(effective_now(instant, Some("Asia/Tokyo")), day(2026, 3, 14));
        assert_eq!(effective_now(instant, Some("Not/A_Zone")), day(2026, 3, 14));
    }

    #[test]
    fn the_build_is_as_of_source_date_epoch_when_one_is_given() {
        assert_eq!(instant_from(Some("1700000000")).as_second(), 1_700_000_000);
        for unset in [None, Some("not a number")] {
            let before = jiff::Timestamp::now();
            let read = instant_from(unset);
            assert!(before <= read && read <= jiff::Timestamp::now(), "{unset:?} reads the clock");
        }
    }

    #[test]
    fn the_site_zone_is_the_home_pages_cascade() {
        let page = |url: &str, cascade: &str| ParsedDocument {
            url_path: url.into(),
            cascade: Some(serde_yaml::from_str(cascade).unwrap()),
            ..Default::default()
        };
        assert_eq!(site_zone(&[page("events/index.html", "timezone: Asia/Tokyo")]), None);
        assert_eq!(site_zone(&[page("index.html", "timezone: ' Asia/Tokyo '")]), Some("Asia/Tokyo"));
        assert_eq!(site_zone(&[page("index.html", "timezone: Nowhere")]), None);
    }
}
