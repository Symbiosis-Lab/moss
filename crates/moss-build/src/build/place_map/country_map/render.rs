//! A country map as HTML: inline SVG with one path per country, a legend, the
//! as-of stamp, the panel the site script fills, and, when embedded on another
//! page, the rows as a list grouped by level.

use super::matching::Atlas;
use super::projection::{self, NORTH, SOUTH};
use super::resolve::Model;
use super::search;
use crate::build::served_path::ServedPath;
use crate::i18n::{t, Language};
use moss_core::media::html_escape;
use std::fmt::Write;

/// The viewBox width. Only its ratio to a rendered map matters: the shapes were
/// simplified for a map about 1,300 px wide, so a smaller box loses nothing.
const WIDTH: f64 = 1000.0;

/// Where the map is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place<'a> {
    /// On its own page, which goes on to show its table.
    OwnPage,
    /// On another page, which gets the rows as a list as well.
    Embedded,
    /// On another page that places the list itself with `style:levels`; the
    /// key ties that list to this map.
    EmbeddedListApart(&'a str),
}

fn num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A ring as a relative path: absolute start, then deltas, so repeated digits compress.
fn ring_path(ring: &[(f64, f64)], project: &impl Fn(f64, f64) -> (f64, f64)) -> String {
    let mut d = String::new();
    let (mut px, mut py) = (0.0, 0.0);
    for (i, &(lon, lat)) in ring.iter().enumerate() {
        let (x, y) = project(lon, lat);
        let (x, y) = ((x * 10.0).round() / 10.0, (y * 10.0).round() / 10.0);
        if i == 0 {
            let _ = write!(d, "M{} {}", num(x), num(y));
        } else {
            let _ = write!(d, "{}{} {}", if i == 1 { "l" } else { " " }, num(x - px), num(y - py));
        }
        (px, py) = (x, y);
    }
    d.push('z');
    d
}

fn rings_path(rings: &[Vec<(f64, f64)>], project: &impl Fn(f64, f64) -> (f64, f64)) -> String {
    rings.iter().map(|r| ring_path(r, project)).collect::<Vec<_>>().join("")
}

/// The box the map frames when this country is chosen: its rings' union, leaving out
/// outlying islands (rings under a quarter of the largest ring's box) so they
/// do not drag the frame away from the mainland.
fn frame_box(rings: &[Vec<(f64, f64)>], project: &impl Fn(f64, f64) -> (f64, f64)) -> String {
    let boxes: Vec<[f64; 4]> = rings
        .iter()
        .map(|ring| {
            ring.iter().map(|&(lon, lat)| project(lon, lat)).fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, (x, y)| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)])
        })
        .collect();
    let area = |b: &[f64; 4]| (b[2] - b[0]) * (b[3] - b[1]);
    let largest = boxes.iter().map(area).fold(0.0, f64::max);
    let [x0, y0, x1, y1] = boxes.iter().filter(|b| area(b) >= largest * 0.25).fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |u, b| [u[0].min(b[0]), u[1].min(b[1]), u[2].max(b[2]), u[3].max(b[3])]);
    format!("{} {} {} {}", num(x0), num(y0), num(x1), num(y1))
}

fn graticule(project: &impl Fn(f64, f64) -> (f64, f64)) -> String {
    let mut d = String::new();
    let mut lat = -30.0;
    while lat <= 60.0 {
        let (x1, y) = project(-180.0, lat);
        let (x2, _) = project(180.0, lat);
        let _ = write!(d, "M{} {}H{}", num(x1), num(y), num(x2));
        lat += 30.0;
    }
    let mut lon = -150.0;
    while lon <= 150.0 {
        let mut lat = SOUTH;
        let mut first = true;
        while lat <= NORTH + 1e-9 {
            let (x, y) = project(lon, lat);
            let _ = write!(d, "{}{} {}", if first { "M" } else { "L" }, num(x), num(y));
            first = false;
            lat += 3.0;
        }
        lon += 30.0;
    }
    d
}

pub(crate) fn render(model: &Model, title: &str, date: Option<&str>, lang: Language, place: Place<'_>) -> String {
    let atlas = Atlas::get();
    let (height, project) = projection::frame(WIDTH);
    let n = model.spec.levels.len();
    let by_country: std::collections::HashMap<usize, &super::resolve::Row> = model.rows.iter().map(|r| (r.country, r)).collect();
    let stamp = date.map(|d| (d, t(lang, "country_map_as_of").replace("{date}", &crate::build::components::date::format_display_date(d, lang, None))));

    let vars = shares(n);

    let search_index = ServedPath::for_place_map_asset(&atlas.shapes.digest, search::FILE).map(|p| format!("/{}", p.as_str())).unwrap_or_default();
    let key = match place {
        Place::EmbeddedListApart(key) => html_escape(key),
        _ => String::new(),
    };
    let mut out = format!(r#"<figure class="moss-country-map" data-moss-country-map="{key}" data-not-rated="{}""#, html_escape(t(lang, "country_map_not_rated")));
    if model.spec.search {
        let _ = write!(out, r#" data-search="{}""#, html_escape(&search_index));
    }
    let _ = write!(out, r#" style="{vars}">"#);
    if model.spec.search {
        let _ = write!(
            out,
            r##"<a class="moss-country-search" href="#moss-country-rows" data-count-one="{}" data-count-other="{}" data-none="{}" data-see-list="{}">{}</a>"##,
            html_escape(t(lang, "country_map_result_one")),
            html_escape(t(lang, "country_map_result_other")),
            html_escape(t(lang, "country_map_no_match")),
            html_escape(t(lang, "country_map_see_list")),
            html_escape(t(lang, "country_map_search")),
        );
    }
    let _ = write!(out, r#"<div class="moss-country-stage"><div class="moss-country-scroll"><svg viewBox="0 0 {} {}" role="img" aria-label="{}">"#, num(WIDTH), num(height), html_escape(title));
    let _ = write!(out, r#"<path class="moss-country-graticule" d="{}"/><g class="moss-country-land">"#, graticule(&project));
    let _ = write!(out, r#"<path class="moss-country-disputed" d="{}"/>"#, rings_path(&atlas.shapes.disputed, &project));
    for (i, c) in atlas.shapes.countries.iter().enumerate() {
        if c.rings.is_empty() {
            continue;
        }
        let level = by_country.get(&i).and_then(|r| r.level).map(|l| format!(r#" data-level="{}" style="--i:{l}""#, l + 1)).unwrap_or_default();
        let _ = write!(out, r#"<path class="moss-country" data-slug="{}" data-name="{}"{level} data-box="{}" d="{}"/>"#, atlas.slug_of(i), html_escape(&c.name_long), frame_box(&c.rings, &project), rings_path(&c.rings, &project));
    }
    out.push_str("</g></svg></div>");
    out.push_str(r#"<div class="moss-country-panel" role="region" aria-live="polite" hidden></div></div>"#);

    out.push_str(r#"<ul class="moss-country-legend">"#);
    for (i, level) in model.spec.levels.iter().enumerate() {
        let _ = write!(out, r#"<li>{}{}</li>"#, swatch(i), html_escape(&level.name));
    }
    out.push_str("</ul>");
    if let Some((raw, text)) = &stamp {
        let _ = write!(out, r#"<p class="moss-country-stamp"><time datetime="{}">{}</time></p>"#, html_escape(raw), html_escape(text));
    }
    if place == Place::Embedded {
        out.push_str(&list(model, lang, ""));
    } else if place == Place::OwnPage && model.spec.search {
        out.push_str(r#"<span id="moss-country-rows"></span>"#);
    }
    let as_of = stamp.as_ref().map(|(_, text)| text.as_str());
    for r in &model.rows {
        let name = html_escape(&r.label);
        let level = r.level.map(|l| (swatch(l), model.spec.levels[l].name.as_str(), model.spec.levels[l].text.as_deref()));
        let body = if !r.columns.is_empty() {
            let rows: String = r.columns.iter().map(|(k, v)| format!("<dt>{}</dt><dd>{}</dd>", html_escape(k), html_escape(v))).collect();
            format!("<dl>{rows}</dl>")
        } else {
            level.as_ref().and_then(|(_, _, text)| *text).map(|text| format!("<p>{}</p>", html_escape(text))).unwrap_or_default()
        };
        let level_line = level.as_ref().map_or_else(|| html_escape(t(lang, "country_map_not_rated")), |(sw, name, _)| format!("{sw}{}", html_escape(name)));
        let _ = write!(out, r#"<template data-slug="{}">{}"#, atlas.slug_of(r.country), panel(lang, &name, &level_line, &body, as_of, model.spec.action.as_ref(), Some(&r.label)));
        out.push_str("</template>");
    }
    let unrated = model.spec.unrated.as_deref().map(|text| format!("<p>{}</p>", html_escape(text))).unwrap_or_default();
    let _ = write!(out, r#"<template data-not-rated>{}</template>"#, panel(lang, "", &html_escape(t(lang, "country_map_not_rated")), &unrated, as_of, model.spec.action.as_ref(), None));
    out.push_str("</figure>");
    out
}

/// Each level's share of the colour ramp, set inline wherever its swatches draw.
fn shares(n: usize) -> String {
    let mut vars = String::new();
    for i in 1..=n {
        let share = if n > 1 { (i - 1) * 100 / (n - 1) } else { 0 };
        let _ = write!(vars, "--moss-country-share-{i}:{share}%;");
    }
    vars
}

fn swatch(level: usize) -> String {
    format!(r#"<span class="moss-country-swatch" data-level="{}" aria-hidden="true"></span>"#, level + 1)
}

/// The characters `encodeURIComponent` leaves alone.
const URI_COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

/// What a panel shows for a chosen country. The action link is filled for a
/// named country; for an unrated one it keeps `{country}` in `data-link` for the
/// script to fill with the name of the country tapped.
#[allow(clippy::too_many_arguments)]
fn panel(lang: Language, name: &str, level_line: &str, body: &str, as_of: Option<&str>, action: Option<&super::spec::Action>, country: Option<&str>) -> String {
    let mut out = format!(
        r#"<div class="moss-country-panel-head"><h3 class="moss-country-panel-name">{name}</h3><button class="moss-country-panel-close" type="button" aria-label="{}">&times;</button></div><p class="moss-country-panel-level">{level_line}</p>{body}"#,
        html_escape(t(lang, "close"))
    );
    if let Some(text) = as_of {
        let _ = write!(out, r#"<p class="moss-country-stamp">{}</p>"#, html_escape(text));
    }
    if let Some(action) = action.filter(|a| safe_link(&a.link)) {
        let link = match country {
            Some(c) => format!(r#"href="{}""#, html_escape(&action.link.replace("{country}", &percent_encoding::utf8_percent_encode(c, URI_COMPONENT).to_string()))),
            None => format!(r#"data-link="{}""#, html_escape(&action.link)),
        };
        let _ = write!(out, r#"<p><a {link}>{}</a></p>"#, html_escape(&action.label));
    }
    out
}

/// An author's link is followed only when it is a path or an http(s), mailto or fragment address.
fn safe_link(link: &str) -> bool {
    let l = link.trim_start().to_ascii_lowercase();
    ["http:", "https:", "mailto:", "/", ".", "#"].iter().any(|p| l.starts_with(p))
}

/// The rows grouped by level, lowest first, each an anchor the panel opens from.
/// Apart from its map (`key` set) it carries the map's shares for its swatches.
pub(crate) fn list(model: &Model, lang: Language, key: &str) -> String {
    let atlas = Atlas::get();
    let mut out = if key.is_empty() {
        String::from(r#"<div class="moss-country-list" id="moss-country-rows">"#)
    } else {
        format!(r#"<div class="moss-country-list" id="moss-country-rows" data-moss-country-list="{}" style="{}">"#, html_escape(key), shares(model.spec.levels.len()))
    };
    for g in 0..=model.spec.levels.len() {
        let level = (g < model.spec.levels.len()).then_some(g);
        let rows: Vec<_> = model.rows.iter().filter(|r| r.level == level).collect();
        if rows.is_empty() {
            continue;
        }
        let title = level.map_or_else(|| t(lang, "country_map_not_rated").to_string(), |l| model.spec.levels[l].name.clone());
        let _ = write!(out, r#"<section class="moss-country-group"><p class="moss-country-group-title">{}{}</p><ul>"#, level.map(swatch).unwrap_or_default(), html_escape(&title));
        for r in rows {
            let s = atlas.slug_of(r.country);
            let _ = write!(out, r##"<li id="{s}"><a href="#{s}">{}</a></li>"##, html_escape(&r.label));
        }
        out.push_str("</ul></section>");
    }
    out.push_str("</div>");
    out
}
