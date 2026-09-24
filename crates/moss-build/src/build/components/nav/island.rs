//! The floating nav island — the second, smaller navigation object that
//! appears part-way down a long page.
//!
//! A child module of `nav` rather than a sibling file for one reason: it
//! builds from `NavigationBuilder`'s private state, and a child module can
//! read its parent's privates. Keeping it out of `nav.rs` keeps that file
//! under the size the ratchet caps at, and matches how the island reads on
//! screen — related to the masthead, not part of it.

use super::NavigationBuilder;

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
        let Some(segments) = &self.breadcrumb_segments else {
            return String::new();
        };
        if segments.is_empty() {
            return String::new();
        }

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

        let sections_label =
            crate::build::page::meta::escape_html_attr(crate::i18n::t(lang, "nav_sections"));
        let trail_label =
            crate::build::page::meta::escape_html_attr(crate::i18n::t(lang, "nav_breadcrumb"));

        // The sections glyph and the progress rule are inert markup until
        // `nav-island.ts` fills them in; both are `hidden` / zero-width so a
        // script-less page shows neither. `aria-label` only, no `data-tooltip`
        // — toggles carry no hover hints (the glyph is its own label, and a
        // hint sticks after a tap on touch); the island's one hint is the `…`
        // button's, which names levels the bar has hidden and is set by
        // nav-island.ts while folded.
        let sections_button = format!(
            r#"<button type="button" class="moss-nav-island-sections" aria-expanded="false" aria-label="{sections_label}"><svg xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h10M4 12h16M4 18h12"/></svg></button>"#
        );

        [
            r#"<div class="moss-nav-island">"#.to_string(),
            r#"<div class="moss-nav-island-bar">"#.to_string(),
            format!(r#"<nav class="moss-nav-island-trail" aria-label="{trail_label}">{trail_body}</nav>"#),
            format!(r#"<span class="moss-nav-island-actions">{sections_button}</span>"#),
            r#"<span class="moss-nav-island-progress"><span class="moss-nav-island-progress-fill"></span></span>"#.to_string(),
            r#"</div>"#.to_string(),
            // Two panels, one shape. The `…` opens the levels it folded away;
            // the glyph opens this page's sections. Both are populated by the
            // script and empty in the emitted HTML. They are siblings of the
            // bar rather than children because the bar clips its own overflow
            // — which means their containing block is `.moss-nav-island`, and
            // `nav-island.ts` must place them against THAT box, not the bar's.
            r#"<div class="moss-nav-island-menu" data-island-menu="levels" hidden></div>"#.to_string(),
            r#"<div class="moss-nav-island-menu" data-island-menu="sections" hidden></div>"#.to_string(),
            r#"</div>"#.to_string(),
        ]
        .concat()
    }
}
