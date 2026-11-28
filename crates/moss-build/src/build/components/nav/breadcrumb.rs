//! The masthead's `.nav-left` — a breadcrumb trail when segments are set,
//! a plain site-name link otherwise.
//!
//! A child module of `nav` for the same reason as `island`: it builds from
//! `NavigationBuilder`'s private state. The trail itself is moss-core's.

use super::NavigationBuilder;

impl<'a> NavigationBuilder<'a> {
    /// The plain (non-breadcrumb) `.nav-left`: a linked site name. Extracted
    /// so the two call sites below (no breadcrumb segments at all, and an
    /// empty segment list) share one format string instead of each carrying
    /// their own copy.
    fn plain_site_name_html(&self, home_path: &str, logo_html: &str) -> String {
        format!(r#"<div class="nav-left">{}</div>"#, self.site_name_link_html(home_path, logo_html))
    }

    /// The linked site name, in the masthead and in a headerless island.
    pub(super) fn site_name_link_html(&self, home_path: &str, logo_html: &str) -> String {
        format!(r#"<a href="{}" class="site-name">{}{}</a>"#, home_path, logo_html, self.site_title)
    }

    /// The `.nav-left` fragment: the breadcrumb trail, or the plain site name
    /// where there is none. `home_path` / `logo_html` are computed once by
    /// `generate_navigation`.
    pub(super) fn nav_left_html(&self, home_path: &str, logo_html: &str) -> String {
        match self.breadcrumb_segments.as_deref() {
            Some(segments) if !segments.is_empty() => moss_core::page::nav::trail_html(segments, &moss_core::page::nav::TrailMarkup {
                home_path,
                logo_html,
                current_visible: self.breadcrumb_current_visible,
                source_fm: self.fm_breadcrumb,
                lang: self.current_lang,
            }),
            _ => self.plain_site_name_html(home_path, logo_html),
        }
    }
}
