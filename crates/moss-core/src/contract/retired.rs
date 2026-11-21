//! Every contract name moss has released and no longer writes: classes,
//! `data-*` values and `--moss-*` properties.
//!
//! A theme selects these names, so one going away changes how a site looks
//! with no error anywhere. A row here is what the build tells the theme's
//! author ([`Retired::note`], and [`Retired::replaced_by`] when something
//! matches the same elements today). The released-contract lock test fails
//! when a released name is neither live nor listed here, so a name cannot
//! leave the contract without a row.
//!
//! A row with a `replaced_by` retired from [`ALIAS_FLOOR`] on must also be
//! `aliased`: moss keeps writing the old name beside the new one, so a theme
//! written against it keeps working. Rows from before that release are not
//! aliased; their names had already stopped matching when this rule began.

/// What kind of name a row retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetiredKind {
    /// A class on an element moss writes, with or without the `moss-` prefix.
    Class,
    /// One value of a `data-*` attribute, written `class[data-name=value]`.
    Data,
    /// A `--moss-*` design token or escape-hatch custom property.
    Property,
}

#[derive(Debug, Clone, Copy)]
pub struct Retired {
    pub kind: RetiredKind,
    /// The name as a theme writes it, without the leading `.` for a class.
    pub name: &'static str,
    /// The first release without it. `None` when it went before the
    /// contract's recorded history begins (moss 0.6.6).
    pub retired_in: Option<&'static str>,
    /// A selector (or property) that reaches the same thing today.
    pub replaced_by: Option<&'static str>,
    /// Whether moss still writes the old name beside its replacement.
    pub aliased: bool,
    /// What the theme's author is told, as one clause.
    pub note: &'static str,
}

/// The first release in which a retired rename must stay aliased.
pub const ALIAS_FLOOR: &str = "0.15.28";

const fn class(name: &'static str, retired_in: Option<&'static str>, replaced_by: Option<&'static str>, note: &'static str) -> Retired {
    Retired { kind: RetiredKind::Class, name, retired_in, replaced_by, aliased: false, note }
}

const fn property(name: &'static str, retired_in: &'static str, replaced_by: Option<&'static str>, note: &'static str) -> Retired {
    Retired { kind: RetiredKind::Property, name, retired_in: Some(retired_in), replaced_by, aliased: false, note }
}

const COLLAPSED: &str = "the card layouts share one set of classes";
const CALLOUT_TYPE: &str = "the callout's type is its data-type attribute";
const EMBED_TYPE: &str = "the embed's kind is its data-type attribute";
const HIGHLIGHTER: &str = "moss never wrote it: the highlighter it belonged to was removed unused";
const APPLY: &str = "the :::apply form was removed";
const LINK_PREVIEW: &str = "an external link in a grid renders as a card";
const SYNDICATION: &str = "a page no longer links to its copies on other networks";

pub const RETIRED: &[Retired] = &[
    class("moss-cards-grid", None, Some(".moss-cards[data-layout=grid]"), COLLAPSED),
    class("moss-cards-list", None, Some(".moss-cards[data-layout=list]"), COLLAPSED),
    class("moss-card-grid", None, Some(".moss-cards[data-layout=grid] .moss-card"), COLLAPSED),
    class("moss-card-grid-cover", None, Some(".moss-cards[data-layout=grid] .moss-card-cover"), COLLAPSED),
    class("moss-card-grid-no-cover", None, Some(".moss-cards[data-layout=grid] .moss-card-no-cover"), COLLAPSED),
    class("moss-card-grid-content", None, Some(".moss-cards[data-layout=grid] .moss-card-content"), COLLAPSED),
    class("moss-card-grid-kicker", None, Some(".moss-cards[data-layout=grid] .moss-card-kicker"), COLLAPSED),
    class("moss-card-grid-title", None, Some(".moss-cards[data-layout=grid] .moss-card-title"), COLLAPSED),
    class("moss-card-grid-meta", None, Some(".moss-cards[data-layout=grid] .moss-card-meta"), COLLAPSED),
    class("moss-card-list", None, Some(".moss-cards[data-layout=list] .moss-card"), COLLAPSED),
    class("moss-card-list-row", None, Some(".moss-cards[data-layout=list] .moss-card-row"), COLLAPSED),
    class("moss-card-list-cover", None, Some(".moss-cards[data-layout=list] .moss-card-cover"), COLLAPSED),
    class("moss-card-list-body", None, Some(".moss-cards[data-layout=list] .moss-card-body"), COLLAPSED),
    class("moss-card-list-head", None, Some(".moss-cards[data-layout=list] .moss-card-body"), COLLAPSED),
    class("moss-card-list-kicker", None, Some(".moss-cards[data-layout=list] .moss-card-kicker"), COLLAPSED),
    class("moss-card-list-title", None, Some(".moss-cards[data-layout=list] .moss-card-title"), COLLAPSED),
    class("moss-card-list-meta", None, Some(".moss-cards[data-layout=list] .moss-card-meta"), COLLAPSED),
    class("moss-card-list-description", None, Some(".moss-cards[data-layout=list] .moss-card-description"), COLLAPSED),
    class("moss-card-minimal", None, Some(".moss-cards[data-layout=minimal] .moss-card"), COLLAPSED),
    class("callout-note", None, Some(".callout[data-type=note]"), CALLOUT_TYPE),
    class("callout-info", None, Some(".callout[data-type=info]"), CALLOUT_TYPE),
    class("callout-tip", None, Some(".callout[data-type=tip]"), CALLOUT_TYPE),
    class("callout-warning", None, Some(".callout[data-type=warning]"), CALLOUT_TYPE),
    class("callout-pending", None, Some(".callout[data-type=pending]"), CALLOUT_TYPE),
    class("moss-embed-pdf", None, Some(".moss-embed[data-type=pdf]"), EMBED_TYPE),
    class("moss-embed-iframe", None, Some(".moss-embed[data-type=iframe]"), EMBED_TYPE),
    class("moss-embed-notebook", None, Some(".moss-embed[data-type=notebook]"), EMBED_TYPE),
    class("moss-embed-table", None, Some(".moss-embed[data-type=table]"), EMBED_TYPE),
    class("moss-embed-3d", None, Some(".moss-embed[data-type=3d]"), EMBED_TYPE),
    class("moss-summary-layout", Some("0.6.7"), Some(".moss-cards[data-layout=list]"), "a summary listing is a list listing"),
    class("moss-visual", Some("0.7.5"), None, "the Visual page layout was removed"),
    class("moss-gridxyz", Some("0.7.5"), None, "nothing writes it"),
    class("moss-subscribe-btn-slot", Some("0.7.11"), Some(".moss-btn-slot"), "every button slot shares one class"),
    class("moss-apply-details", Some("0.7.15"), None, APPLY),
    class("moss-apply-label", Some("0.7.15"), None, APPLY),
    class("moss-img-fallback", Some("0.11.0"), None, "an image that fails to load is no longer replaced"),
    class("moss-collection-description", Some("0.11.0"), None, "a collection no longer prints its description"),
    class("moss-card-head", Some("0.15.1"), Some(".moss-card-body"), "kicker, meta and title sit directly in the card body"),
    class("link-preview", Some("0.15.1"), Some(".moss-card[data-external]"), LINK_PREVIEW),
    class("link-preview-title", Some("0.15.1"), Some(".moss-card[data-external] .moss-card-title"), LINK_PREVIEW),
    class("link-preview-domain", Some("0.15.1"), Some(".moss-card[data-external] .moss-card-kicker"), LINK_PREVIEW),
    class("moss-card-count", Some("0.15.1"), None, "a card no longer shows a count"),
    class("moss-embed-video-download", Some("0.15.1"), None, "a video embed no longer offers a download link"),
    class("moss-hl-keyword", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-string", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-comment", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-function", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-type", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-number", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-operator", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-builtin", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-tag", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-attr", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-meta", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-addition-bg", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-deletion", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-hl-deletion-bg", Some("0.15.2"), None, HIGHLIGHTER),
    class("moss-place-breadcrumb", Some("0.15.8"), None, "a place no longer lists the places it sits in"),
    class("moss-apply", Some("0.15.13"), None, APPLY),
    class("moss-apply-form", Some("0.15.13"), None, APPLY),
    class("moss-apply-matters", Some("0.15.13"), None, APPLY),
    class("moss-apply-hp", Some("0.15.13"), None, APPLY),
    class("moss-apply-status", Some("0.15.13"), None, APPLY),
    class("moss-apply-helper", Some("0.15.13"), None, APPLY),
    class("moss-scroll-dots-track", Some("0.15.13"), None, "the scroll dots became previous, next and a count"),
    class("comment-source-link", Some("0.15.23"), None, SYNDICATION),
    class("moss-syndication", Some("0.15.23"), None, SYNDICATION),
    class("moss-syndication-label", Some("0.15.23"), None, SYNDICATION),
    class("moss-series-nav-text", Some("0.15.26"), Some(".moss-series-nav-link"), "a series link holds its arrow and title directly"),
    class("moss-series-nav-label", Some("0.15.26"), None, "the direction word is hidden, read by screen readers only"),
    property("--moss-font-size", "0.7.4", Some("--moss-reading-size"), "renamed"),
    property("--moss-font-size-base", "0.7.4", Some("--moss-reading-size-base"), "renamed"),
    property("--moss-font-weight", "0.7.11", Some("--moss-font-weight-body"), "renamed"),
    property("--moss-size-3xl", "0.12.0", None, "nothing read it; --moss-size-2xl is the largest step"),
    property("--moss-scroll-dot-start", "0.15.13", None, "nothing read it"),
];

/// The retired row for `name` of `kind`, if there is one.
pub fn find(kind: RetiredKind, name: &str) -> Option<&'static Retired> {
    RETIRED.iter().find(|r| r.kind == kind && r.name == name)
}
