//! The dialect table: irreducible per-builder knowledge as data, not code.
//!
//! Heuristics are the default; a dialect row records only the deltas
//! heuristics get wrong (sentinel URL values, active-child wrappers, chrome
//! component names). A future builder costs a table row, not a module.

/// How the walker treats one component `type` value. A type may carry
/// SEVERAL rules — they all apply, in table order (Blog.Section has both a
/// `Child` and a `Container` row).
pub(crate) enum NodeRule {
    /// Prose from a string field; `unescape` for HTML-entity-escaped values.
    Text {
        field: &'static str,
        unescape: bool,
    },
    /// Quoted prose from a string field.
    Quote {
        field: &'static str,
    },
    /// Image from the dialect's image fields, optionally gated by a
    /// `(field, skip_when)` boolean opt-out on the component.
    Image {
        gate: Option<(&'static str, bool)>,
    },
    /// External video URL from a string field (empty → dropped).
    Video {
        field: &'static str,
    },
    /// Link button from `text` + `url` fields (either empty → dropped).
    Button,
    Separator,
    /// Walk ONE child component held directly under this field
    /// (Blog.Section's singular `component`). Untyped values fall out.
    Child(&'static str),
    /// Structural wrapper: iterate each field's COLLECTION of children —
    /// a components map keyed by slot name, or a list.
    Container(&'static [&'static str]),
    /// Wrapper that activates ONE child slot: read the discriminator
    /// `field`, recurse into the slot mapped for its value, else into
    /// `default_slot`. (Strikingly `Media`: the inactive `video` slot holds
    /// a stock placeholder that must not embed.)
    ActiveChild {
        field: &'static str,
        slots: &'static [(&'static str, &'static str)],
        default_slot: &'static str,
    },
    /// Known chrome / dynamic widget: skip silently, by decision.
    Chrome,
}

/// CDN fallback for images whose URL field is a sentinel: the real asset is
/// `{prefix}{res_id}{infix}{key}.{format}`.
pub(crate) struct CdnTemplate {
    pub key_field: &'static str,
    pub format_field: &'static str,
    pub default_format: &'static str,
    pub prefix: &'static str,
    pub infix: &'static str,
}

/// One builder's JSON-tree dialect: component rules + image conventions.
pub(crate) struct JsonDialect {
    pub rules: &'static [(&'static str, NodeRule)],
    /// Whether an unmatched `type` value names a genuine component (counted
    /// into `skipped`) as opposed to a field discriminator (ignored). Per
    /// dialect because the naming convention is the builder's: Strikingly
    /// components are PascalCase, Readymag's are kebab-case.
    pub unknown_is_component: fn(&str) -> bool,
    /// Image URL field, and the sentinel value meaning "no real URL — use
    /// the CDN template" (Strikingly stores literal `"!"`).
    pub image_url_field: &'static str,
    pub image_url_sentinel: &'static str,
    /// Caption fields tried in order for image alt text.
    pub image_caption_fields: &'static [&'static str],
    pub cdn: Option<CdnTemplate>,
}

/// Strikingly naming: PascalCase = component; lowercase `type` values
/// ("section", "default", …) are field discriminators, not components.
fn pascal_case_component(ty: &str) -> bool {
    ty.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Strikingly's `$S` component tree. Match arms come from the riverbend
/// corpus census (`河灣/.port/census-summary.txt`): ALL prose is
/// `RichText.value`; real videos are bare `Video` components; `Media`
/// wrappers carry a stock placeholder video and select their active child
/// via `current`; `Background.useImage=false` is an explicit opt-out.
pub(crate) static STRIKINGLY: JsonDialect = JsonDialect {
    rules: &[
        (
            "RichText",
            NodeRule::Text {
                field: "value",
                unescape: false,
            },
        ),
        // HtmlComponent.value arrives HTML-entity-escaped in the $S JSON.
        (
            "HtmlComponent",
            NodeRule::Text {
                field: "value",
                unescape: true,
            },
        ),
        ("Blog.Quote", NodeRule::Quote { field: "value" }),
        ("Image", NodeRule::Image { gate: None }),
        (
            "Background",
            NodeRule::Image {
                gate: Some(("useImage", false)),
            },
        ),
        (
            "Media",
            NodeRule::ActiveChild {
                field: "current",
                slots: &[("video", "video")],
                default_slot: "image",
            },
        ),
        ("Video", NodeRule::Video { field: "url" }),
        ("Button", NodeRule::Button),
        ("Separator", NodeRule::Separator),
        ("Blog.Section", NodeRule::Child("component")),
        ("Blog.Section", NodeRule::Container(&["components"])),
        ("Slide", NodeRule::Container(&["components"])),
        ("BlockComponentItem", NodeRule::Container(&["components"])),
        ("RepeatableItem", NodeRule::Container(&["components"])),
        ("RepeatedElements", NodeRule::Container(&["components"])),
        ("Buttons", NodeRule::Container(&["components"])),
        ("BlockComponent", NodeRule::Container(&["items"])),
        ("Repeatable", NodeRule::Container(&["list"])),
        ("Gallery", NodeRule::Container(&["sources"])),
        ("Spacer", NodeRule::Chrome),
        ("SlideSettings", NodeRule::Chrome),
        ("EmailForm", NodeRule::Chrome),
        ("CustomForm", NodeRule::Chrome),
        ("BlogCollectionComponent", NodeRule::Chrome),
        ("PortfolioComponent", NodeRule::Chrome),
        ("LayoutVariants", NodeRule::Chrome),
    ],
    unknown_is_component: pascal_case_component,
    image_url_field: "url",
    image_url_sentinel: "!",
    image_caption_fields: &["caption", "description"],
    cdn: Some(CdnTemplate {
        key_field: "storageKey",
        format_field: "format",
        default_format: "jpg",
        prefix: "https://custom-images.strikinglycdn.com/res/",
        // c_limit requests a large render; the asset download pass fetches
        // it like any remote image.
        infix: "/image/upload/c_limit,h_2400,w_2400,q_90/",
    }),
};

/// Host-gated CSS-selector dialect for sites whose DOM defeats the generic
/// scorer but need no JSON walk: pick the first non-empty match.
pub(crate) struct SelectorDialect {
    /// Matches `host == suffix` or `host` ending in `.suffix`.
    pub host_suffix: &'static str,
    /// Content containers, most specific first.
    pub selectors: &'static [&'static str],
}

/// Douban reviews/notes: the body lives in a small subtree while a
/// `<ul class="nav">` global menu (no `<nav>` tag, no clutter class)
/// dominates the scorer's biggest candidate.
pub(crate) static DOUBAN: SelectorDialect = SelectorDialect {
    host_suffix: "douban.com",
    selectors: &[".review-content", "#link-report .note", ".note"],
};
