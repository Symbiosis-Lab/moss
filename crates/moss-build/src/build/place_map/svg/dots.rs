//! Dots of a static place map that link to their place pages.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::text::xml_escape;
use super::super::places_data;
use super::{dot, PlaceMapTarget, ResolvedPlace, Writer};

/// Place page URLs a figure's dots link to, and the page the figure is on.
#[derive(Clone, Copy)]
pub(super) struct DotLinks<'a> {
    pub(super) urls: &'a BTreeMap<String, String>,
    pub(super) own_page: &'a str,
}

impl<'a> DotLinks<'a> {
    /// A place's page, unless that is the page the map is drawn on.
    pub(super) fn url(&self, key: &str) -> Option<&'a str> {
        let url = self.urls.get(key)?.as_str();
        let own = places_data::page_url(self.own_page);
        (url.trim_matches('/') != own.trim_matches('/')).then_some(url)
    }
}

impl Writer<'_> {
    pub(super) fn any_dot_link(&self, target: &PlaceMapTarget) -> bool {
        let Some(links) = self.dot_links else { return false };
        target.marker_places().any(|place| links.url(&place.key).is_some())
    }

    /// Opens the link around a dot when one of the places drawn there has a
    /// page, with a ring that shows only while the link has keyboard focus.
    /// The dot's own look is untouched. Returns whether a link was opened.
    pub(super) fn open_dot_link(&mut self, group: &[&ResolvedPlace], x: i32, y: i32, ring: f64, filled: bool) -> bool {
        let Some(links) = self.dot_links.filter(|_| self.linked) else { return false };
        let Some(url) = group.iter().find_map(|place| links.url(&place.key)) else { return false };
        let mut names: Vec<&str> = Vec::new();
        for place in group {
            if !names.contains(&place.display.as_str()) {
                names.push(&place.display);
            }
        }
        let inner = if filled {
            format!("{}{}", dot(x, y, 7.0, "var(--moss-place-marker-casing, #ffffff)", ""), dot(x, y, 4.0, "var(--moss-place-marker, #2d5a2d)", ""))
        } else {
            String::new()
        };
        write!(
            self.output,
            "<a href=\"{}\" aria-label=\"{}\"><g data-map-focus>{}{inner}</g>",
            xml_escape(url),
            xml_escape(&names.join(", ")),
            dot(x, y, ring, "var(--moss-place-focus, #1b4fd8)", ""),
        )
        .expect("writing to String cannot fail");
        true
    }

    /// Closes [`Self::open_dot_link`] with a transparent target of at least
    /// 24 by 24 CSS px, so a small dot is still easy to hit.
    pub(super) fn close_dot_link(&mut self, x: i32, y: i32) {
        self.output.push_str(&dot(x, y, 12.0, "transparent", ""));
        self.output.push_str("</a>");
    }
}
