//! The search button, language toggle and theme toggle: the cluster the
//! masthead shows at its end, and the one the nav island takes over when the
//! site has `[site].header = "none"`. A child module of `nav` because it reads
//! the builder's private state.

use super::NavigationBuilder;

impl<'a> NavigationBuilder<'a> {
    /// `<div class="nav-icons">` with the controls this page carries.
    pub(super) fn nav_icons_html(&self) -> String {
        // Icon items: search (left), language toggle, theme toggle (right).
        // Left to right the cluster runs from the most content-related control
        // to the most presentation-related one: search sits nearest the nav
        // links it complements (it IS navigation), language changes what
        // content you read, theme is pure presentation and keeps its
        // established right-most anchor.
        let mut icon_items: Vec<String> = Vec::new();

        // Search icon — only when this build actually shipped an index
        // (`_moss/pagefind/`).
        // The client runtime binds the click; with JS off the
        // button is inert, which is why it is a `<button>` and not a link.
        if self.has_search {
            let search_label = crate::i18n::t(self.current_lang, "nav_search");
            icon_items.push(format!(
                r#"<button class="nav-search-btn" type="button" aria-label="{}"><svg class="search-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" viewBox="0 0 32 32"><circle cx="14" cy="14" r="8.5"/><path d="m20.2 20.2 6.3 6.3"/></svg></button>"#,
                search_label,
            ));
        }

        // Language toggle in nav (site-wide: shows all site languages)
        if !self.translations.is_empty() {
            let current = format!(
                r#"<span class="nav-lang-current">{}</span>"#,
                crate::i18n::switcher_label(&self.current_lang_tag)
            );
            let links: Vec<String> = self.translations.iter().map(|t| {
                let pretty = crate::build::scan::article_map::to_pretty_url(&t.url_path);
                let href = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (language toggle nav href to translation page, not a framework asset)
                // `lang` as well as `hreflang`: the label is written in the
                // target language ("简" on an English page), and without it a
                // screen reader speaks it in the page's voice (WCAG 3.1.2).
                format!(
                    r#"<a href="{}" class="nav-lang-link" hreflang="{}" lang="{}">{}</a>"#,
                    // The declared tag, not `code()` — whose own doc says to use
                    // the BCP-47 form for an hreflang, and which collapses an
                    // unshipped language onto one of three variants.
                    href, t.lang_tag, t.lang_tag, t.display_name
                )
            }).collect();
            let mut parts = vec![current];
            parts.extend(links);
            icon_items.push(format!(
                r#"<div class="nav-lang-toggle" role="group" aria-label="{}">{}</div>"#,
                crate::i18n::t(self.current_lang, "nav_language"),
                parts.join(" / ")
            ));
        }

        // Theme toggle icon (toggles light ↔ dark) — "classic" style from toggles.dev
        //
        // No `data-tooltip` on any toggle — only `aria-label`. The glyphs are
        // their own labels (a sun/moon, a magnifier), so a pill repeating
        // "Toggle theme" was noise; worse, on touch there is no un-hover, so a
        // tapped toggle's hint just hung there. Hover hints in the nav now
        // exist only where they carry information the row has hidden — a
        // truncated breadcrumb label, a fold's `…` levels — and those are
        // JS-promoted (breadcrumb-hint.ts, breadcrumb-fold.ts), never emitted
        // here.
        let theme_label = crate::i18n::t(self.current_lang, "nav_toggle_theme");
        icon_items.push(format!(
            r#"<button class="nav-theme-btn" type="button" aria-label="{}" onclick="toggleTheme()"><svg class="theme-toggle-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" fill="currentColor" stroke-linecap="round" viewBox="0 0 32 32"><clipPath id="theme-toggle__classic__cutout"><path d="M0-5h30a1 1 0 0 0 9 13v24H0Z"/></clipPath><g clip-path="url(#theme-toggle__classic__cutout)"><circle cx="16" cy="16" r="9.34"/><g stroke="currentColor" stroke-width="1"><path d="M16 5.5v-4"/><path d="M16 30.5v-4"/><path d="M1.5 16h4"/><path d="M26.5 16h4"/><path d="m23.4 8.6 2.8-2.8"/><path d="m5.7 26.3 2.9-2.9"/><path d="m5.8 5.8 2.8 2.8"/><path d="m23.4 23.4 2.9 2.9"/></g></g></svg></button>"#,
            theme_label,
        ));
        format!(r#"<div class="nav-icons">{}</div>"#, icon_items.join(""))
    }
}
