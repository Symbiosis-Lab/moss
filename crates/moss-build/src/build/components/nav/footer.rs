//! The site footer (`generate_footer`). A child module of `nav` for the same
//! reason as `breadcrumb`/`island`: it builds from `NavigationBuilder`'s
//! private state without widening any of it.

use super::NavigationBuilder;
use crate::build::types::ParsedDocument;

impl<'a> NavigationBuilder<'a> {
    /// Generates footer HTML.
    ///
    /// Layout:
    /// ```html
    /// <footer class="container">
    ///   {footer.md HTML, if present}
    ///   {default link list <p class="footer-default">, if any links}
    ///   {auto-injected subscribe form, if moss-hosted with [channels.email]}
    /// </footer>
    /// ```
    ///
    /// Three-segment vertical stack: leading author chrome (footer.md) →
    /// auto-generated link list → trailing widget (subscribe form). The
    /// trailing-widget position is a deliberate design decision — links lead,
    /// the auto-injected widget trails.
    ///
    /// Flat HTML — authored content sits as direct children of `<footer>`.
    /// The default visual chrome (border-top divider, padding, muted
    /// typography) lives on `footer.container` directly in CSS; the footer
    /// renders at the body font-size by default. This shape gives sites two
    /// ways to customize:
    ///
    /// 1. Override `footer.container { ... }` to replace the default chrome.
    /// 2. Use `body > footer.container > selector` rules to target individual
    ///    elements for custom designs (e.g. brand text +
    ///    :::grid + copyright + :::subscribe stack with custom flex layout).
    ///
    /// History: An earlier "verbatim footer" design (commit 6e47a8024)
    /// stripped the `.footer-content` wrapper but left no chrome on
    /// `<footer>` either, removing the divider + muted typography from the
    /// default look. The current shape restores the chrome on `footer.container`
    /// directly so existing sites' `body > footer.container > *` direct-
    /// child selectors keep working.
    ///
    /// When `footer.md` is absent, the wrapper still emits with default
    /// content: an auto-generated link list from pages with `footer: true`
    /// frontmatter, plus an optional RSS link.
    ///
    /// `current_page_url` is used to mark the matching footer link as
    /// `.active` (parallel to `generate_navigation`).
    pub fn generate_footer(&self, show_rss: bool) -> String {
        let mut footer_pages: Vec<&ParsedDocument> = self.documents
            .iter()
            .filter(|d| d.footer == Some(true) && self.in_page_edition(d))
            .collect();
        footer_pages.sort_by(|a, b| match (a.weight, b.weight) {
            // Tied weights fall through to alphabetical for cross-platform
            // determinism — see comment in generate_navigation().
            (Some(aw), Some(bw)) => aw.cmp(&bw).then_with(|| a.nav_text().cmp(b.nav_text())),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            // Sort alphabetically by the plain-text chrome label.
            (None, None) => a.nav_text().cmp(b.nav_text()),
        });

        let mut default_links = Vec::new();
        for doc in &footer_pages {
            let pretty = crate::build::scan::article_map::to_pretty_url(&doc.url_path);
            let href = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (footer nav href to user content page, not a framework asset)
            let class = if self.current_page_url.map_or(false, |url| url == doc.url_path) {
                "footer-link active"
            } else {
                "footer-link"
            };
            // Footer link text is navigation chrome: `nav_label` else label.
            default_links.push(format!(
                r#"<a href="{}" class="{}">{}</a>"#,
                href, class, crate::build::features::html_escape(doc.nav_text())
            ));
        }

        if show_rss {
            let rss_url = crate::build::served_path::ServedPath::for_rss("").unwrap().to_relative_url();
            default_links.push(format!(
                r#"<a href="{rss_url}" class="footer-link" data-external>{}</a>"#,
                crate::i18n::t(self.lang, "rss"),
            ));
        }

        // Two slot markers, resolved during the pre-ship slot pass:
        //   slot:footer-left  — author chrome (footer.md) or empty
        //   slot:footer-end   — auto-injected subscribe form or empty
        // When both content slots are empty, only the default link list shows.
        //
        // Footer LAYOUT is driven purely by CSS: `footer.container:has(> .moss-subscribe)`
        // lays the footer out as a flex row (links left, subscribe form
        // right-anchored) — see site.css. There is no `data-moss-shape` marker:
        // the retired attribute existed only to toggle that layout from the
        // build side, but keying the CSS on the presence of the (now-unified)
        // `.moss-subscribe` form covers the auto-injected AND footer.md cases
        // uniformly, so the footer open tag is a plain `<footer class="container">`.
        //
        // The default link list is emitted in a wrapping <p> with class
        // `footer-default` so themes can hide it (`.footer-default { display:
        // none }`) when they author a richer footer.
        //
        // No `.footer-content` wrapper here: the visual chrome (divider,
        // padding, muted typography) lives on `<footer class="container">`
        // directly. This keeps the HTML flat — author content sits as direct
        // children of <footer>, which lets sites use `body > footer.container
        // > selector` rules to target individual elements
        // for their custom design.
        let default_inner = if default_links.is_empty() {
            String::new()
        } else {
            format!(
                "\n        <p class=\"footer-default\">{}</p>",
                default_links.join(" · ")
            )
        };

        // `<!-- moss:footer -->` / `<!-- /moss:footer -->` bound the element
        // for `build::enhance::strip_empty_footer`, which runs later (after
        // the pre-ship slot pass resolves footer-left/footer-end) and needs
        // to find and, if nothing rendered, remove exactly this element —
        // never by searching for a literal `<footer` tag, which an author's
        // own raw-HTML `<footer>` elsewhere on the page (moss passes body
        // HTML through verbatim) could also match. These sentinels are
        // code-controlled and placed with no adjacent whitespace so that
        // stripping them back out, once the keep/drop decision is made,
        // reconstructs this exact byte shape — the same trust model the
        // `<!-- slot:NAME -->` markers below already rely on.
        // `<!-- footer:has-content -->` is the render-time half of that
        // decision: present only when the default link list (footer:true
        // pages, the RSS link) is non-empty, since that content is baked in
        // here and unlike footer-left/footer-end is never re-derived later.
        let has_default_marker = if default_links.is_empty() { "" } else { "<!-- footer:has-content -->" };

        format!(
            r#"<!-- moss:footer -->{has_default_marker}<footer class="container">
        <!-- slot:footer-left -->{default_inner}
        <!-- slot:footer-end -->
</footer><!-- /moss:footer -->"#
        )
    }
}

/// Remove the whole chrome `<footer>` element when it has nothing left to
/// show; otherwise strip the bookkeeping markers back out so shipped HTML
/// carries no trace of them. The element is found by its own
/// `<!-- moss:footer -->` / `<!-- /moss:footer -->` bounds — fixed,
/// code-controlled tokens, the same trust model `<!-- slot:NAME -->` already
/// uses — never by searching for a literal `<footer` tag, which an author's
/// own raw-HTML `<footer>` elsewhere on the page (moss passes body HTML
/// through verbatim) could also match.
///
/// The keep/drop decision itself is three known facts, never a scan of
/// `html` for visible content: `footer_left_has_content` /
/// `footer_end_has_content` (from `inject_slots`'s own `slots.get_html`
/// calls above — did `footer.md`/a `slot: footer-left` page, or the
/// auto-injected subscribe form/a plugin widget, resolve to anything this
/// pass), and the `<!-- footer:has-content -->` marker, the render-time
/// third fact: whether `generate_footer` emitted any default links or a feed
/// link, which is baked into `html` already and never re-resolved here — the
/// marker is only how it survives the render → slot-injection boundary.
///
/// moss's colophon is bounded in the template by its own
/// `<!-- moss:colophon -->` / `<!-- /moss:colophon -->` tokens. A kept footer
/// takes it in as its last child, so a theme's footer layout (a band
/// background, a closing picture) can place the mark; a stripped footer, or
/// no footer at all, leaves it where the template put it. Its bounds are
/// erased either way.
pub(crate) fn strip_empty_footer(html: String, footer_left_has_content: bool, footer_end_has_content: bool) -> String {
    const OPEN: &str = "<!-- moss:footer -->";
    const HAS_CONTENT: &str = "<!-- footer:has-content -->";
    const CLOSE: &str = "<!-- /moss:footer -->";
    const COLOPHON_OPEN: &str = "<!-- moss:colophon -->";
    const COLOPHON_CLOSE: &str = "<!-- /moss:colophon -->";

    let erase_colophon_bounds = |s: String| s.replacen(COLOPHON_OPEN, "", 1).replacen(COLOPHON_CLOSE, "", 1);

    let Some(open_start) = html.find(OPEN) else {
        return erase_colophon_bounds(html);
    };
    let Some(close_rel) = html[open_start..].find(CLOSE) else {
        return erase_colophon_bounds(html);
    };
    let close_end = open_start + close_rel + CLOSE.len();

    let region = &html[open_start..close_end];
    let has_default_content = region.contains(HAS_CONTENT);

    if !(footer_left_has_content || footer_end_has_content || has_default_content) {
        return erase_colophon_bounds(format!("{}{}", &html[..open_start], &html[close_end..]));
    }

    // Keep the element; erase the bookkeeping markers (each a plain
    // substring removal — `HAS_CONTENT` is a no-op when absent) so the
    // shipped bytes are exactly what `generate_footer` would have
    // produced with no sentinels at all.
    let mut kept = region.replacen(OPEN, "", 1).replacen(HAS_CONTENT, "", 1).replacen(CLOSE, "", 1);
    let mut rest = html[close_end..].to_string();
    if let (Some(c_open), Some(c_close), Some(at)) = (rest.find(COLOPHON_OPEN), rest.find(COLOPHON_CLOSE), kept.rfind("</footer>")) {
        let c_end = c_close + COLOPHON_CLOSE.len();
        if c_open < c_close {
            kept.insert_str(at, &format!("    {}\n", &rest[c_open + COLOPHON_OPEN.len()..c_close]));
            // The blank gap the colophon sat in goes with it.
            let gap_start = if rest[..c_open].trim().is_empty() { 0 } else { c_open };
            rest.replace_range(gap_start..c_end, "");
        }
    }
    erase_colophon_bounds(format!("{}{}{}", &html[..open_start], kept, rest))
}
