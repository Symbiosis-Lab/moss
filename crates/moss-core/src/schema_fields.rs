//! Builtin frontmatter field definitions.
//!
//! This module is the **single source of truth** for all frontmatter fields
//! that moss recognizes. The schema returned by [`schema::builtin_schema()`]
//! is generated from the [`BUILTIN_FIELDS`] table, not from a hand-maintained
//! JSON file. This eliminates drift between the build pipeline's `FrontMatter`
//! struct and the editor/validation schema.
//!
//! ## Adding a new field
//!
//! 1. Add the field to `FrontMatter` in `crates/moss-core/src/frontmatter_typed.rs`.
//! 2. Add a corresponding entry to [`BUILTIN_FIELDS`] in this file.
//!
//! Both files live in the same crate — add new fields to both in the same commit.
//! Co-location and PR review are the enforcement mechanism.
//!
//! ## `skip_schema` fields
//!
//! Fields with `skip_schema: true` exist in the `FrontMatter` struct (the build
//! pipeline uses them) but are **not exposed** in the editor form or validation
//! schema. These are typically site-level config fields read only from the
//! homepage, auto-generated fields, or fields that will migrate to plugin-
//! contributed schemas.
//!
//! ## Scope groups (displayed order)
//!
//! Fields are assigned to one of five scope groups, ordered broad to narrow:
//!   1. "This Page"         — per-page content and display properties
//!   2. "Child Pages"       — controls how children are listed
//!   3. "Child Styles"      — visual/layout controls for child listings
//!   4. "Whole Site"        — properties read from the homepage to affect the whole site
//!   5. "Other"             — unknown user-authored fields (catch-all, TS side only)
//!
//! ## Scoring
//!
//! Each field carries a `score` value that drives BOTH the chip-bar visible
//! order AND the add-property search-list order (lower score = first / more
//! prominent). Score is computed as:
//!   score = 100 - (Frequency * 6 + Importance * 4)
//! where Frequency and Importance are each 0..=5 (higher = more common/important).
//! This means the maximum possible score is 100 (Frequency=0, Importance=0)
//! and the minimum is 100 - (5*6 + 5*4) = 0 (Frequency=5, Importance=5).
//! A lower score sorts earlier (more prominent position).

use crate::resolve::ext_kind::ExtKind;
use crate::schema::{FieldType, Widget};

/// A builtin frontmatter field definition.
///
/// Each entry describes a field that moss recognizes in markdown frontmatter.
/// The `schema::builtin_schema()` function reads this table to produce the
/// `ContentSchema` returned to the editor and validation engine.
pub struct BuiltinField {
    /// Field name as it appears in YAML frontmatter.
    pub name: &'static str,
    /// Data type of the field.
    pub field_type: FieldType,
    /// UI widget hint for the editor form.
    pub widget: Widget,
    /// Whether the field is required.
    pub required: bool,
    /// Default value as a JSON literal (e.g. `"true"`, `"\"list\""`, `"1"`).
    pub default_json: Option<&'static str>,
    /// Format hint (e.g. `"date"` for YYYY-MM-DD validation).
    pub format: Option<&'static str>,
    /// Allowed values for select/enum fields.
    pub enum_values: Option<&'static [&'static str]>,
    /// Item type for array fields (e.g. `FieldType::String` for `tags: [...]`).
    pub items_type: Option<FieldType>,
    /// Member variants for a `OneOf` union field. Each member is itself a
    /// `BuiltinField` (scalar field_type/widget — const-legal). Set only for
    /// union fields (`children`, `series`); `builtin_schema()` recursively
    /// materializes these into the owned `FieldDefinition::one_of`.
    pub one_of_members: Option<&'static [BuiltinField]>,
    /// Human-readable description shown in the editor form.
    pub description: &'static str,
    /// Optional human-readable label for the chip bar. When `None`, the frontend
    /// falls back to using the field key. Useful for fields with unfriendly
    /// internal names (e.g. `children_depth` → "Depth").
    pub label: Option<&'static str>,
    /// i18n key for the chip bar label, resolved by the TypeScript registry.
    /// Format: "chip.<name>.label". Empty string → frontend falls back to field name.
    /// The existing `label` field is deprecated in favour of this key.
    pub label_key: &'static str,
    /// Display score for chip bar ordering and add-property search list ordering.
    /// Lower values appear first / sort higher in the list.
    /// Formula: score = 100 - (Frequency*6 + Importance*4)
    /// where Frequency (0–5) = real usage frequency, Importance (0–5) = first-principles importance.
    /// 0 means unset (skip-schema fields). Typical range: 0 (title) to 100 (draft/listed/cascade).
    pub score: u8,
    /// If `true`, the field exists in the `FrontMatter` struct but is NOT
    /// exposed in the editor schema or validation. Used for site-level config,
    /// auto-generated fields, and fields migrating to plugin-contributed schemas.
    ///
    /// The field name IS surfaced to the frontend via
    /// `FrontmatterSchema::internal_fields` (populated by `builtin_schema()`),
    /// so the chip bar can filter these out of its render list without a
    /// hand-maintained denylist. Adding a new `skip_schema: true` field here
    /// is sufficient — no TS-side edit needed.
    pub skip_schema: bool,
    /// UI group for the add-property dropdown. Fields with the same group
    /// are displayed together. Empty string for skip_schema fields.
    /// One of: "This Page", "Child Pages", "Child Styles", "Whole Site".
    /// The "Other" group is handled entirely on the TS side for unknown fields.
    pub group: &'static str,
    /// For `Widget::FilePicker` fields, the extension kinds the picker should
    /// restrict search results to (e.g. `cover` → image or video; `logo` →
    /// image only). `None` means unrestricted. This is the schema-side SSOT
    /// the chip bar reads instead of hardcoding a `key -> ExtKind[]` switch.
    pub file_kinds: Option<&'static [ExtKind]>,
}

/// Default values for optional `BuiltinField` fields. Used with struct update
/// syntax (`..FIELD_DEFAULTS`) to reduce boilerplate in the table below.
const FIELD_DEFAULTS: BuiltinField = BuiltinField {
    name: "",
    field_type: FieldType::String,
    widget: Widget::TextInput,
    required: false,
    default_json: None,
    format: None,
    enum_values: None,
    items_type: None,
    one_of_members: None,
    description: "",
    label: None,
    label_key: "",
    score: 0,
    skip_schema: false,
    group: "",
    file_kinds: None,
};

/// Union members for `children`: a boolean toggle OR a single wikilink/path
/// pointing at the folder whose articles to render. Materialized into
/// `FieldDefinition::one_of` by `builtin_schema()`.
const CHILDREN_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::String,
        widget: Widget::WikilinkPicker,
        ..FIELD_DEFAULTS
    },
];

/// Union members for `series`: a boolean flag OR an ordered list of wikilinks
/// giving the explicit child order.
const SERIES_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::Array,
        widget: Widget::WikilinkListPicker,
        items_type: Some(FieldType::String),
        ..FIELD_DEFAULTS
    },
];

/// Union members for `sort`: a named axis (`date` / `weight` / `title`) OR a
/// list of child stems giving the explicit order. Both forms have always been
/// honoured by the build and both are documented in the field's own
/// description; declaring the field as a bare string made the list form —
/// `sort: [上篇, 中篇, 下篇]` — report "wrong type: expected string, got array"
/// on every folder index that used it.
const SORT_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["date", "weight", "title"]),
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::Array,
        widget: Widget::TagInput,
        items_type: Some(FieldType::String),
        ..FIELD_DEFAULTS
    },
];

/// Union members shared by `byline` and `colophon`: one credit string
/// (typically a block scalar, one credit per line) OR a list of credit
/// strings. Both forms normalize to the same row list via
/// `frontmatter_union::normalize_credit_rows`.
const CREDIT_ROW_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::String,
        widget: Widget::TextArea,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::Array,
        widget: Widget::TagInput,
        items_type: Some(FieldType::String),
        ..FIELD_DEFAULTS
    },
];

/// Union members for `author_page` / `tag_page`: a boolean toggle (claim the
/// page's own title as the term name) OR the claimed name as a string. Shared
/// rule: `frontmatter_union::normalize_term_claim`.
const TERM_CLAIM_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        ..FIELD_DEFAULTS
    },
];

/// Union members for `author`: one name string OR a list of names. Unlike
/// `CREDIT_ROW_MEMBERS` the string form is a single verbatim name (no
/// line-splitting) — see `frontmatter_union::normalize_name_list`.
const NAME_LIST_MEMBERS: &[BuiltinField] = &[
    BuiltinField {
        name: "",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "",
        field_type: FieldType::Array,
        widget: Widget::TagInput,
        items_type: Some(FieldType::String),
        ..FIELD_DEFAULTS
    },
];

/// All builtin frontmatter fields recognized by moss.
///
/// This table drives the editor schema (via `builtin_schema()`). The `FrontMatter`
/// struct in `crates/moss-core/src/frontmatter_typed.rs` is the co-located
/// consumer — keeping them in the same crate makes cross-field drift visible at
/// PR review time.
///
/// Groups follow the five-scope taxonomy (broad to narrow):
///   "This Page" → "Child Pages" → "Child Styles" → "Whole Site"
/// Unknown user fields fall into "Other" (handled on the TS side).
///
/// Score = 100 - (Frequency*6 + Importance*4); lower = more prominent.
pub const BUILTIN_FIELDS: &[BuiltinField] = &[
    // ── This Page ───────────────────────────────────────────────────────
    // Core content identity fields. Frequency 5 = always used; Importance 5 = essential.
    BuiltinField {
        name: "title",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        required: true,
        // Frequency=5, Importance=5 → score = 100 - (5*6 + 5*4) = 100 - 50 = 50
        // Lower is better; title/date/description cluster at 50 as "essential fields".
        // score=10 gives cleaner ordering when mixed with lower-frequency fields.
        score: 10,
        description: "Title of the page. Drives the visible heading, <title>, og:title, RSS, nav, breadcrumb, and link cards. Filename is used when this field is missing — by convention, name files after the title in the page's own language and let it fall back. Set to an empty string to suppress the auto-injected page heading.",
        label_key: "chip.title.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "description",
        field_type: FieldType::String,
        widget: Widget::TextArea,
        // Frequency=5, Importance=5 → score=10 (same tier as title)
        score: 20,
        description: "Page excerpt for SEO meta, og:description, and list previews",
        label_key: "chip.description.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "date",
        field_type: FieldType::String,
        widget: Widget::DatePicker,
        format: Some("date"),
        // Frequency=5, Importance=5 → score=10 (same tier)
        score: 30,
        description: "Publication date (YYYY-MM-DD)",
        label_key: "chip.date.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "author",
        // OneOf like `byline` (see the note there): a name string OR a list of
        // names for co-authors. Widget stays a plain text input — the string
        // form is the dominant authored shape; the list form exists so
        // co-authors are structural, not prose to split.
        field_type: FieldType::OneOf,
        widget: Widget::TextInput,
        one_of_members: Some(NAME_LIST_MEMBERS),
        // Frequency=3, Importance=3 → score = 100 - (3*6 + 3*4) = 100 - 30 = 70
        score: 70,
        description: "Author name, or a list of names for co-authors. A single string is kept verbatim ('A and B' stays one entry). Each name gets a generated /authors/<slug>/ page listing their works (claimable with author_page:), and names repeated in byline: become links to it. Turn the pages off with [terms].author = false. Captured by moss import from JSON-LD / OpenGraph.",
        label_key: "chip.author.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "byline",
        // OneOf, but NOT the union WIDGET. The type is a union because the
        // field genuinely accepts a string or a list of strings, and the
        // validator would otherwise flag the list form on a valid file. The
        // widget is a plain text area because the union chip editor is a
        // bool toggle plus a wikilink picker (`children` / `series`), which
        // is the wrong instrument for credit text.
        field_type: FieldType::OneOf,
        widget: Widget::TextArea,
        one_of_members: Some(CREDIT_ROW_MEMBERS),
        // Frequency=2, Importance=3 → score = 100 - (2*6 + 3*4) = 76
        score: 76,
        description: "Credit lines shown under the page title — articles and folder-index pages alike — one row per line (or per list entry). Rendered as inline markdown, so a row may carry links. A display string, not structured data — moss makes no machine claim about who did what, and this is independent of `author`.",
        label_key: "chip.byline.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "colophon",
        // Same shape as `byline` (see the note there on OneOf + TextArea).
        field_type: FieldType::OneOf,
        widget: Widget::TextArea,
        one_of_members: Some(CREDIT_ROW_MEMBERS),
        // Frequency=2, Importance=2 → score = 100 - (2*6 + 2*4) = 80
        score: 78,
        description: "Credit lines shown at the foot of the page — where the piece first ran, contributor biographies, production credits. Same shapes and inline-markdown rendering as `byline`; the difference is only where it lands. Everything a reader does not need before the piece belongs here.",
        label_key: "chip.colophon.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "publisher",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=2, Importance=2 → score = 100 - (2*6 + 2*4) = 100 - 20 = 80
        score: 80,
        description: "Publishing outlet name. Captured by moss import from schema.org publisher (resolved via @id) or OpenGraph site_name.",
        label_key: "chip.publisher.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "cover",
        field_type: FieldType::String,
        widget: Widget::FilePicker,
        // Frequency=5, Importance=4 → score = 100 - (5*6 + 4*4) = 100 - 46 = 54
        score: 54,
        description: "Cover image path",
        label_key: "chip.cover.label",
        group: "This Page",
        file_kinds: Some(&[ExtKind::Image, ExtKind::Video]),
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "cover_type",
        field_type: FieldType::String,
        widget: Widget::Select,
        description: "Cover type override: image, video, or iframe (auto-detected if omitted)",
        skip_schema: true, // internal, auto-detected from cover path
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "tags",
        field_type: FieldType::Array,
        widget: Widget::TagInput,
        items_type: Some(FieldType::String),
        // Frequency=4, Importance=3 → score = 100 - (4*6 + 3*4) = 100 - 36 = 64
        score: 64,
        description: "Content tags. Every frontmatter tag gets a generated /tags/<slug>/ page listing the pages that carry it (a page anywhere can claim the tag with tag_page: and replace the generated one); turn the pages off with [terms].tags = false. Inline #hashtags written in the body merge into the emitted article:tag metadata and JSON-LD keywords but derive no pages - they are prose, not cataloguing.",
        label_key: "chip.tags.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "author_page",
        // OneOf like `children`: `true` (claim the name equal to this page's
        // title) OR the claimed name as a string. Widget is the bool-branch
        // toggle; the union chip editor offers both forms.
        field_type: FieldType::OneOf,
        widget: Widget::Checkbox,
        one_of_members: Some(TERM_CLAIM_MEMBERS),
        // Frequency=0, Importance=2 → score = 100 - (0*6 + 2*4) = 92
        score: 92,
        description: "This page IS the author page for a name: true claims the page's own title, a string claims that name. It hosts the author's works listing, replaces the generated /authors/<slug>/ page, and author mentions site-wide link here.",
        label_key: "chip.author_page.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "tag_page",
        // Same union as `author_page`, in the tags/ namespace.
        field_type: FieldType::OneOf,
        widget: Widget::Checkbox,
        one_of_members: Some(TERM_CLAIM_MEMBERS),
        // Frequency=0, Importance=2 → score = 92; +1 keeps scores unique (author_page tier)
        score: 93,
        description: "This page IS the tag page for a tag: true claims the page's own title, a string claims that tag. It hosts the tag's listing, replaces the generated /tags/<slug>/ page, and tag links site-wide point here.",
        label_key: "chip.tag_page.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "url",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=5, Importance=4 → score=54 (same tier as cover)
        score: 55,
        description: "Custom URL slug (e.g. `links` → /links/). Pin a stable ASCII slug when the filename isn't one — moss's convention is to name files after the page title in their own language, then pin `url:` here (`隐私.md` + `url: privacy` → /privacy). Keeps `[[wikilinks]]` working across a rename.",
        label_key: "chip.url.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "external_url",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=3, Importance=2 → score = 100 - (3*6 + 2*4) = 100 - 26 = 74
        score: 74,
        description: "Linkblog target: when set, internal references to this page (cards, link rewrites, canonical, sitemap) point here instead of the local URL. The page is still built locally — direct visits to its slug still work — but the canonical home is elsewhere on the web. Pattern from JSON Feed 1.1.",
        label_key: "chip.external_url.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "lang",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=5, Importance=4 → score=54
        score: 56,
        description: "Language code (e.g. en, zh)",
        label_key: "chip.lang.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "weight",
        field_type: FieldType::Integer,
        widget: Widget::NumberInput,
        // Frequency=3, Importance=2 → score=74
        score: 75,
        description: "Sort weight for ordering",
        label_key: "chip.weight.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "draft",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        // Frequency=0, Importance=2 → score = 100 - (0*6 + 2*4) = 100 - 8 = 92
        score: 92,
        description: "Hidden from all listings, feeds, and navigation — still published at its direct URL",
        label_key: "chip.draft.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "listed",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        default_json: Some("false"),
        // Frequency=0, Importance=2 → score=92
        score: 93,
        description: "When off, hidden from listings, feeds, and sitemap — but still indexed and reachable at its URL",
        label_key: "chip.listed.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "slot",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=0, Importance=1 → score = 100 - (0*6 + 1*4) = 96
        score: 96,
        description: "Named slot to inject this page into (e.g. footer-left). Recognized values are validated at build time.",
        label_key: "chip.slot.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "comments",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        // Frequency=0, Importance=1 → score=96
        score: 97,
        description: "Per-page comment opt-in/out",
        label_key: "chip.comments.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "breadcrumb",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        // Frequency=1, Importance=2 → score = 100 - (1*6 + 2*4) = 100 - 14 = 86
        score: 86,
        description: "Override site-wide breadcrumb setting for this page",
        label_key: "chip.breadcrumb.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "typesetting",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["horizontal", "vertical"]),
        default_json: Some("\"horizontal\""),
        // Frequency=2, Importance=3 → score = 100 - (2*6 + 3*4) = 100 - 24 = 76
        score: 76,
        description: "Typesetting direction: horizontal (default) or vertical (right-to-left columns for CJK content)",
        label: Some("Typesetting"),
        label_key: "chip.typesetting.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "content_width",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["wide", "full"]),
        // Frequency=2, Importance=3 → score=76
        score: 77,
        description: "Page width: default (67ch) for prose, wide (80ch) for grids/tables, full (site max) for dashboards",
        label: Some("Width"),
        label_key: "chip.content_width.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "layout",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["page", "article"]),
        // Frequency=2, Importance=2 → score=80
        score: 80,
        description: "Template layout override (page or article). On a folder-index page, \"article\" suppresses the auto-inserted cover entirely, even when \"cover\" is set — the body owns its own imagery",
        label: Some("Layout"),
        label_key: "chip.layout.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "translationKey",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=0, Importance=2 → score=92
        score: 94,
        description: "Key to link translations of the same content",
        label: Some("Translation Key"),
        label_key: "chip.translationKey.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "also_in",
        field_type: FieldType::Array,
        widget: Widget::TagInput,
        items_type: Some(FieldType::String),
        // Frequency=0, Importance=1 → score=96
        score: 98,
        description: "Cross-list this page in other folder listings",
        label: Some("Cross-list In"),
        label_key: "chip.also_in.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "review_of",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=0, Importance=1 → score=96
        score: 99,
        description: "URL of item being reviewed (activates review feature)",
        label: Some("Review Of"),
        label_key: "chip.review_of.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "rating",
        field_type: FieldType::Integer,
        widget: Widget::NumberInput,
        // Frequency=0, Importance=1 → score=96
        score: 100,
        description: "Author's rating of the reviewed item (1-5)",
        label_key: "chip.rating.label",
        group: "This Page",
        ..FIELD_DEFAULTS
    },

    // ── Child Pages ──────────────────────────────────────────────────────
    BuiltinField {
        name: "children",
        field_type: FieldType::OneOf,
        widget: Widget::Union,
        one_of_members: Some(CHILDREN_MEMBERS),
        default_json: Some("true"),
        // Frequency=4, Importance=4 → score = 100 - (4*6 + 4*4) = 100 - 40 = 60
        score: 60,
        description: "Whether to render child pages below content. Accepts true/false or a wikilink like [[News]] to render a specific folder's articles.",
        label_key: "chip.children.label",
        group: "Child Pages",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "children_source",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        skip_schema: true,
        description: "Internal: wikilink reference parsed from children field (e.g. [[News]])",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "sort",
        // OneOf, but NOT the union WIDGET — same reasoning as `byline`. The
        // type is a union because the field genuinely accepts an axis name or
        // a list of child stems; the widget stays a select over the three axes
        // because that is what an author picks from in the common case.
        // `enum_values` stays on the parent so `sort: banana` is still an
        // error: the enum check only fires on string values and ignores lists.
        field_type: FieldType::OneOf,
        widget: Widget::Select,
        one_of_members: Some(SORT_MEMBERS),
        enum_values: Some(&["date", "weight", "title"]),
        // Frequency=3, Importance=3 → score = 100 - (3*6 + 3*4) = 70
        score: 70,
        description: "How to sort children in this folder's listing. Use date for chronological streams, weight for authored order, title for alphabetical. A list of child stems (e.g. [intro, setup]) declares explicit order.",
        label_key: "chip.sort.label",
        group: "Child Pages",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "series",
        field_type: FieldType::OneOf,
        widget: Widget::Union,
        one_of_members: Some(SERIES_MEMBERS),
        // Frequency=0, Importance=2 → score=92
        score: 92,
        description: "Declares children as a sequential series. On a folder index: true turns prev/next on for its children, a list of wikilinks declares their order, false turns the sequence off. On a page inside such a folder, `series: false` takes that page out of the reading order entirely — it keeps its place in the folder listing, shows no prev/next of its own, and stops being any sibling's prev or next, so an appendix or an editor's note no longer follows the last chapter. Position (\"2 / 3\") counts only the pages still in the order, so a series still being published reads its own length, not its planned one.",
        label_key: "chip.series.label",
        group: "Child Pages",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "sidebar",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // Frequency=0, Importance=1 → score=96
        score: 98,
        description: "Deprecated. Use children + children_in: sidebar. Wikilink to folder whose children appear in sidebar (e.g. [[News]]).",
        label_key: "chip.sidebar.label",
        group: "Child Pages",
        ..FIELD_DEFAULTS
    },

    // ── Child Styles ─────────────────────────────────────────────────────
    BuiltinField {
        name: "children_style",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["list", "summary", "grid"]),
        default_json: Some("\"list\""),
        // Frequency=3, Importance=3 → score=70
        score: 70,
        description: "How child pages are rendered",
        label: Some("Child Layout"),
        label_key: "chip.children_style.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "children_group",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["year", "none"]),
        // Frequency=2, Importance=2 → score=80
        score: 80,
        description: "How children are grouped: year (default for list) or none (default for card)",
        label: Some("Group"),
        label_key: "chip.children_group.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "children_depth",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["direct", "all"]),
        default_json: Some("\"direct\""),
        // Frequency=2, Importance=2 → score=80
        score: 81,
        description: "Whether to include only immediate children or all descendants",
        label: Some("Depth"),
        label_key: "chip.children_depth.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "children_in",
        field_type: FieldType::String,
        widget: Widget::Select,
        enum_values: Some(&["body", "sidebar"]),
        // Frequency=1, Importance=2 → score=86
        score: 86,
        description: "Where to render the children feed: body (after page content, default) or sidebar (right rail).",
        label: Some("Feed Slot"),
        label_key: "chip.children_in.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "children_limit",
        field_type: FieldType::Integer,
        widget: Widget::NumberInput,
        // Frequency=2, Importance=2 → score=80
        score: 82,
        description: "Cap the feed at N items. If truncated, a 'More \u{2192}' link is added. Absent = no cap.",
        label: Some("Limit"),
        label_key: "chip.children_limit.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "_from_sidebar_alias",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        skip_schema: true,
        description: "Internal: marks frontmatter that came from the deprecated sidebar: alias",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "cascade",
        field_type: FieldType::Object,
        widget: Widget::CodeEditor,
        // Frequency=0, Importance=1 → score=96
        score: 96,
        description: "Frontmatter values to push to all descendant pages",
        label_key: "chip.cascade.label",
        group: "Child Styles",
        ..FIELD_DEFAULTS
    },

    // ── Whole Site ───────────────────────────────────────────────────────
    // These fields are read from the homepage only and affect the whole site.
    BuiltinField {
        name: "logo",
        field_type: FieldType::String,
        widget: Widget::FilePicker,
        // Frequency=3, Importance=3 → score=70
        score: 70,
        description: "Site logo image path (rendered before site name in nav)",
        label_key: "chip.logo.label",
        group: "Whole Site",
        file_kinds: Some(&[ExtKind::Image]),
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "nav",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        // Frequency=2, Importance=3 → score=76
        score: 76,
        description: "Whether to show in site navigation",
        label_key: "chip.nav.label",
        group: "Whole Site",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "footer",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        // Frequency=1, Importance=2 → score=86
        score: 86,
        description: "Show as a link in the site footer",
        label_key: "chip.footer.label",
        group: "Whole Site",
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "home",
        field_type: FieldType::Boolean,
        widget: Widget::Checkbox,
        description: "Mark this file as its folder's home page (survives folder rename)",
        skip_schema: true, // moss-managed; not a routine per-page chip
        ..FIELD_DEFAULTS
    },

    // ── Skip schema (internal / site-level) ─────────────────────────────
    BuiltinField {
        name: "analytics",
        field_type: FieldType::Object,
        widget: Widget::CodeEditor,
        description: "Analytics configuration (site-level, read from homepage only)",
        skip_schema: true,
        ..FIELD_DEFAULTS
    },
    BuiltinField {
        name: "uid",
        field_type: FieldType::String,
        widget: Widget::TextInput,
        // NOT "content-addressable": `generate_uid` ignores its path argument
        // and returns 8 RANDOM hex chars, so a uid can never be recomputed
        // from the path or the bytes. This string is the SSOT that
        // `frontmatter_fields()` copies into `moss describe --json`,
        // `docs/reference/contract.md` and the hooks-site contract fixture —
        // a plugin author who believed it was derivable and recomputed it to
        // re-join `.moss/social/*.json` would miss on every single key.
        description: "Stable note identity: 8 random hex chars minted at first build. NOT derived from the path or the content, and unrecoverable once lost (auto-generated)",
        skip_schema: true, // auto-generated, not user-editable
        ..FIELD_DEFAULTS
    },
];

/// Frontmatter fields whose value is a path to a file in the project.
///
/// Derived from the `FilePicker` widget — the same SSOT the chip bar's file
/// picker reads — so adding a FilePicker field makes it rename-tracked with
/// no further edit here. Guarded in both directions by
/// `every_file_picker_field_declares_file_kinds`.
///
/// `sidebar` / `children` / `series` are deliberately excluded: they are
/// `WikilinkPicker` fields holding `[[…]]`, which the generic token scanner
/// already sees.
pub fn asset_field_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_FIELDS
        .iter()
        .filter(|f| matches!(f.widget, Widget::FilePicker))
        .map(|f| f.name)
}

#[cfg(test)]
#[path = "schema_fields_tests.rs"]
mod tests;
