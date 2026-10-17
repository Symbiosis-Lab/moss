//! Calendar files for events.
//!
//! A page with `start:` is an event. Each event page gets its own "Add to
//! calendar" file, and each folder whose listing shows event pages gets a
//! subscribable `calendar.ics` of them.
//!
//! ## Served paths
//!
//! Both files sit beside the page output, so a relative reader of the page URL
//! and the file agree on the folder:
//!
//! ```text
//! Events/talk/index.html   → Events/talk/event.ics      (the page's own event)
//! Events/talk.html         → Events/talk.ics
//! Events/index.html        → Events/calendar.ics        (what its listing shows)
//! index.html               → calendar.ics               (what the home page lists)
//! ```
//!
//! A page directory holds `index.html` and `event.ics`; a pretty-URL page can
//! never be named `event.ics` or `calendar.ics`, so these cannot collide with
//! another page's output. A file of the same name in the site folder is copied
//! over the generated one (the author's file wins).
//!
//! The `.ics` files are published addresses: people subscribe to
//! `calendar.ics` and keep it. Do not rename them.
//!
//! ## Which folders
//!
//! A folder calendar holds exactly the events the folder's own listing shows,
//! chosen by the listing's selector (direct children, or nested ones when the
//! folder's `children_depth` reaches them). Only a folder with a home page of
//! its own has one: the subscribe link needs a page to live on, and a folder
//! moss indexes by itself has none.
//!
//! ## What is written
//!
//! `UID` is the page's `uid` at the site's host. Times with a `timezone:` are
//! written with `TZID` and a `VTIMEZONE`; times without one are floating local
//! time, and the build says so once. An all-day `end` is inclusive on the page
//! and exclusive in the file. `STATUS` is `CANCELLED` for `cancelled` and
//! `TENTATIVE` for `postponed`: iCalendar has no "postponed" value, and
//! `TENTATIVE` is the nearest honest one (the date is not settled). Other
//! statuses write none.

pub mod ics;
pub mod zone;

use std::collections::{BTreeMap, HashMap, HashSet};

use moss_core::event::EventTime;

use crate::build::served_path::{ServedPath, CALENDAR_ICS, EVENT_ICS};
use crate::build::site_url::SiteUrl;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;
use ics::Event;

fn raw_str<'a>(doc: &'a ParsedDocument, key: &str) -> Option<&'a str> {
    doc.raw_frontmatter.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn page_output(doc: &ParsedDocument) -> Option<ServedPath> {
    ServedPath::from_source(&doc.url_path).ok()
}

/// The folders a served page sits in, from the site root down:
/// `events/talk/index.html` is in `""`, `events` and `events/talk`.
fn ancestor_dirs(served: &str) -> impl Iterator<Item = &str> {
    std::iter::once("").chain(served.match_indices('/').map(move |(i, _)| &served[..i]))
}

/// Is this served path a calendar file the build generates? Used by the
/// publish gate: such a file has no source of its own, and disappears when the
/// author's `start:` or listing changes.
pub fn is_calendar_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == EVENT_ICS || name == CALENDAR_ICS
}

/// Something about a page's event fields the build had to drop or could not use.
#[derive(Default)]
struct Problems {
    bad_start: Vec<String>,
    bad_end: Vec<String>,
    bad_zone: Vec<String>,
    bad_online: Vec<String>,
}

fn warn_once(what: &str, pages: &[String]) {
    if pages.is_empty() {
        return;
    }
    let shown: Vec<&str> = pages.iter().take(5).map(String::as_str).collect();
    let more = pages.len().saturating_sub(shown.len());
    log::warn!("{what}: {}{}", shown.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() });
}

/// A value safe to put in a `UID`: ASCII letters, digits, `.`, `_` and `-`.
/// Anything else (spaces, Unicode, control characters) is replaced by a hash of
/// the original, which stays stable and cannot break the line.
fn uid_stem(raw: &str) -> String {
    let safe = !raw.is_empty() && raw.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if safe { raw.to_string() } else { crate::build::assets::paths::compute_binary_hash(raw.as_bytes()) }
}

/// An `online` value is a `CONFERENCE` URI only when it is a plain http(s) URL;
/// anything else (a multi-line value, another scheme) would write arbitrary
/// content into the file.
fn online_url(raw: &str) -> Option<String> {
    // The url crate silently drops line breaks inside a URL; a value that has any is not a plain address.
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host().is_some()).then(|| url.to_string())
}

fn year_of(t: &EventTime) -> i16 {
    match *t {
        EventTime::Date(y, ..) | EventTime::DateTime(y, ..) => y as i16,
    }
}

/// The event a page describes, or `None` when it has no usable `start`.
fn event_of(doc: &ParsedDocument, site_url: &SiteUrl, problems: &mut Problems) -> Option<Event> {
    if !doc.is_public_page() {
        return None;
    }
    let page = doc.source_path.clone().unwrap_or_else(|| doc.url_path.clone());
    let Some(ev) = doc.event.as_ref() else {
        if raw_str(doc, "start").is_some() {
            problems.bad_start.push(page);
        }
        return None;
    };
    let (start, end) = (ev.start, ev.end);
    if raw_str(doc, "end").is_some() && end.is_none() {
        problems.bad_end.push(page.clone());
    }
    let zone_name = ev.timezone.as_deref();
    let tzid = zone_name.filter(|z| zone::is_known(z)).map(str::to_string);
    if let (Some(name), None) = (zone_name, &tzid) {
        problems.bad_zone.push(format!("`{name}` ({page})"));
    }
    let status = match ev.status.as_deref().map(str::trim) {
        Some("cancelled") => Some("CANCELLED"),
        Some("postponed") => Some("TENTATIVE"),
        _ => None,
    };
    let online = ev.online.as_deref().map(str::trim).filter(|o| !o.is_empty()).and_then(|o| {
        let url = online_url(o);
        if url.is_none() {
            problems.bad_online.push(page.clone());
        }
        url
    });
    let stamp_day = doc
        .date
        .as_deref()
        .and_then(|d| EventTime::parse(d).ok())
        .unwrap_or(start)
        .day_key()
        .replace('-', "");
    let stem = uid_stem(&doc.uid.clone().unwrap_or_else(|| doc.url_path.clone()));
    let url = site_url
        .is_deployed()
        .then(|| crate::build::page::canonical::canonical_for_url_path(site_url, &doc.url_path).ok())
        .flatten();
    Some(Event {
        uid: format!("{stem}@{}", site_url.host()),
        summary: doc.title.clone(),
        start,
        end,
        tzid,
        status,
        location: Some(doc.location.join(", ")).filter(|l| !l.is_empty()),
        description: doc.description.clone().filter(|d| !d.trim().is_empty()),
        url,
        online,
        stamp: format!("{stamp_day}T000000Z"),
    })
}

/// A calendar file for `events`, with a `VTIMEZONE` for each zone they use.
fn render(name: &str, events: &[Event], subscribable: bool) -> String {
    let mut years: BTreeMap<&str, (i16, i16)> = BTreeMap::new();
    for e in events {
        let Some(z) = e.tzid.as_deref() else { continue };
        let (lo, hi) = (year_of(&e.start), e.end.as_ref().map_or(year_of(&e.start), year_of));
        let slot = years.entry(z).or_insert((lo, hi));
        slot.0 = slot.0.min(lo);
        slot.1 = slot.1.max(hi);
    }
    let zones: Vec<Vec<String>> = years
        .iter()
        .filter_map(|(z, (lo, hi))| Some(zone::vtimezone(z, &zone::load(z)?, lo.saturating_sub(1), hi.saturating_add(1))))
        .collect();
    ics::calendar_text(name, &zones, events, subscribable)
}

impl ParsedDocument {
    /// Whether this page's body ends with its own folder's calendar links.
    /// `render/html.rs` prints them whenever the page has a planned calendar
    /// (`folder_href`) and this holds; the embed pass relies on that and leaves
    /// a self-embed of the page's own folder without links, so they are never
    /// printed twice. A page with `children: false` shows no listing of its
    /// own, so its links belong under the listings it embeds instead.
    pub fn prints_folder_calendar_links(&self) -> bool {
        self.children != Some(false)
    }
}

struct PlannedEvent {
    path: ServedPath,
    event: Event,
}

struct PlannedFolder {
    path: ServedPath,
    name: String,
    events: Vec<Event>,
}

/// Every calendar file a build writes, decided once from the documents: which
/// pages are events, and which folders' listings show events. The writer and the
/// page links both read it, so they cannot disagree about a file.
#[derive(Default)]
pub struct CalendarPlan {
    /// Keyed by the event page's `url_path`.
    events: HashMap<String, PlannedEvent>,
    /// Keyed by the folder home page's `url_path`.
    folders: HashMap<String, PlannedFolder>,
}

impl CalendarPlan {
    /// Plans the calendars and warns, once each, about event fields it had to
    /// drop or could not use.
    pub fn build(documents: &[ParsedDocument], project: &ProjectStructure, site_url: &SiteUrl) -> Self {
        use crate::build::folder_embed::members::{is_folder_home, listed_unfiltered, own_dir};
        let mut problems = Problems::default();
        let mut plan = CalendarPlan::default();
        let mut floating = 0usize;
        // One parse per page; folders below look events up here.
        let mut parsed: HashMap<&str, Event> = HashMap::new();
        // Folders with at least one event page under them. A folder's listing can
        // only show events beneath it, so a folder outside this set has no calendar.
        let mut dirs_with_events: HashSet<String> = HashSet::new();
        for doc in documents {
            let Some(event) = event_of(doc, site_url, &mut problems) else { continue };
            let Some(page) = page_output(doc) else { continue };
            let Some(path) = ServedPath::for_event_ics(&page) else { continue };
            dirs_with_events.extend(ancestor_dirs(page.as_str()).map(str::to_string));
            if event.tzid.is_none() && !event.start.is_all_day() && doc.event.as_ref().is_some_and(|ev| ev.timezone.is_none()) {
                floating += 1;
            }
            parsed.insert(doc.url_path.as_str(), event.clone());
            plan.events.insert(doc.url_path.clone(), PlannedEvent { path, event });
        }

        for host in documents {
            // A synthesized folder index has no source file; a home page has one.
            let Some(dir) = own_dir(host).filter(|_| is_folder_home(host)) else { continue };
            if !dirs_with_events.contains(dir.as_str()) {
                continue;
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut events = Vec::new();
            let members = listed_unfiltered(host, documents, project).unwrap_or_else(|| {
                log::warn!("Folder `{}` lists a folder that does not exist, so its calendar file is left out", host.url_path);
                Vec::new()
            });
            for member in members {
                if let Some(e) = parsed.get(member.url_path.as_str()) {
                    if seen.insert(e.uid.clone()) {
                        events.push(e.clone());
                    }
                }
            }
            if events.is_empty() {
                continue;
            }
            events.sort_by(|a, b| (a.start.sort_key(), &a.uid).cmp(&(b.start.sort_key(), &b.uid)));
            plan.folders.insert(
                host.url_path.clone(),
                PlannedFolder { path: ServedPath::for_calendar(&dir), name: host.title.clone(), events },
            );
        }

        warn_once("Event pages whose `start:` is not a date or date and time (YYYY-MM-DD or YYYY-MM-DD HH:MM) are left out of the calendar files", &problems.bad_start);
        warn_once("Event pages whose `end:` is unreadable or before `start:` get no end in their calendar files", &problems.bad_end);
        warn_once("`timezone:` is not a time zone name (use Area/City), so these events' calendar times are floating", &problems.bad_zone);
        warn_once("`online:` is not an http(s) address, so it is left out of the calendar files", &problems.bad_online);
        if floating > 0 {
            log::warn!(
                "{floating} timed event(s) have no `timezone:`, so their calendar files carry floating local times \
                 that each calendar app reads in the viewer's own zone. Add `timezone: Area/City` to the event pages."
            );
        }
        plan
    }

    /// The event file URL for an event page.
    pub fn event_href(&self, doc: &ParsedDocument) -> Option<String> {
        self.events.get(&doc.url_path).map(|e| e.path.to_relative_url())
    }

    /// The calendar URL for a folder home page whose listing shows events.
    pub fn folder_href(&self, doc: &ParsedDocument) -> Option<String> {
        self.folders.get(&doc.url_path).map(|f| f.path.to_relative_url())
    }

    /// Write every planned file.
    pub fn emit(
        &self,
        output_dir: &std::path::Path,
        pending: &mut crate::build::manifest::PendingManifest,
    ) -> Result<(), String> {
        let mut write = |sp: &ServedPath, text: String| {
            crate::build::context::BuildContext::for_render(output_dir, pending)
                .emit_held(sp, text.into_bytes(), crate::build::manifest::HashBucket::Files)
                .map_err(|e| format!("Failed to emit {}: {e}", sp.as_str()))
        };
        for e in self.events.values() {
            write(&e.path, render(&e.event.summary, std::slice::from_ref(&e.event), false))?;
        }
        for f in self.folders.values() {
            write(&f.path, render(&f.name, &f.events, true))?;
        }
        Ok(())
    }

    /// The `<head>` alternate link for a page, or an empty string.
    pub fn head_link(&self, doc: &ParsedDocument, lang: crate::i18n::Language) -> String {
        let t = |key| crate::i18n::t(lang, key);
        let link = |title: &str, href: &str| format!("\n    <link rel=\"alternate\" type=\"text/calendar\" title=\"{title}\" href=\"{href}\">");
        let mut out = String::new();
        if let Some(h) = self.event_href(doc) {
            out.push_str(&link(t("add_to_calendar"), &h));
        }
        if let Some(h) = self.folder_href(doc) {
            out.push_str(&link(t("download_calendar"), &h));
        }
        out
    }

    /// The "Add to calendar" paragraph for an event page whose meta line did
    /// not carry the link.
    pub fn event_link_html(&self, href: &str, lang: crate::i18n::Language) -> String {
        links_paragraph(vec![format!(
            r#"<a class="moss-calendar-link" href="{href}" type="text/calendar">{}</a>"#,
            crate::i18n::t(lang, "add_to_calendar")
        )])
    }

    /// A folder page's download link and, on a deployed site, its `webcal://`
    /// subscribe link. Empty when the page has no calendar.
    pub fn folder_links_html(&self, doc: &ParsedDocument, site_url: &SiteUrl, lang: crate::i18n::Language) -> String {
        match self.folder_href(doc) {
            Some(href) => links_paragraph(folder_links(&href, site_url, lang)),
            None => String::new(),
        }
    }

    /// The links that follow an embedded listing of the folder whose home page
    /// is `home_url_path`: empty unless that folder has a calendar and at least
    /// one of its events has not ended at `now`.
    pub fn embed_links_html(&self, home_url_path: &str, site_url: &SiteUrl, lang: crate::i18n::Language, now: chrono::NaiveDateTime) -> String {
        let Some(folder) = self.folders.get(home_url_path) else { return String::new() };
        if !folder.events.iter().any(|e| crate::build::folder_embed::upcoming::span_is_upcoming(e.start, e.end, now)) {
            return String::new();
        }
        links_paragraph(folder_links(&folder.path.to_relative_url(), site_url, lang))
    }
}

fn folder_links(href: &str, site_url: &SiteUrl, lang: crate::i18n::Language) -> Vec<String> {
    let t = |key| crate::i18n::t(lang, key);
    let mut links = vec![format!(r#"<a class="moss-calendar-link" href="{href}" type="text/calendar">{}</a>"#, t("download_calendar"))];
    if site_url.is_deployed() {
        links.push(format!(
            r#"<a class="moss-calendar-link" href="webcal://{}{href}">{}</a>"#,
            site_url.host(),
            t("subscribe_to_calendar")
        ));
    }
    links
}

const LINKS_OPEN: &str = "\n<p class=\"moss-calendar-links\">";
const LINKS_CLOSE: &str = "</p>\n";

fn links_paragraph(links: Vec<String>) -> String {
    format!("{LINKS_OPEN}{}{LINKS_CLOSE}", links.join(" · "))
}

/// `html` without any calendar-links paragraph, for text that leaves the
/// page (a feed entry), where "Download calendar" has nothing to act on.
pub fn strip_links_paragraphs(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((before, after)) = rest.split_once(LINKS_OPEN) {
        out.push_str(before);
        rest = after.split_once(LINKS_CLOSE).map_or("", |(_, tail)| tail);
    }
    out.push_str(rest);
    out
}
