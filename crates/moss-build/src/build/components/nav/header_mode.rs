//! `[site].header` — what leads the masthead's left side.
//!
//! A standalone value type rather than a private field on
//! `NavigationBuilder` itself: unlike `breadcrumb`/`island`, nothing here
//! needs the builder's private state, so it earns a plain sibling module
//! instead of one built for privacy access.

/// `Brand` (the default) is the site name or breadcrumb trail
/// `NavigationBuilder::nav_left_html` already renders. `Nav` drops that
/// entirely: no site name or breadcrumb on any page, and the link list opens
/// with a Home item instead (see `NavigationBuilder::generate_navigation`).
/// Breadcrumbs are suppressed too under `Nav` — the Home link already gives
/// every page a way back, which is the same job breadcrumb auto-enable
/// exists to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeaderMode {
    #[default]
    Brand,
    Nav,
}

impl HeaderMode {
    /// Parse `[site].header`. An absent key, empty string, or `"brand"` all
    /// resolve to the default; anything but `"nav"` besides those falls back
    /// to `Brand` with a build warning, the same shape as
    /// `LocatorPlacement::from_config`.
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("") | Some("brand") => Self::Brand,
            Some("nav") => Self::Nav,
            Some(other) => {
                crate::build::cli_output::log_warn_problem!(
                    "[site].header must be 'brand' or 'nav', not '{other}'; using 'brand'"
                );
                Self::Brand
            }
        }
    }
}
