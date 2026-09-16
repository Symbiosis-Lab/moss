//! The masthead's `.nav-left` — a breadcrumb trail when segments are set,
//! a plain site-name link otherwise.
//!
//! A child module of `nav` for the same reason as `island`: it builds from
//! `NavigationBuilder`'s private state, and a child module can read its
//! parent's privates. Moved out of `nav.rs` whole (2026-08-14) to keep that
//! file under the size the ratchet caps at.

use super::{NavigationBuilder, island};

impl<'a> NavigationBuilder<'a> {
    /// Build the `.nav-left` fragment: breadcrumb trail or plain site name.
    ///
    /// `home_path` / `logo_html` are precomputed by `generate_navigation`
    /// (the logo also appears in the hamburger header it builds).
    pub(super) fn nav_left_html(&self, home_path: &str, logo_html: &str) -> String {
        if let Some(segments) = &self.breadcrumb_segments {
            if segments.is_empty() {
                // No segments: fall back to plain site name
                format!(
                    r#"<div class="nav-left"><a href="{}" class="site-name">{}{}</a></div>"#,
                    home_path, logo_html, self.site_title
                )
            } else {
                let mut parts = Vec::new();
                for segment in segments {
                    if segment.is_current {
                        continue;
                    }
                    let is_home = segment.url == home_path;
                    // Non-home segments nest their label in a `<span>`: the
                    // span owns `overflow: hidden` + `text-overflow: ellipsis`
                    // so the `<a>` itself can stay unclipped, which is what
                    // lets its `data-tooltip` pseudo-element paint outside the
                    // truncated box. Collapsing the two onto the `<a>` clips
                    // the hint along with the label.
                    let content = if is_home {
                        format!("{}{}", logo_html, segment.title)
                    } else {
                        format!(r#"<span class="breadcrumb-label">{}</span>"#, segment.title)
                    };
                    // Middle/parent segments are the ones CSS shrinks with an
                    // ellipsis (and clamps to a bare "…" on a phone), so they
                    // carry their untruncated label — but in `data-hint-label`,
                    // which site.css deliberately styles nothing from. The
                    // hover hint is only worth showing when the label really is
                    // cut off, and that is a question about rendered width that
                    // CSS cannot ask; `breadcrumb-hint.ts` (bundled into
                    // theme.js) measures `scrollWidth > clientWidth` on the
                    // label span and copies this attribute over to
                    // `data-tooltip` — the attribute site.css does render —
                    // only while that holds. Emitting `data-tooltip` here
                    // instead is what made every segment show a hint whether or
                    // not it was truncated. The home segment is
                    // `flex-shrink: 0` and never truncates, so it gets nothing.
                    let hint = if is_home {
                        String::new()
                    } else {
                        format!(
                            r#" data-hint-label="{}""#,
                            crate::build::page::meta::escape_html_attr(&segment.title)
                        )
                    };
                    // `data-trail-crumb` is the fold algorithm's handle on the
                    // trail (masthead-fold.ts) — on EVERY crumb, including the
                    // two that never fold, so the script can index the list
                    // positionally instead of re-deriving which is which.
                    parts.push(format!(
                        r#"<a href="{}" class="{}" data-trail-crumb{}>{}</a>"#,
                        segment.url,
                        if is_home { "site-name" } else { "breadcrumb-segment" },
                        hint,
                        content
                    ));
                }
                let separator = r#"<span class="breadcrumb-separator">/</span>"#;
                // Editor preview: the trail names the field that toggles it.
                // Only the breadcrumb-mode `.nav-left` — the plain site-name
                // fallbacks are not breadcrumbs and stay bare chrome.
                let fm_attr =
                    if self.fm_breadcrumb { r#" data-source-fm="breadcrumb""# } else { "" };
                // Fewer than three crumbs means there is no middle to fold —
                // the site name and the parent segment both always survive —
                // so a shallow trail carries no `…` and no panel at all,
                // rather than controls that can never unhide. With three or
                // more, the masthead folds exactly like the island (ADR-049
                // §4, extended to the masthead): the `…` opens the folded
                // levels, and only the last segment may ellipsise. The panel
                // is a sibling of `.nav-left` because `.nav-left` clips its
                // own overflow — its containing block is `.nav-content`.
                if parts.len() < 3 {
                    format!(r#"<div class="nav-left"{fm_attr}>{}</div>"#, parts.join(separator))
                } else {
                    let more_label = crate::build::page::meta::escape_html_attr(
                        crate::i18n::t(self.current_lang, "nav_hidden_levels"),
                    );
                    let more = island::fold_more_fragment("moss-breadcrumb-more", &more_label);
                    format!(
                        r#"<div class="nav-left"{fm_attr}>{}{separator}{more}{}</div><div class="moss-breadcrumb-menu" hidden></div>"#,
                        parts[0],
                        parts[1..].join(separator)
                    )
                }
            }
        } else {
            format!(
                r#"<div class="nav-left"><a href="{}" class="site-name">{}{}</a></div>"#,
                home_path, logo_html, self.site_title
            )
        }
    }
}
