//! Every contract name moss has released and no longer writes: classes,
//! `data-*` values and `--moss-*` properties a theme selects, and the
//! shortcodes, attributes, front-matter fields, config keys and syntax a
//! folder's pages are written in.
//!
//! A theme selects these names, so one going away changes how a site looks
//! with no error anywhere; a page written in an old spelling must keep
//! building. A row here is what the build tells the author
//! ([`Retired::note`], and [`Retired::replaced_by`] when something reaches
//! the same thing today). The released-contract lock test fails
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
    /// A `:::name` block.
    Shortcode,
    /// An attribute a block reads, written without its `=`.
    ShortcodeAttr,
    /// A front-matter field.
    FrontMatter,
    /// A `.moss/config.toml` key as `<table>.<key>`.
    ConfigKey,
    /// A piece of markdown syntax moss reads.
    Syntax,
}

/// What replaces a retired name, and how `moss migrate` rewrites a folder
/// that still uses it. One field, so a replacement without a migration
/// cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migrate {
    /// The kind's own rewriter puts this selector or property in its place.
    Rename(&'static str),
    /// The feature is gone; nothing replaces it.
    Gone,
}

#[derive(Debug, Clone, Copy)]
pub struct Retired {
    pub kind: RetiredKind,
    /// The name as a theme writes it, without the leading `.` for a class.
    pub name: &'static str,
    /// The first release without it. `None` when it went before the
    /// contract's recorded history begins (moss 0.6.6).
    pub retired_in: Option<&'static str>,
    /// What reaches the same thing today, and how a folder is moved to it.
    pub migrate: Migrate,
    /// Whether moss still writes the old name beside its replacement, or for
    /// a content row still reads it.
    pub aliased: bool,
    /// What the author is told, as one clause.
    pub note: &'static str,
    /// The date (`YYYY-MM-DD`) the build first told authors the name was
    /// going. A removal from [`ALIAS_FLOOR`] on is at least 12 months after
    /// it; `None` on rows from before that rule.
    pub deprecated_on: Option<&'static str>,
}

impl RetiredKind {
    /// The word a migration id starts with.
    pub const fn slug(self) -> &'static str {
        match self {
            RetiredKind::Class => "class",
            RetiredKind::Data => "data",
            RetiredKind::Property => "property",
            RetiredKind::Shortcode => "shortcode",
            RetiredKind::ShortcodeAttr => "shortcode-attr",
            RetiredKind::FrontMatter => "front-matter",
            RetiredKind::ConfigKey => "config-key",
            RetiredKind::Syntax => "syntax",
        }
    }
}

impl Retired {
    /// A selector (or property) that reaches the same thing today.
    pub const fn replaced_by(&self) -> Option<&'static str> {
        match self.migrate {
            Migrate::Rename(now) => Some(now),
            Migrate::Gone => None,
        }
    }

    /// The stable name `moss migrate` gives this row's migration:
    /// `<kind>:<name>`, such as `class:moss-cards-grid`.
    pub fn id(&self) -> String {
        format!("{}:{}", self.kind.slug(), self.name)
    }

    /// Whether migrating changes nothing a reader sees: moss still writes
    /// the old name, so old and new render the same. A rewrite of a name
    /// moss stopped writing brings its rules back, which changes the page.
    pub const fn keeps_look(&self) -> bool {
        self.aliased
    }

    /// The new spelling of a renamed row the parser names in its own note.
    pub(crate) fn renamed_to(&self) -> &'static str {
        self.replaced_by().unwrap_or_else(|| panic!("retired row {} has no replacement", self.id()))
    }

    /// When it went, as a sentence ends: `in moss 0.15.1`.
    pub fn when(&self) -> String {
        self.retired_in.map_or_else(|| "before moss 0.6.6".to_string(), |v| format!("in moss {v}"))
    }
}

/// The first release in which a retired rename must stay aliased.
pub const ALIAS_FLOOR: &str = "0.15.28";

const fn class(name: &'static str, retired_in: Option<&'static str>, migrate: Migrate, note: &'static str) -> Retired {
    Retired { kind: RetiredKind::Class, name, retired_in, migrate, aliased: false, note, deprecated_on: None }
}

const fn property(name: &'static str, retired_in: &'static str, migrate: Migrate, note: &'static str) -> Retired {
    Retired { kind: RetiredKind::Property, name, retired_in: Some(retired_in), migrate, aliased: false, note, deprecated_on: None }
}

/// A content spelling moss still reads beside its replacement.
const fn renamed(kind: RetiredKind, name: &'static str, retired_in: &'static str, now: &'static str, note: &'static str) -> Retired {
    Retired { kind, name, retired_in: Some(retired_in), migrate: Migrate::Rename(now), aliased: true, note, deprecated_on: None }
}

const fn removed_block(name: &'static str, retired_in: &'static str, note: &'static str) -> Retired {
    Retired { kind: RetiredKind::Shortcode, name, retired_in: Some(retired_in), migrate: Migrate::Gone, aliased: false, note, deprecated_on: None }
}

const COLLAPSED: &str = "the card layouts share one set of classes";
const CALLOUT_TYPE: &str = "the callout's type is its data-type attribute";
const EMBED_TYPE: &str = "the embed's kind is its data-type attribute";
const HIGHLIGHTER: &str = "moss never wrote it: the highlighter it belonged to was removed unused";
const APPLY: &str = "the :::apply form was removed";
const LINK_PREVIEW: &str = "an external link in a grid renders as a card";
const SYNDICATION: &str = "a page no longer links to its copies on other networks";

use Migrate::{Gone, Rename};

pub const RETIRED: &[Retired] = &[
    class("moss-cards-grid", None, Rename(".moss-cards[data-layout=grid]"), COLLAPSED),
    class("moss-cards-list", None, Rename(".moss-cards[data-layout=list]"), COLLAPSED),
    class("moss-card-grid", None, Rename(".moss-cards[data-layout=grid] .moss-card"), COLLAPSED),
    class("moss-card-grid-cover", None, Rename(".moss-cards[data-layout=grid] .moss-card-cover"), COLLAPSED),
    class("moss-card-grid-no-cover", None, Rename(".moss-cards[data-layout=grid] .moss-card-no-cover"), COLLAPSED),
    class("moss-card-grid-content", None, Rename(".moss-cards[data-layout=grid] .moss-card-content"), COLLAPSED),
    class("moss-card-grid-kicker", None, Rename(".moss-cards[data-layout=grid] .moss-card-kicker"), COLLAPSED),
    class("moss-card-grid-title", None, Rename(".moss-cards[data-layout=grid] .moss-card-title"), COLLAPSED),
    class("moss-card-grid-meta", None, Rename(".moss-cards[data-layout=grid] .moss-card-meta"), COLLAPSED),
    class("moss-card-list", None, Rename(".moss-cards[data-layout=list] .moss-card"), COLLAPSED),
    class("moss-card-list-row", None, Rename(".moss-cards[data-layout=list] .moss-card-row"), COLLAPSED),
    class("moss-card-list-cover", None, Rename(".moss-cards[data-layout=list] .moss-card-cover"), COLLAPSED),
    class("moss-card-list-body", None, Rename(".moss-cards[data-layout=list] .moss-card-body"), COLLAPSED),
    class("moss-card-list-head", None, Rename(".moss-cards[data-layout=list] .moss-card-body"), COLLAPSED),
    class("moss-card-list-kicker", None, Rename(".moss-cards[data-layout=list] .moss-card-kicker"), COLLAPSED),
    class("moss-card-list-title", None, Rename(".moss-cards[data-layout=list] .moss-card-title"), COLLAPSED),
    class("moss-card-list-meta", None, Rename(".moss-cards[data-layout=list] .moss-card-meta"), COLLAPSED),
    class("moss-card-list-description", None, Rename(".moss-cards[data-layout=list] .moss-card-description"), COLLAPSED),
    class("moss-card-minimal", None, Rename(".moss-cards[data-layout=minimal] .moss-card"), COLLAPSED),
    class("callout-note", None, Rename(".callout[data-type=note]"), CALLOUT_TYPE),
    class("callout-info", None, Rename(".callout[data-type=info]"), CALLOUT_TYPE),
    class("callout-tip", None, Rename(".callout[data-type=tip]"), CALLOUT_TYPE),
    class("callout-warning", None, Rename(".callout[data-type=warning]"), CALLOUT_TYPE),
    class("callout-pending", None, Rename(".callout[data-type=pending]"), CALLOUT_TYPE),
    class("moss-embed-pdf", None, Rename(".moss-embed[data-type=pdf]"), EMBED_TYPE),
    class("moss-embed-iframe", None, Rename(".moss-embed[data-type=iframe]"), EMBED_TYPE),
    class("moss-embed-notebook", None, Rename(".moss-embed[data-type=notebook]"), EMBED_TYPE),
    class("moss-embed-table", None, Rename(".moss-embed[data-type=table]"), EMBED_TYPE),
    class("moss-embed-3d", None, Rename(".moss-embed[data-type=3d]"), EMBED_TYPE),
    class("moss-summary-layout", Some("0.6.7"), Rename(".moss-cards[data-layout=list]"), "a summary listing is a list listing"),
    class("moss-visual", Some("0.7.5"), Gone, "the Visual page layout was removed"),
    class("moss-gridxyz", Some("0.7.5"), Gone, "nothing writes it"),
    class("moss-subscribe-btn-slot", Some("0.7.11"), Rename(".moss-btn-slot"), "every button slot shares one class"),
    class("moss-apply-details", Some("0.7.15"), Gone, APPLY),
    class("moss-apply-label", Some("0.7.15"), Gone, APPLY),
    class("moss-img-fallback", Some("0.11.0"), Gone, "an image that fails to load is no longer replaced"),
    class("moss-collection-description", Some("0.11.0"), Gone, "a collection no longer prints its description"),
    class("moss-card-head", Some("0.15.1"), Rename(".moss-card-body"), "kicker, meta and title sit directly in the card body"),
    class("link-preview", Some("0.15.1"), Rename(".moss-card[data-external]"), LINK_PREVIEW),
    class("link-preview-title", Some("0.15.1"), Rename(".moss-card[data-external] .moss-card-title"), LINK_PREVIEW),
    class("link-preview-domain", Some("0.15.1"), Rename(".moss-card[data-external] .moss-card-kicker"), LINK_PREVIEW),
    class("moss-card-count", Some("0.15.1"), Gone, "a card no longer shows a count"),
    class("moss-embed-video-download", Some("0.15.1"), Gone, "a video embed no longer offers a download link"),
    class("moss-hl-keyword", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-string", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-comment", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-function", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-type", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-number", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-operator", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-builtin", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-tag", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-attr", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-meta", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-addition-bg", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-deletion", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-hl-deletion-bg", Some("0.15.2"), Gone, HIGHLIGHTER),
    class("moss-place-breadcrumb", Some("0.15.8"), Gone, "a place no longer lists the places it sits in"),
    class("moss-apply", Some("0.15.13"), Gone, APPLY),
    class("moss-apply-form", Some("0.15.13"), Gone, APPLY),
    class("moss-apply-matters", Some("0.15.13"), Gone, APPLY),
    class("moss-apply-hp", Some("0.15.13"), Gone, APPLY),
    class("moss-apply-status", Some("0.15.13"), Gone, APPLY),
    class("moss-apply-helper", Some("0.15.13"), Gone, APPLY),
    class("moss-scroll-dots-track", Some("0.15.13"), Gone, "the scroll dots became previous, next and a count"),
    class("comment-source-link", Some("0.15.23"), Gone, SYNDICATION),
    class("moss-syndication", Some("0.15.23"), Gone, SYNDICATION),
    class("moss-syndication-label", Some("0.15.23"), Gone, SYNDICATION),
    class("moss-series-nav-text", Some("0.15.26"), Rename(".moss-series-nav-link"), "a series link holds its arrow and title directly"),
    class("moss-series-nav-label", Some("0.15.26"), Gone, "the direction word is hidden, read by screen readers only"),
    class("moss-form__hp", Some("0.15.30"), Gone, "request forms no longer carry a honeypot field"),
    property("--moss-font-size", "0.7.4", Rename("--moss-reading-size"), "renamed"),
    property("--moss-font-size-base", "0.7.4", Rename("--moss-reading-size-base"), "renamed"),
    property("--moss-font-weight", "0.7.11", Rename("--moss-font-weight-body"), "renamed"),
    property("--moss-size-3xl", "0.12.0", Gone, "nothing read it; --moss-size-2xl is the largest step"),
    property("--moss-scroll-dot-start", "0.15.13", Gone, "nothing read it"),
    renamed(RetiredKind::ShortcodeAttr, "cols", "0.15.1", "per-line", "renamed"),
    renamed(RetiredKind::Syntax, "---", "0.6.5", "+++", "a `:::grid` cell ends at a `+++` line"),
    renamed(RetiredKind::FrontMatter, "sidebar", "0.6.5", "children", "a folder's pages go in the sidebar with `children_in: sidebar`"),
    renamed(RetiredKind::ConfigKey, "site.domain", "0.15.21", "site.url", "the site's address, path included, is `url`"),
    removed_block("apply", "0.15.13", APPLY),
    removed_block("toc", "0.6.5", "the :::toc block was removed"),
];

/// The retired row for `name` of `kind`, if there is one.
pub fn find(kind: RetiredKind, name: &str) -> Option<&'static Retired> {
    RETIRED.iter().find(|r| r.kind == kind && r.name == name)
}

/// A row the parser names in its own message, so it is always there.
pub(crate) fn row(kind: RetiredKind, name: &str) -> &'static Retired {
    find(kind, name).unwrap_or_else(|| panic!("no retired row for {} {name}", kind.slug()))
}
