//! The floating nav island — the second, smaller navigation object that
//! appears part-way down a long page.
//!
//! A child module of `nav` rather than a sibling file for one reason: it
//! builds from `NavigationBuilder`'s private state, and a child module can
//! read its parent's privates. Keeping it out of `nav.rs` keeps that file
//! under the size the ratchet caps at, and matches how the island reads on
//! screen — related to the masthead, not part of it.

use super::{BreadcrumbSegment, NavigationBuilder};

/// What the island is for this page, decided once when the builder is set up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum IslandShape {
    /// The breadcrumb trail continued by the current page, plus the sections
    /// button; none when the page has no trail.
    #[default]
    Trail,
    /// A page that asked for its contents open but has no trail (the home
    /// page): the island with the page's title as its only crumb.
    TitleOnly(String),
    /// `[site].header = "none"`: the page's only chrome, with no trail.
    Pinned,
}

/// The `…` disclosure a folding breadcrumb trail opens, plus the separator
/// that follows it — ONE fragment for both trails (masthead and island), so
/// the two cannot drift. Both ship `hidden`: the fold script
/// (`breadcrumb-fold.ts`) unhides them only once the trail
/// has actually had to fold, so with JavaScript off neither exists.
///
/// No `aria-haspopup="menu"` and no `role="menu"` on the panel it opens:
/// both promise arrow-key roving focus, which a popover of plain links does
/// not implement. Claiming menu semantics and then not honouring them is
/// worse than not claiming them — a screen-reader user told "menu" presses
/// ArrowDown and nothing moves. `aria-expanded` already says the button
/// discloses something, which is the truth here.
pub(super) fn fold_more_fragment(class: &str, more_label: &str) -> String {
    format!(
        r#"<button type="button" class="{class}" hidden aria-expanded="false" aria-label="{more_label}">…</button><span class="breadcrumb-separator" data-trail-more-separator hidden>/</span>"#
    )
}

impl<'a> NavigationBuilder<'a> {
    /// Generates the floating nav island — the small bar that appears when the
    /// reader scrolls back up past the masthead.
    ///
    /// It is **not** the masthead pinned. It is a second, smaller object: the
    /// same breadcrumb trail continued one level (the current page becomes the
    /// last crumb), a sections button, and a reading-progress rule. No nav
    /// links, no theme toggle, no language switcher — those are set-once
    /// preferences that belong at the top of the document. No search either:
    /// the masthead has it and an inert icon in shipped chrome is worse than no
    /// icon.
    ///
    /// Reached only when the site opted in with `[site].floating_nav = true`
    /// (default off since 2026-08-30), and returns an
    /// empty string when the page has no breadcrumb trail: the homepage
    /// (nowhere to go "up" to) and any site that has turned breadcrumbs off.
    ///
    /// Whether an emitted island ever SHOWS is decided at runtime by
    /// `nav-island.ts`: only on a page with two or more section headings — a
    /// real contents table. That half of the rule
    /// lives in the script because heading depth is a rendered-DOM question it
    /// already answers for the sections panel, and duplicating it here would
    /// invite drift for the price of a few hundred `display: none` bytes.
    ///
    /// # What the emitted markup promises with JavaScript off
    ///
    /// Everything here is a link the masthead also carries, so nothing is
    /// reachable only through this bar. The island is `display: none` until
    /// `nav-island.ts` writes `data-shown` on it, so with no
    /// script the page behaves exactly as it does today. The `…` ships
    /// `hidden` because the fold may never need it; the sections button does
    /// not, because an emitted island that is never shown without a contents
    /// table is an island whose sections button always has something to open.
    pub fn generate_nav_island(&self) -> String {
        if self.island == IslandShape::Pinned {
            return self.island_frame(true, &[self.sections_button_html(), self.nav_icons_html()]);
        }
        // `nav` header mode carries no breadcrumb trail — a home segment
        // would reintroduce the brand link this mode exists to drop, in the
        // one nav surface that isn't gated on it. The Home link the masthead
        // already opens with covers the same "way back" job.
        let trail = self.breadcrumb_segments.as_deref().filter(|s| !s.is_empty());
        if self.header_mode == super::HeaderMode::Nav && trail.is_some() {
            return String::new();
        }
        // A page that asked for its contents open but has no trail (the home
        // page) still gets the island, with itself as the only crumb.
        let only;
        let segments = match (trail, &self.island) {
            (Some(trail), _) => trail,
            (None, IslandShape::TitleOnly(title)) => {
                only = [BreadcrumbSegment { title: title.clone(), url: String::new(), is_current: true }];
                &only[..]
            }
            _ => return String::new(),
        };

        let home_path = self.home_path();
        let logo_html = self.logo_html();
        let lang = self.current_lang;
        let separator = r#"<span class="breadcrumb-separator">/</span>"#;

        // The trail is the masthead's markup continued: same
        // `site-name` / `breadcrumb-segment` / `breadcrumb-label` /
        // `breadcrumb-separator` classes, same order, so "it matches the nav
        // bar" is true by construction rather than by eye. The one
        // difference is the tail: `generate_navigation` skips
        // `segment.is_current` because the page title is directly below it on
        // screen. In the island it is not, so the island appends it as the
        // final crumb with `aria-current="page"`.
        let mut crumbs: Vec<String> = Vec::new();
        for segment in segments {
            let is_home = !segment.is_current && segment.url == home_path;
            let content = if is_home {
                format!("{}{}", logo_html, segment.title)
            } else {
                format!(r#"<span class="breadcrumb-label">{}</span>"#, segment.title)
            };
            // `data-island-crumb` is the fold algorithm's handle on the trail.
            // It is on EVERY crumb, including the two that can never fold, so
            // the script can index the list positionally (first and last
            // survive; the middle is sacrificed left to right) instead of
            // re-deriving which is which from class names.
            if segment.is_current {
                // Not a link: this is the page you are already on. It is also
                // the only crumb allowed to truncate — an ancestor either fits
                // whole or folds away, because half an ancestor name tells the
                // reader nothing.
                //
                // Because it is the one crumb that CAN be cut, it is also the
                // one that needs a way to say what was cut off:
                // `breadcrumb-hint.ts` promotes `data-hint-label`
                // to a real tooltip exactly while the label is truncated, and
                // drops it again when the window widens — reveal
                // the full text only when it is genuinely cut off.
                crumbs.push(format!(
                    r#"<span class="breadcrumb-segment moss-nav-island-current" aria-current="page" data-island-crumb data-hint-label="{}">{}</span>"#,
                    crate::build::page::meta::escape_html_attr(&segment.title),
                    content
                ));
            } else {
                crumbs.push(format!(
                    r#"<a href="{}" class="{}" data-island-crumb>{}</a>"#,
                    segment.url,
                    if is_home { "site-name" } else { "breadcrumb-segment" },
                    content
                ));
            }
        }

        // The `…` button and its separator live immediately after the first
        // crumb, which is where the fold opens up: ancestors are dropped from
        // the middle outward, so the gap always starts there.
        let more_label =
            crate::build::page::meta::escape_html_attr(crate::i18n::t(lang, "nav_hidden_levels"));
        // The masthead's trail folds with the same fragment (see
        // `fold_more_fragment` above) — only the class differs.
        let more = fold_more_fragment("moss-nav-island-more", &more_label);

        // Fewer than three crumbs means there is no middle to sacrifice — the
        // first and the current page both always survive — so such a trail
        // carries no `…` at all rather than a button that can never unhide.
        let trail_body = if crumbs.len() < 3 {
            crumbs.join(separator)
        } else {
            format!("{}{separator}{more}{}", crumbs[0], crumbs[1..].join(separator))
        };

        let trail_label =
            crate::build::page::meta::escape_html_attr(crate::i18n::t(lang, "nav_breadcrumb"));

        let sections_button = self.sections_button_html();

        self.island_frame(
            false,
            &[
                format!(r#"<nav class="moss-nav-island-trail" aria-label="{trail_label}">{trail_body}</nav>"#),
                format!(r#"<span class="moss-nav-island-actions">{sections_button}</span>"#),
                r#"<span class="moss-nav-island-progress"><span class="moss-nav-island-progress-fill"></span></span>"#.to_string(),
            ],
        )
    }

    /// The island's scaffolding: the bar around `bar`, and the panels beside
    /// it. The panels are populated by the script and empty in the emitted
    /// HTML. They are siblings of the bar rather than children because the bar
    /// clips its own overflow, which makes their containing block
    /// `.moss-nav-island`; `nav-island.ts` places them against THAT box. The
    /// `…` panel lists the levels a folding trail dropped, so a pinned island,
    /// which has no trail, has only the sections panel.
    fn island_frame(&self, pinned: bool, bar: &[String]) -> String {
        // Without scripting nothing writes `data-shown`, and a site with no
        // header has no other place for its language links: a pinned island
        // shows as a plain bar at the top of the page, without the sections
        // button and panel the script drives.
        let no_script = if pinned {
            "<noscript><style>.moss-nav-island[data-header=none]{display:block;position:absolute}.moss-nav-island[data-header=none] .moss-nav-island-sections,.moss-nav-island[data-header=none] [data-island-menu]{display:none}</style></noscript>"
        } else {
            ""
        };
        let levels = if pinned { "" } else { r#"<div class="moss-nav-island-menu" data-island-menu="levels" hidden></div>"# };
        format!(
            r#"<div class="moss-nav-island"{}><div class="moss-nav-island-bar">{}</div>{levels}<div class="moss-nav-island-menu" data-island-menu="sections" hidden></div>{no_script}</div>"#,
            if pinned { r#" data-header="none""# } else { "" },
            bar.concat(),
        )
    }

    /// The sections button: a ruler glyph the script fills with the current
    /// section's name and wires to the sections panel.
    fn sections_button_html(&self) -> String {
        let sections_label =
            crate::build::page::meta::escape_html_attr(crate::i18n::t(self.current_lang, "nav_sections"));
        // The glyph is a ruler: dashes of one length with one longer, the mark
        // the contents ruler and the panel's rows use for the current section.
        // The sections glyph and the progress rule are inert markup until
        // `nav-island.ts` fills them in; both are `hidden` / zero-width so a
        // script-less page shows neither. `aria-label` only, no `data-tooltip`
        // — toggles carry no hover hints (the glyph is its own label, and a
        // hint sticks after a tap on touch); the island's one hint is the `…`
        // button's, which names levels the bar has hidden and is set by
        // nav-island.ts while folded.
        format!(
            r#"<button type="button" class="moss-nav-island-sections" aria-expanded="false" aria-label="{sections_label}"><svg xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 6h6M5 10h6M5 14h13M5 18h6"/></svg></button>"#
        )
    }
}
