//! Dots of a static place map that link to their place pages.

use std::collections::BTreeMap;
use std::fmt::Write;

use super::text::xml_escape;
use super::super::places_data;
use super::{clamp_dot, dot, dot_reach, marker_radius, snap, PlaceMapTarget, Precision, Projection, ResolvedPlace, Writer};

/// Place page URLs a figure's dots link to, and the page the figure is on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DotLinks<'a> {
    pub(crate) urls: &'a BTreeMap<String, String>,
    pub(crate) own_page: &'a str,
    pub(crate) parents: &'a BTreeMap<String, String>,
    pub(crate) gazetteer: &'a crate::vault::places::Gazetteer,
    pub(crate) namespace: &'a str,
    pub(crate) explorer: bool,
}

impl<'a> DotLinks<'a> {
    /// A place's page, unless that is the page the map is drawn on.
    pub(super) fn url(&self, key: &str) -> Option<&'a str> {
        let url = self.urls.get(key)?.as_str();
        let own = places_data::page_url(self.own_page);
        (url.trim_matches('/') != own.trim_matches('/')).then_some(url)
    }

    fn ancestors(&self, key: &str) -> Vec<String> {
        let mut chain = vec![key.to_string()];
        while let Some(parent) = self.parents.get(chain.last().unwrap()) {
            let key = self.gazetteer.term_key(self.namespace, parent);
            if chain.contains(&key) { break; }
            chain.push(key);
        }
        chain
    }

    /// A group's link: the nearest place page all its places share, else the
    /// explorer, else the first place page.
    fn group_url(&self, group: &[&ResolvedPlace]) -> Option<String> {
        if group.len() > 1 {
            let chains: Vec<Vec<String>> = group.iter().map(|place| self.ancestors(&place.key)).collect();
            let shared = chains[0].iter().filter(|key| chains[1..].iter().all(|chain| chain.contains(key)));
            if let Some(url) = shared.into_iter().find_map(|key| self.url(key)) { return Some(url.to_string()); }
            if self.explorer {
                let url = places_data::root_url(self.namespace);
                if url != places_data::page_url(self.own_page) { return Some(url); }
            }
        }
        group.iter().find_map(|place| self.url(&place.key)).map(str::to_string)
    }
}

impl Writer<'_> {
    pub(super) fn any_dot_link(&self, target: &PlaceMapTarget) -> bool {
        let Some(links) = self.dot_links else { return false };
        target.marker_places().any(|place| links.url(&place.key).is_some())
    }

    /// Opens one link around a group's dots when one of its places has a
    /// page, with a ring round each dot that shows only while the link has
    /// keyboard focus. The dots' own look is untouched. Returns whether a
    /// link was opened.
    pub(super) fn open_dot_link(&mut self, group: &[&ResolvedPlace], dots: &[(i32, i32)], ring: f64, filled: bool) -> bool {
        let Some(links) = self.dot_links.filter(|_| self.linked) else { return false };
        let Some(url) = links.group_url(group) else { return false };
        let mut names: Vec<&str> = Vec::new();
        for place in group {
            if !names.contains(&place.display.as_str()) {
                names.push(&place.display);
            }
        }
        write!(self.output, "<a href=\"{}\" aria-label=\"{}\"><g data-map-focus>", xml_escape(&url), xml_escape(&names.join(", "))).expect("writing to String cannot fail");
        for &(x, y) in dots {
            self.output.push_str(&dot(x, y, ring, "var(--moss-place-focus, #1b4fd8)", ""));
            if filled {
                self.output.push_str(&dot(x, y, 7.0, "var(--moss-place-marker-casing, #ffffff)", ""));
                self.output.push_str(&dot(x, y, 4.0, "var(--moss-place-marker, #2d5a2d)", ""));
            }
        }
        self.output.push_str("</g>");
        true
    }

    /// Closes [`Self::open_dot_link`] with a transparent target of at least
    /// 24 by 24 CSS px on each dot, so a small dot is still easy to hit.
    pub(super) fn close_dot_link(&mut self, dots: &[(i32, i32)]) {
        for &(x, y) in dots {
            self.output.push_str(&dot(x, y, HIT_RADIUS, "transparent", ""));
        }
        self.output.push_str("</a>");
    }
}

const HIT_RADIUS: f64 = 12.0;

/// Places that share one link: those drawn on the same pixel, and linked dots
/// whose hit targets would overlap on the narrowest map. Each distinct dot is
/// still drawn where its place is.
pub(super) struct MarkerGroup<'a> {
    /// The first place's projected point, before any clamping.
    pub(super) point: (f64, f64),
    pub(super) places: Vec<&'a ResolvedPlace>,
    /// Each distinct dot centre, with the first place drawn there.
    pub(super) dots: Vec<(i32, i32, &'a ResolvedPlace)>,
}

pub(super) fn marker_groups<'a>(target: &'a PlaceMapTarget, projection: &Projection, linked: bool) -> Vec<MarkerGroup<'a>> {
    let nodes: Vec<_> = target.markers_in_draw_order().into_iter().filter_map(|place| {
        let point = projection.project(place.point()?)?;
        let shape = match place.precision {
            Precision::Exact | Precision::City => 0,
            Precision::Region => 1,
            Precision::Country => 2,
        };
        let centre = if shape == 0 { clamp_dot(point, marker_radius(place.precision) + 1.0) } else { point };
        Some((place, (snap(centre.0), snap(centre.1)), shape, point))
    }).collect();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, &(_, (x, y), shape, _)) in nodes.iter().enumerate() {
        let mut joined = vec![i];
        groups.retain(|group| {
            let overlaps = group.iter().any(|&j| {
                let (_, (gx, gy), other_shape, _) = nodes[j];
                shape == other_shape && ((x, y) == (gx, gy)
                    || (linked && shape == 0 && f64::from(x - gx).hypot(f64::from(y - gy)) < dot_reach(HIT_RADIUS * 2.0)))
            });
            if overlaps { joined.extend(group); }
            !overlaps
        });
        joined.sort_unstable();
        groups.push(joined);
    }
    groups.sort_by_key(|group| group[0]);
    groups.into_iter().map(|group| {
        let mut dots: Vec<(i32, i32, &ResolvedPlace)> = Vec::new();
        for &i in &group {
            let (place, (x, y), _, _) = nodes[i];
            if !dots.iter().any(|&(dx, dy, _)| (dx, dy) == (x, y)) { dots.push((x, y, place)); }
        }
        MarkerGroup { point: nodes[group[0]].3, places: group.into_iter().map(|i| nodes[i].0).collect(), dots }
    }).collect()
}
