//! Typed frontmatter structs for the build pipeline.
//!
//! Lives in moss-core so validation, the resolver, and src-tauri's pipeline
//! all share one definition. See ADR-018 for the boundary rule.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

/// Series declaration field: sequential mode (`series: true`) or explicit
/// wikilink order (`series: ["[[Ch 1]]", "[[Ch 2]]"]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(untagged)]
pub enum SeriesField {
    /// `series: true` — sequential mode, sort children by weight.
    Flag(bool),
    /// `series: ["[[Ch 1]]", "[[Ch 2]]"]` — explicit wikilink order.
    Ordered(Vec<String>),
}

/// Analytics configuration for script injection.
///
/// Supports two frontmatter formats:
/// - String shorthand: `analytics: "https://guo.goatcounter.com/count"` (provider auto-detected from URL)
/// - Object form: `analytics: { provider: goatcounter, url: "..." }`
#[derive(Debug, Serialize, Default, Clone)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AnalyticsConfig {
    /// Analytics provider: "goatcounter", "umami" (default when absent)
    pub provider: Option<String>,
    /// URL — script src for Umami, count endpoint for GoatCounter
    pub url: String,
    /// Site ID for the analytics service (Umami only)
    pub site_id: Option<String>,
}

impl<'de> serde::Deserialize<'de> for AnalyticsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct AnalyticsVisitor;

        impl<'de> de::Visitor<'de> for AnalyticsVisitor {
            type Value = AnalyticsConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a URL string or an analytics config object")
            }

            fn visit_str<E: de::Error>(self, url: &str) -> Result<Self::Value, E> {
                Ok(AnalyticsConfig {
                    provider: None,
                    url: url.to_string(),
                    site_id: None,
                })
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: de::MapAccess<'de>,
            {
                #[derive(Deserialize)]
                struct Inner {
                    provider: Option<String>,
                    url: String,
                    site_id: Option<String>,
                }
                let inner =
                    Inner::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(AnalyticsConfig {
                    provider: inner.provider,
                    url: inner.url,
                    site_id: inner.site_id,
                })
            }
        }

        deserializer.deserialize_any(AnalyticsVisitor)
    }
}

impl AnalyticsConfig {
    /// Generate the HTML script tag for this analytics configuration.
    /// When `provider` is None, auto-detects from the URL domain.
    pub fn to_script_tag(&self) -> String {
        let provider = self.provider.as_deref().unwrap_or_else(|| {
            if self.url.contains("goatcounter.com") {
                "goatcounter"
            } else {
                "umami"
            }
        });
        match provider {
            "goatcounter" => {
                format!(
                    r#"<script data-goatcounter="{}" async src="//gc.zgo.at/count.js"></script>"#,
                    self.url
                )
            }
            _ => {
                format!(
                    r#"<script defer src="{}" data-website-id="{}"></script>"#,
                    self.url,
                    self.site_id.as_deref().unwrap_or("")
                )
            }
        }
    }

    /// Generate a tracking pixel URL for the given path, if the provider supports it.
    /// GoatCounter's /count endpoint returns a 1x1 GIF when accessed without JavaScript,
    /// making it suitable as an `<img>` src for RSS feed tracking.
    pub fn to_pixel_url(&self, path: &str) -> Option<String> {
        let provider = self.provider.as_deref().unwrap_or_else(|| {
            if self.url.contains("goatcounter.com") {
                "goatcounter"
            } else {
                "umami"
            }
        });
        match provider {
            "goatcounter" => {
                let clean_path = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{}", path) // allow:served-path-url-construct (GoatCounter analytics page path, not a framework asset URL)
                };
                Some(format!("{}?p={}", self.url, clean_path))
            }
            _ => None,
        }
    }
}

/// Frontmatter structure for parsing YAML metadata from markdown files
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct FrontMatter {
    /// Optional title override from frontmatter
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub title: Option<String>,
    /// Optional publication date
    pub date: Option<String>,
    /// Navigation weight for ordering (lower numbers = higher priority)
    pub weight: Option<i32>,
    /// Custom URL slug (e.g., "links" -> "/links/"). Pin a stable ASCII slug when
    /// the filename isn't one — moss's convention is to name files after the page
    /// title in their own language, then pin `url:` here (隐私.md + url: privacy -> /privacy).
    /// Takes priority over filename-based slug generation.
    pub url: Option<String>,
    /// Author name(s). A single string is one name kept verbatim (including
    /// pre-formatted forms like "A and B"); a YAML list is one name per entry
    /// for co-authors. Normalized by
    /// [`crate::frontmatter_union::normalize_name_list`]; serialized back as a
    /// plain string when there is exactly one name, so the dominant
    /// single-author form round-trips unchanged. Captured automatically by
    /// `moss import` from JSON-LD / OpenGraph metadata.
    #[serde(
        default,
        deserialize_with = "deserialize_name_list",
        serialize_with = "serialize_name_list"
    )]
    pub author: Option<Vec<String>>,
    /// Term-page claim: this page IS the term page for an author name.
    /// `true` claims the name equal to the page's own title; a string claims
    /// that name explicitly. The claiming page hosts the author's works
    /// listing and replaces the generated `/authors/<slug>/` page; term links
    /// site-wide point here. See `moss_core::terms`.
    #[serde(
        default,
        deserialize_with = "deserialize_term_claim",
        serialize_with = "serialize_term_claim"
    )]
    pub author_page: Option<crate::terms::TermClaim>,
    /// Term-page claim for a tag — same shapes and behaviour as
    /// `author_page`, in the `tags/` namespace.
    #[serde(
        default,
        deserialize_with = "deserialize_term_claim",
        serialize_with = "serialize_term_claim"
    )]
    pub tag_page: Option<crate::terms::TermClaim>,
    /// Byline shown under the article title: the credit lines a reader sees,
    /// as the author wrote them. One row per list entry, or per line of a
    /// block scalar:
    ///
    /// ```yaml
    /// byline: |
    ///   作者　糜緒洋
    ///   編輯　謝丁
    ///   首發媒體　[端傳媒](https://…)
    /// ```
    ///
    /// A display string, not structured data: moss renders each row as inline
    /// markdown and makes no machine claim about who did what. That is
    /// deliberate — `author:` is used for the *editor* on real sites, so
    /// asserting roles from credit text would put false statements in
    /// structured data. Independent of `author:`, which stays a plain name.
    #[serde(default, deserialize_with = "deserialize_credit_rows")]
    pub byline: Option<Vec<String>>,
    /// Colophon: the same kind of credit rows as `byline`, rendered at the
    /// FOOT of the page instead of under the title.
    ///
    /// ```yaml
    /// colophon: |
    ///   首發媒體　[端傳媒](https://…)、[單讀](https://…)
    ///   封面　基輔米迦勒修道院門口的陣亡將士紀念牆（拍攝：糜緒洋）
    ///   編輯　謝丁，記者、作家，曾任《正午》主編
    /// ```
    ///
    /// Two fields rather than one because publications agree on the split: a
    /// byline holds the two or three names a reader needs before the piece,
    /// and everything else — contributor biographies, where it first ran,
    /// production credits — belongs after it. Same shapes, same inline
    /// markdown, same "display string, no machine claims" rule.
    #[serde(default, deserialize_with = "deserialize_credit_rows")]
    pub colophon: Option<Vec<String>>,
    /// Publishing outlet name (for imported pages). `moss import` resolves this
    /// from schema.org `publisher` (via `@id` ref to an `Organization` entry)
    /// or OpenGraph `og:site_name`, falling back to the URL host.
    pub publisher: Option<String>,
    /// Linkblog target: when set, internal references to this page (cards,
    /// link rewrites, canonical, sitemap) point here instead of the local URL.
    /// The page is still built locally — direct visits to its slug still
    /// work — but the canonical home is elsewhere on the web. Pattern from
    /// [JSON Feed 1.1](https://www.jsonfeed.org/version/1.1/): `external_url`
    /// is the same as the href in a linkblog post.
    ///
    /// This is a manual linkblog field. Note: `moss import` does NOT set it —
    /// an import is the user's own content (POSSE), so the vault copy is
    /// canonical and the source URL is recorded in `syndicated` instead. (Before
    /// 2026-07 import wrote `external_url`; that was reversed.)
    ///
    /// `source_url` alias — accepts existing files with the previous (one-PR)
    /// field name without breaking. Single-direction back-compat: writes use
    /// `external_url` only. Remove the alias one release after merge.
    #[serde(alias = "source_url")]
    pub external_url: Option<String>,
    /// Analytics configuration for privacy-focused analytics
    pub analytics: Option<AnalyticsConfig>,
    /// Site logo image path (rendered before site name in nav)
    pub logo: Option<String>,
    /// Cover image URL for collection pages
    pub cover: Option<String>,
    /// Explicit cover type override: "image", "video", or "iframe"
    pub cover_type: Option<String>,
    /// Whether to show in navigation
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub nav: Option<bool>,
    /// Explicit folder-home marker: this file is its folder's home page,
    /// regardless of filename. Written by moss on homes it creates; survives rename.
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub home: Option<bool>,
    /// Draft: rendered and published at its direct URL, but hidden from all
    /// listings, feeds, sitemap, and navigation (and marked `noindex`).
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub draft: Option<bool>,
    /// Listed: when `false`, the page is hidden from moss's auto-generated
    /// surfaces (home feed, recent, folder embeds, RSS, llms.txt, sitemap, and
    /// auto sidebar listings) but remains indexable, share-cardable, and
    /// reachable at its direct URL. Absent ⇒ listed. Orthogonal to `draft`:
    /// `draft` adds `noindex` and drops the share card; `listed: false` does
    /// neither. Nav bar / footer link placement (explicit `nav:` / `footer:`)
    /// are independent of this flag.
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub listed: Option<bool>,
    /// Page description for SEO and list previews
    pub description: Option<String>,
    /// Content tags for organization
    pub tags: Option<Vec<String>>,
    /// Whether to render child pages below content.
    /// Accepts bool (true/false) or a wikilink/path string like "[[News]]".
    /// The lenient deserializer resolves ANY non-bool string to `Some(true)`
    /// (= render children on); the folder reference itself is extracted into
    /// `children_source` by `crate::frontmatter_union::normalize_children`
    /// in the pipeline. This `Option<bool>` is the RESOLVED form the render
    /// layer consumes — its type is unchanged so no render-layer ripple.
    #[serde(default, deserialize_with = "deserialize_children_lenient")]
    pub children: Option<bool>,
    /// Wikilink reference for targeted children rendering (e.g. "[[News]]").
    /// When set, only the referenced folder's articles are rendered as
    /// children, instead of all direct children of the current page.
    pub children_source: Option<String>,
    /// Wikilink to folder whose children appear in sidebar (e.g. "[[News]]")
    pub sidebar: Option<String>,
    /// How child pages are rendered: "list" (default), "card"
    pub children_style: Option<String>,
    /// How children are grouped: "year" or "none"
    pub children_group: Option<String>,
    /// What children to include: "direct" (default), "all" descendants
    pub children_depth: Option<String>,
    /// Where to render the children feed: "body" (default) or "sidebar".
    /// Resolved at consumer; absent means body.
    pub children_in: Option<String>,
    /// Cap the children feed at N items. If truncated, a "More →" link
    /// is added. Absent = no cap.
    pub children_limit: Option<u32>,
    /// Internal: marks frontmatter that came from the deprecated `sidebar:` alias.
    /// Used by the sidebar callsite to apply the legacy default-3 limit on cross-ref.
    /// Skip-serialize so the form doesn't round-trip the synthetic flag back into the file.
    ///
    /// The sidebar callsite reads this flag (not `sidebar.is_some()`) so a
    /// conflict like `sidebar: "[[A]]" + children: "[[B]]"` — where the alias
    /// yields and warns "sidebar ignored" — actually skips the right rail
    /// rather than rendering it. Removed alongside the alias itself (#633).
    #[serde(skip_serializing, default)]
    pub _from_sidebar_alias: Option<bool>,
    /// Listing sort: axis (date/weight/title) or explicit list of child stems.
    #[serde(alias = "order")]
    pub sort: Option<crate::sort::SortField>,
    /// Series declaration: bool for prev/next chrome. Legacy list form
    /// (SeriesField::Ordered) preserved for back-compat but normalized away
    /// at deserialize time — see FrontMatter::normalize (Task 5).
    pub series: Option<SeriesField>,
    /// Override site-wide breadcrumb setting
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub breadcrumb: Option<bool>,
    /// Override site-wide footer setting
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub footer: Option<bool>,
    /// Frontmatter values to cascade to all descendants
    pub cascade: Option<HashMap<String, Value>>,
    /// Folder paths where this article also appears in lists
    #[serde(alias = "also")]
    pub also_in: Option<Vec<String>>,
    /// Language override (e.g., "en", "zh-hans", "zh-hant")
    pub lang: Option<String>,
    /// Translation key for linking arbitrary files as translations
    #[serde(rename = "translationKey")]
    pub translation_key: Option<String>,
    /// Whether to show comments on this page (default: true)
    #[serde(default, deserialize_with = "deserialize_bool_lenient")]
    pub comments: Option<bool>,
    /// Durable page identity: 8 RANDOM hex chars minted at first build — never derivable, never changed once published (docs/reference/social-data-standard.md)
    #[serde(default, deserialize_with = "deserialize_string_lenient")]
    pub uid: Option<String>,
    /// Typesetting direction: "horizontal" (default) or "vertical"
    pub typesetting: Option<String>,
    /// Content width preset: "wide" or "full"
    pub content_width: Option<String>,
    /// Template layout override: "page" or "article"
    pub layout: Option<String>,
    /// URL of item being reviewed (activates review feature for this page)
    pub review_of: Option<String>,
    /// Author's rating of the reviewed item (1-5)
    pub rating: Option<u8>,
    /// Named slot this page injects into (e.g. `footer-left`).
    /// Validation against the recognized slot vocabulary happens at consumer
    /// time in the build pipeline, so authors get a deferred warning rather
    /// than a hard parse error.
    pub slot: Option<String>,
}

impl FrontMatter {
    /// One-time normalization after deserialize: consume legacy
    /// `series: [list]` (SeriesField::Ordered) into `sort: List + series: Flag(true)`.
    /// Idempotent. If `sort:` is already set explicitly, only the `series` field
    /// flips to Flag(true) (preserving the chrome-implied semantics).
    pub fn normalize(&mut self) {
        if let Some(SeriesField::Ordered(items)) = &self.series {
            if self.sort.is_none() {
                self.sort = Some(crate::sort::SortField::List(items.clone()));
            }
            self.series = Some(SeriesField::Flag(true));
        }
    }
}

/// One dropped field, for build advisories and (later) chip diagnostics (ADR-020).
///
/// Only `Dropped` outcomes exist today — a field whose value couldn't satisfy
/// its typed field and was removed so its neighbours survive. Severity tiers
/// (coerced/lossy) are added in Phase 3c alongside their first real consumer
/// (the chip UI), per the "consumer before it ships" rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldWarning {
    /// Frontmatter key that was dropped.
    pub key: String,
    /// Author-facing message (the serde error that rejected the value).
    pub message: String,
}

/// Project the canonical parsed frontmatter map into the typed `FrontMatter`.
///
/// The single typed projection shared by the build (publish) and, later, the
/// editor. `serde_yaml::from_value` reuses every `#[derive(Deserialize)]` +
/// `deserialize_with` on `FrontMatter` — no per-field code — and serde_yaml
/// coerces YAML scalars, so a numeric uid/title becomes a string here.
///
/// Resilience (ADR-020): if a value genuinely cannot satisfy its typed field
/// (e.g. `weight: high`, or an `analytics` object missing its required `url`),
/// that ONE field is dropped and every good neighbour survives. Offending
/// fields are found by deserializing each field in ISOLATION — this is
/// serde-driven, not schema-driven, so it also covers `skip_schema` fields and
/// custom-deserializer fields the validation engine can't see (the gap that an
/// earlier schema-driven attempt missed). Pure; no I/O.
pub fn project_typed(values: &serde_yaml::Mapping) -> (FrontMatter, Vec<FieldWarning>) {
    // Happy path: serde_yaml coerces scalars (numeric uid/title → string) and
    // ignores unknown fields, so all coercible inputs succeed with no warnings.
    if let Ok(fm) = serde_yaml::from_value::<FrontMatter>(serde_yaml::Value::Mapping(values.clone())) {
        return (fm, Vec::new());
    }
    // One or more fields can't satisfy the typed schema. Identify them by
    // deserializing each field in ISOLATION: `FrontMatter` fields are
    // independent `Option`s with no required cross-field state, so a field that
    // fails on its own is exactly a field that poisons the whole struct. Drop
    // those, keep the rest. Unknown fields are ignored by `from_value` and so
    // never appear here.
    let mut sanitized = values.clone();
    let mut warnings: Vec<FieldWarning> = Vec::new();
    for (k, v) in values.iter() {
        let Some(key) = k.as_str() else { continue };
        let mut single = serde_yaml::Mapping::new();
        single.insert(k.clone(), v.clone());
        if let Err(e) = serde_yaml::from_value::<FrontMatter>(serde_yaml::Value::Mapping(single)) {
            sanitized.remove(k);
            warnings.push(FieldWarning {
                key: key.to_string(),
                message: format!("{key}: {e}"),
            });
        }
    }
    // With every poisoning field removed, the re-projection succeeds. The
    // defensive `unwrap_or_default` covers only the theoretical residual case
    // of a cross-field interaction (none exist in `FrontMatter` today).
    let fm = serde_yaml::from_value::<FrontMatter>(serde_yaml::Value::Mapping(sanitized))
        .unwrap_or_default();
    (fm, warnings)
}

/// Deserialize a credit-row union (`byline`, `colophon`): one string, or a
/// list of strings.
///
/// Hand-rolled rather than `#[serde(untagged)]`. An untagged enum reports
/// every mismatch as "data did not match any variant of untagged enum" — a
/// message that names neither the field nor what was wrong, and which
/// `project_typed`'s per-field isolation would then hand to the author
/// verbatim. Going through [`crate::frontmatter_union::normalize_credit_rows`]
/// gives the author "byline: expected a string or a list of strings, found a
/// number" AND keeps one definition of what the shapes mean, shared with the
/// editor.
///
/// An empty result (`byline: ""`, `byline: []`, blank lines only) is `None`:
/// nothing to display is the same as absent, and no empty row is emitted.
pub fn deserialize_credit_rows<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Value::deserialize(deserializer)?;
    let rows = crate::frontmatter_union::normalize_credit_rows(&value).map_err(D::Error::custom)?;
    Ok(if rows.is_empty() { None } else { Some(rows) })
}

/// Deserialize the `author` union: one name string, or a list of names.
/// Same hand-rolled-over-untagged rationale as [`deserialize_credit_rows`];
/// the shape rules live in [`crate::frontmatter_union::normalize_name_list`].
pub fn deserialize_name_list<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Value::deserialize(deserializer)?;
    let names = crate::frontmatter_union::normalize_name_list(&value).map_err(D::Error::custom)?;
    Ok(if names.is_empty() { None } else { Some(names) })
}

/// Deserialize a term-claim union (`author_page`, `tag_page`): `true`, or the
/// claimed name as a string. Shape rules live in
/// [`crate::frontmatter_union::normalize_term_claim`].
pub fn deserialize_term_claim<'de, D>(
    deserializer: D,
) -> Result<Option<crate::terms::TermClaim>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Value::deserialize(deserializer)?;
    crate::frontmatter_union::normalize_term_claim(&value).map_err(D::Error::custom)
}

/// Serialize a term claim back to its authored form: `UseTitle` → `true`,
/// `Name(s)` → the string.
pub fn serialize_term_claim<S>(
    v: &Option<crate::terms::TermClaim>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use crate::terms::TermClaim;
    use serde::Serialize;
    match v {
        Some(TermClaim::UseTitle) => true.serialize(serializer),
        Some(TermClaim::Name(n)) => n.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// Serialize `author` back to its dominant authored form: exactly one name
/// emits a plain string (`author: 馬欣宜`), several emit a list. Keeps the
/// single-author round-trip byte-shape stable for every existing file.
pub fn serialize_name_list<S>(v: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    match v.as_deref() {
        Some([one]) => one.serialize(serializer),
        other => other.serialize(serializer),
    }
}

/// Deserialize a bool that may be a YAML string ("true"/"false") or a native bool.
/// Returns None for missing values, Some(bool) for valid values, errors for invalid strings.
pub fn deserialize_bool_lenient<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct BoolLenientVisitor;

    impl<'de> de::Visitor<'de> for BoolLenientVisitor {
        type Value = Option<bool>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a boolean or string \"true\"/\"false\"")
        }

        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            match v {
                "true" => Ok(Some(true)),
                "false" => Ok(Some(false)),
                _ => Err(E::custom(format!("invalid bool string: {}", v))),
            }
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(BoolLenientVisitor)
}

/// Lenient deserializer for the `children` union field.
///
/// Accepts `bool` OR a string. A bool passes through; a string is resolved via
/// the SHARED `crate::frontmatter_union::normalize_children` so the build
/// pipeline and the editor agree byte-for-byte on what a value means. The folder
/// reference carried by a string is recovered separately by calling
/// `normalize_children` on the raw value in the pipeline (this deserializer only
/// produces the resolved `Option<bool>`; it cannot write the sibling
/// `children_source`). This replaces the old pre-parse YAML rewrite.
pub fn deserialize_children_lenient<'de, D>(
    deserializer: D,
) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct ChildrenLenientVisitor;

    impl<'de> de::Visitor<'de> for ChildrenLenientVisitor {
        type Value = Option<bool>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean or a wikilink/path string")
        }

        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            let norm = crate::frontmatter_union::normalize_children(
                &serde_yaml::Value::String(v.to_string()),
            );
            Ok(Some(norm.children))
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    deserializer.deserialize_any(ChildrenLenientVisitor)
}

/// Lenient deserializer for string fields YAML may have implicitly typed as a
/// non-string scalar. `uid: 46160604` -> integer, `uid: 753659e7` -> float,
/// `title: 2024` -> integer. serde struct deserialize is atomic, so without this
/// one such field fails the WHOLE `FrontMatter` (and the pipeline blanks every
/// field). Stringify int/float/bool scalars so a numeric value can't poison its
/// neighbors. Integer round-trips exactly; a float token is lossy (YAML already
/// collapsed it to f64) — accepted because losing the whole block is worse. See
/// ADR-020.
pub fn deserialize_string_lenient<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    struct StringLenientVisitor;
    impl<'de> de::Visitor<'de> for StringLenientVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, or a number/bool YAML coerced from one")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> { Ok(Some(v.to_string())) }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> { Ok(Some(v)) }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> { Ok(Some(v.to_string())) }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> { Ok(Some(v.to_string())) }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> { Ok(Some(v.to_string())) }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> { Ok(Some(v.to_string())) }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> { Ok(None) }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> { Ok(None) }
        fn visit_some<D2>(self, d: D2) -> Result<Self::Value, D2::Error>
        where D2: de::Deserializer<'de> { d.deserialize_any(StringLenientVisitor) }
    }
    deserializer.deserialize_any(StringLenientVisitor)
}

/// Extract a meaningful name from a frontmatter reference that may be either
/// a wikilink (`[[Departure]]`), a resolved path (`travel/departure.md`),
/// or a plain name (`Departure`).
///
/// **Note:** Wikilink resolution is centralized in `crates/moss-core/src/resolve/`.
/// Frontmatter values like sidebar, cover, and series are already resolved paths
/// by the time they reach this module. Do not add wikilink handling here.
///
/// After `resolve_frontmatter_wikilinks`, series/sidebar entries are resolved
/// to file paths.  This helper extracts a name for matching against
/// `ParsedDocument::clean_stem` or folder slugs.
///
/// For folder notes (`index.md`), returns the parent folder name since
/// the meaningful identifier is the folder, not "index".
///
/// Examples:
///   - `"[[Departure]]"` → `"Departure"` (wikilink fallback)
///   - `"travel/departure.md"` → `"departure"` (path → filename stem)
///   - `"blog/index.md"` → `"blog"` (folder note → folder name)
///   - `"news.md"` → `"news"` (root file → stem)
///   - `"Departure"` → `"Departure"` (plain name, pass-through)
#[allow(clippy::string_slice)] // char-aligned: ASCII quote chars (single-byte) + len >= 2 guard prevents [1..0] panic
pub fn frontmatter_ref_to_stem(s: &str) -> String {
    let trimmed = s.trim();

    // Strip optional surrounding quotes (simplified frontmatter preserves them)
    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Safety fallback: handle wikilink brackets for direct callers that
    // bypass the resolve phase (e.g. direct process_markdown_file calls).
    #[allow(clippy::string_slice)] // char-aligned: starts_with/ends_with already verified ASCII bracket chars (single-byte)
    let cleaned = if unquoted.starts_with("[[") && unquoted.ends_with("]]") {
        let inner = &unquoted[2..unquoted.len() - 2];
        // Strip leading / (Obsidian root-relative paths)
        inner.trim_start_matches('/')
    } else {
        unquoted
    };

    // If the result looks like a file path (has / or .md extension),
    // extract the meaningful name.
    if cleaned.contains('/') || cleaned.ends_with(".md") {
        #[allow(clippy::unwrap_used)] // rsplit always has at least one element
        let filename = cleaned.rsplit('/').next().unwrap_or(cleaned);
        #[allow(clippy::string_slice)] // byte index from rfind is char-aligned (ASCII dot)
        let stem = match filename.rfind('.') {
            Some(pos) if pos > 0 => &filename[..pos],
            _ => filename,
        };
        // For folder notes (index.md), use the parent folder name
        if stem == "index" {
            let parent = cleaned.rsplit('/').nth(1);
            match parent {
                Some(folder) => folder.to_string(),
                None => stem.to_string(),
            }
        } else {
            stem.to_string()
        }
    } else {
        cleaned.to_string()
    }
}

/// Translate the deprecated `sidebar:` field into the unified `children` family.
///
/// Fires when `sidebar:` is set AND there is no positive `children:` intent
/// (children unset, or `false`). Sets `children_source`, `children = true`,
/// `children_in = "sidebar"`, and the `_from_sidebar_alias` provenance flag
/// so the sidebar callsite can apply the legacy default-3 limit on cross-ref.
///
/// If `children:` is `true` or a wikilink, the alias yields and `sidebar:` is
/// ignored — explicit children intent wins. Returns deprecation/conflict
/// warnings for the build log.
pub fn apply_sidebar_alias(fm: &mut FrontMatter) -> Vec<String> {
    let Some(sidebar_ref) = fm.sidebar.clone() else {
        return Vec::new();
    };

    let mut warnings = Vec::new();
    let has_positive_children_intent =
        fm.children_source.is_some() || matches!(fm.children, Some(true));

    if has_positive_children_intent {
        warnings.push(
            "`sidebar:` ignored because `children:` is set; remove `sidebar:` and use `children_in: sidebar`"
                .to_string(),
        );
    } else {
        fm.children = Some(true);
        fm.children_source = Some(sidebar_ref.clone());
        fm.children_in = Some("sidebar".to_string());
        fm._from_sidebar_alias = Some(true);
        warnings.push(format!(
            "`sidebar:` is deprecated; use `children: {} + children_in: sidebar`",
            sidebar_ref
        ));
    }

    warnings
}

/// Whether content opens with simplified frontmatter (no leading `---`) — the
/// one splitter's answer ([`crate::frontmatter::frontmatter_span`]), never a
/// second scan.
pub fn is_simplified_frontmatter(content: &str) -> bool {
    simplified_fields(content).is_some()
}

/// The simplified field text and the body, or `None` in any other dialect. Every
/// simplified reader in this module reads it, so none can disagree about where
/// the frontmatter ends.
fn simplified_fields(content: &str) -> Option<(&str, &str)> {
    let span = crate::frontmatter::frontmatter_span(content)?;
    if span.kind != crate::frontmatter::FrontmatterKind::Simplified {
        return None;
    }
    // Char-aligned: `fields` and `body` are line-boundary offsets from
    // `frontmatter_span`, which splits on the ASCII `\n`.
    #[allow(clippy::string_slice)]
    Some((&content[span.fields], &content[span.body..]))
}

/// One line of simplified frontmatter: blank, a known bare flag, or `key: value`.
/// Prose, headings, list items, HTML and fence markers all fail it — which is
/// the point: they mark where the body starts.
///
/// The halves are deliberately asymmetric. An unknown **key** is tolerated;
/// the parser ignores keys it doesn't know and sites carry custom ones. An
/// unknown **bare word** is not: `Introduction\n---` is CommonMark for an
/// `<h2>`, ordinary in an Obsidian vault, and accepting it would let uid
/// stamping rewrite that heading on disk. Strictness costs nothing here — a
/// bare word outside [`SIMPLIFIED_BARE_FLAGS`] never meant anything anyway.
///
/// The colon must be followed by a space or end the line — YAML's own rule for
/// what makes a line a mapping rather than a plain string. Without it a bare
/// link, `https://example.com`, reads as the field `https` and the setext `---`
/// underneath it reads as the delimiter, so uid stamping splices `uid:` between
/// an author's link and its own heading underline. The same rule covers
/// `mailto:`, `tel:` and every other scheme for free.
pub(crate) fn is_frontmatter_field_line(trimmed: &str) -> bool {
    match trimmed.split_once(':') {
        None => trimmed.is_empty() || SIMPLIFIED_BARE_FLAGS.contains(&trimmed),
        // Only the key is judged; the value is unconstrained. Lowercase initial
        // because every `BUILTIN_FIELDS` entry has one, and it is what tells
        // `url: x` apart from a sentence like `Note: x`.
        Some((key, value)) => {
            let key = key.trim_end();
            (value.is_empty() || value.starts_with([' ', '\t']))
                && key.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }
    }
}

/// Keys that may be written bare, as `nav` rather than `nav: true`. Must stay in
/// step with the bare-flag arm of [`parse_simplified_frontmatter`];
/// `every_bare_flag_is_recognised_by_the_parser` fails if it drifts.
pub(crate) const SIMPLIFIED_BARE_FLAGS: [&str; 8] = [
    "nav", "home", "draft", "listed", "breadcrumb", "footer", "comments", "children",
];

/// The field names a simplified frontmatter block declares, in source order.
///
/// [`parse_simplified_frontmatter`] drops every name it doesn't recognize —
/// exactly what a caller wanting to say something *about* an unrecognized name
/// needs back. Same delimiter call and same `key: value` / bare-flag shapes as
/// that parser, so the two cannot disagree about what counts as a field. Path-
/// free by moss-core's no-I/O rule; the caller owns the path and the decision
/// to warn (`foreign_frontmatter_warnings` in `build/markdown/pipeline.rs`).
pub fn simplified_frontmatter_keys(content: &str) -> Vec<String> {
    simplified_field_pairs(content).into_iter().map(|(key, _)| key).collect()
}

/// Every simplified field as `(key, value)`, in source order; `None` marks a
/// bare flag. The one owner of "which lines are fields and what are their keys",
/// shared by [`simplified_frontmatter_keys`] and
/// [`crate::frontmatter::frontmatter_map`]. What a VALUE means is deliberately
/// NOT settled here — this hands back raw text, and the typed parser below
/// applies the per-field coercions (comma lists, wikilink refs) it has no schema for.
pub(crate) fn simplified_field_pairs(content: &str) -> Vec<(String, Option<&str>)> {
    let Some((fields, _body)) = simplified_fields(content) else {
        return Vec::new();
    };
    fields
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            match trimmed.split_once(':') {
                Some((key, value)) => Some((key.trim().to_string(), Some(value.trim()))),
                None => Some((trimmed.to_string(), None)),
            }
        })
        .collect()
}

/// Parse simplified frontmatter format into FrontMatter struct.
/// Format:
/// - Boolean flags: just the word (e.g., `nav` → nav: true)
/// - Key-value: `key: value`
/// - Comma lists: `key: a, b, c`
pub fn parse_simplified_frontmatter(content: &str) -> (FrontMatter, String) {
    let mut frontmatter = FrontMatter::default();
    // WHERE the frontmatter ends is `frontmatter_span`'s call — the same one the
    // caller's `is_simplified_frontmatter` gate made. Re-deriving it here asked a
    // different question, and the two drifted.
    let Some((fields, body)) = simplified_fields(content) else {
        return (frontmatter, content.to_string());
    };

    for line in fields.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse the line
        if let Some(colon_pos) = trimmed.find(':') {
            // Key-value pair
            #[allow(clippy::string_slice)] // byte index from find(':') is char-aligned (ASCII colon)
            let key = trimmed[..colon_pos].trim();
            #[allow(clippy::string_slice)] // byte index from find(':') + 1 is char-aligned (ASCII colon is single-byte)
            let value = trimmed[colon_pos + 1..].trim();

            match key {
                "title" => frontmatter.title = Some(value.to_string()),
                "date" => frontmatter.date = Some(value.to_string()),
                "weight" => frontmatter.weight = value.parse().ok(),
                "url" => frontmatter.url = Some(value.to_string()),
                "cover" => frontmatter.cover = Some(value.to_string()),
                // D1: children is boolean — "true" → Some(true), "false" → Some(false)
                // D5: `list` alias removed (breaking change) — see docs/archive/2026-03-05-sidebar-redesign.md
                "children" => {
                    match value {
                        "true" | "" => frontmatter.children = Some(true),
                        "false" => frontmatter.children = Some(false),
                        v if v.starts_with("[[") && v.ends_with("]]") => {
                            // Wikilink reference: render children from the target folder
                            frontmatter.children = Some(true);
                            frontmatter.children_source = Some(v.to_string());
                        }
                        _ => {
                            eprintln!("Warning: children: \"{}\" is not valid. Use true, false, or \"[[Folder]]\".", value);
                        }
                    }
                }
                "sidebar" => frontmatter.sidebar = Some(value.to_string()),
                "children_style" => frontmatter.children_style = Some(value.to_string()),
                "children_group" => frontmatter.children_group = Some(value.to_string()),
                "children_depth" => frontmatter.children_depth = Some(value.to_string()),
                "children_in" => match value {
                    "body" | "sidebar" => frontmatter.children_in = Some(value.to_string()),
                    _ => eprintln!(
                        "Warning: children_in: \"{}\" is not valid. Use \"body\" or \"sidebar\".",
                        value
                    ),
                },
                "children_limit" => frontmatter.children_limit = value.parse().ok(),
                "description" => frontmatter.description = Some(value.to_string()),
                "lang" => frontmatter.lang = Some(value.to_string()),
                "translationKey" | "translation_key" => {
                    frontmatter.translation_key = Some(value.to_string())
                }
                "also" | "also_in" => {
                    let items: Vec<String> = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !items.is_empty() {
                        frontmatter.also_in = Some(items);
                    }
                }
                "tags" => {
                    let items: Vec<String> = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !items.is_empty() {
                        frontmatter.tags = Some(items);
                    }
                }
                // Handle boolean with explicit value. Strip quotes first: authors
                // copying YAML habits into simplified frontmatter (`nav: 'true'`)
                // would otherwise compare "'true'" != "true" and silently land on
                // `false` — a worse variant of #925 (miscoercion, not just drop).
                "nav" | "home" | "draft" | "listed" | "breadcrumb" | "footer" | "comments" => {
                    let unquoted = value.trim_matches(|c| c == '\'' || c == '"');
                    // Anything other than "true"/"false" is a likely typo (e.g.
                    // "yes", "Ture"); warn rather than silently defaulting to
                    // false — the same #925 failure mode in this hand-rolled
                    // parser, mirroring the "children"/"children_in" warnings above.
                    let flag = match unquoted {
                        "true" | "" => Some(true),
                        "false" => Some(false),
                        _ => {
                            eprintln!(
                                "Warning: {}: \"{}\" is not valid. Use true or false.",
                                key, value
                            );
                            None
                        }
                    };
                    match key {
                        "nav" => frontmatter.nav = flag,
                        "home" => frontmatter.home = flag,
                        "draft" => frontmatter.draft = flag,
                        "listed" => frontmatter.listed = flag,
                        "breadcrumb" => frontmatter.breadcrumb = flag,
                        "footer" => frontmatter.footer = flag,
                        "comments" => frontmatter.comments = flag,
                        _ => unreachable!(),
                    }
                }
                "slot" => {
                    if !value.is_empty() {
                        // Validation against known slot names happens at
                        // consumer time in `build::footer::collect_footer_slots_by_language`,
                        // so authors get a deferred warning rather than a
                        // hard parse error.
                        frontmatter.slot = Some(value.to_string());
                    }
                }
                // Unknown key, ignore. Deliberately silent HERE: this parser runs
                // ~5x per file (uid stamping, scanning, editor chips) and never
                // sees the path, so a warning from it repeats and names no file.
                // pipeline.rs raises it once, with the path, over
                // `simplified_frontmatter_keys`.
                _ => {}
            }
        } else {
            // Boolean flag (just the word)
            match trimmed {
                "nav" => frontmatter.nav = Some(true),
                "home" => frontmatter.home = Some(true),
                "draft" => frontmatter.draft = Some(true),
                "listed" => frontmatter.listed = Some(true),
                "breadcrumb" => frontmatter.breadcrumb = Some(true),
                "footer" => frontmatter.footer = Some(true),
                "comments" => frontmatter.comments = Some(true),
                "children" => frontmatter.children = Some(true),
                _ => {} // Unknown flag, ignore
            }
        }
    }

    (frontmatter, body.to_string())
}

/// Compute the output URL path for a source file.
/// This is the single source of truth for file-tree → page-tree mapping.
///
/// All directory segments and the file basename go through [`crate::slug::generate_slug`],
/// so URLs are always lowercase kebab-case regardless of how the source files
/// and folders are cased on disk.
///
/// # Arguments
/// * `file_path` - Relative path from site root (e.g., "posts/hello.md")
/// * `is_index_file` - Whether this file is the index/home file for its folder
/// * `frontmatter_url` - Optional `url` override from frontmatter
/// * `clean_stem` - Language-suffix-stripped filename stem (e.g., "hello" from "hello.zh.md")
pub fn compute_url_path(
    file_path: &str,
    is_index_file: bool,
    frontmatter_url: Option<&str>,
    clean_stem: &str,
) -> String {
    use std::path::Path;
    use crate::slug::{generate_slug, slugify_path_segments};

    if is_index_file {
        let parent_path = Path::new(file_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        if parent_path.is_empty() {
            "index.html".to_string()
        } else if let Some(custom_url) = frontmatter_url {
            // url override replaces the last segment of the parent path
            let grandparent = Path::new(parent_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            let slug = generate_slug(custom_url);
            if grandparent.is_empty() {
                format!("{}/index.html", slug)
            } else {
                format!("{}/{}/index.html", slugify_path_segments(grandparent), slug)
            }
        } else {
            format!("{}/index.html", slugify_path_segments(parent_path))
        }
    } else {
        // Check for frontmatter url override first
        let slug = if let Some(custom_url) = frontmatter_url {
            generate_slug(custom_url)
        } else {
            // Use clean stem (language suffix stripped)
            generate_slug(clean_stem)
        };

        // Preserve directory structure, output as slug/index.html (pretty URL)
        let parent_path = Path::new(file_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");

        if parent_path.is_empty() {
            format!("{}/index.html", slug)
        } else {
            format!("{}/{}/index.html", slugify_path_segments(parent_path), slug)
        }
    }
}

#[cfg(test)]
mod project_typed_tests {
    use super::*;

    fn map_of(pairs: &[(&str, serde_yaml::Value)]) -> serde_yaml::Mapping {
        let mut m = serde_yaml::Mapping::new();
        for (k, v) in pairs {
            m.insert(serde_yaml::Value::String((*k).to_string()), v.clone());
        }
        m
    }

    #[test]
    fn project_typed_clean_map_no_warnings() {
        use serde_yaml::Value;
        let m = map_of(&[
            ("title", Value::String("Hello".into())),
            ("date", Value::String("2025-05-28".into())),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("Hello"));
        assert_eq!(fm.date.as_deref(), Some("2025-05-28"));
        assert!(warnings.is_empty(), "clean frontmatter yields no warnings");
    }

    #[test]
    fn project_typed_numeric_uid_coerces() {
        use serde_yaml::Value;
        // The canonical parse produced a YAML integer for uid (the real-world bug).
        let m = map_of(&[
            ("title", Value::String("Paper".into())),
            ("uid", Value::Number(serde_yaml::Number::from(46160604u64))),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.uid.as_deref(), Some("46160604"));
        assert_eq!(fm.title.as_deref(), Some("Paper"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn project_typed_ignores_unknown_fields() {
        use serde_yaml::Value;
        let m = map_of(&[
            ("title", Value::String("T".into())),
            ("syndicated", Value::String("https://example.com".into())),
            ("some_plugin_field", Value::Number(serde_yaml::Number::from(7u64))),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert!(warnings.is_empty(), "unknown fields are ignored, not errors");
    }

    #[test]
    fn project_typed_drops_only_the_unrepresentable_field() {
        use serde_yaml::Value;
        // `weight: high` cannot become i32. Field-granular: weight is dropped, but
        // every good field (title) survives. One bad field can't poison neighbors.
        let m = map_of(&[
            ("title", Value::String("Kept".into())),
            ("weight", Value::String("high".into())),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("Kept"), "good field survives");
        assert_eq!(fm.weight, None, "bad field defaulted");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "weight");
        assert!(!warnings[0].message.is_empty());
    }

    #[test]
    fn project_typed_drops_malformed_skip_schema_field_keeps_neighbors() {
        use serde_yaml::Value;
        // `analytics` is skip_schema (invisible to validate_frontmatter) and has
        // a custom Deserialize requiring `url`. A malformed analytics must NOT
        // blank the whole block — the gap a schema-driven drop loop missed.
        // Isolation testing catches it because from_value fails on it alone.
        let mut analytics = serde_yaml::Mapping::new();
        analytics.insert(Value::String("provider".into()), Value::String("goatcounter".into()));
        let m = map_of(&[
            ("title", Value::String("Important Title".into())),
            ("date", Value::String("2025-05-28".into())),
            ("analytics", Value::Mapping(analytics)),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("Important Title"), "title survives malformed analytics");
        assert_eq!(fm.date.as_deref(), Some("2025-05-28"), "date survives too");
        assert!(warnings.iter().any(|w| w.key == "analytics"), "analytics is flagged dropped");
    }

    #[test]
    fn project_typed_keeps_coercible_uid_drops_bad_weight() {
        use serde_yaml::Value;
        let m = map_of(&[
            ("title", Value::String("Paper".into())),
            ("uid", Value::Number(46160604u64.into())),
            ("weight", Value::String("high".into())),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("Paper"));
        assert_eq!(fm.uid.as_deref(), Some("46160604"), "numeric uid is coerced, NOT dropped");
        assert_eq!(fm.weight, None);
        assert_eq!(warnings.len(), 1, "only weight warns; uid is coerced silently");
        assert_eq!(warnings[0].key, "weight");
    }

    #[test]
    fn project_typed_drops_multiple_bad_fields_keeps_good() {
        use serde_yaml::Value;
        let m = map_of(&[
            ("title", Value::String("T".into())),
            ("weight", Value::String("high".into())),
            ("tags", Value::String("not-an-array".into())),
        ]);
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("T"), "good field survives multiple bad neighbors");
        let keys: std::collections::HashSet<_> = warnings.iter().map(|w| w.key.as_str()).collect();
        assert!(keys.contains("weight"));
        assert!(keys.contains("tags"));
    }

    // --- byline / colophon: the shapes an author actually writes, parsed from
    //     real YAML.
    //     Row-splitting rules live in frontmatter_union's decision table; what
    //     these pin is that the hand-rolled deserializer is wired to it and
    //     that a bad value drops only itself. ---

    fn byline_of(yaml: &str) -> (Option<Vec<String>>, Vec<FieldWarning>) {
        let m: serde_yaml::Mapping = serde_yaml::from_str(yaml).expect("test yaml parses");
        let (fm, warnings) = project_typed(&m);
        (fm.byline, warnings)
    }

    #[test]
    fn byline_accepts_a_block_scalar_with_a_markdown_link() {
        let (byline, warnings) = byline_of(
            "byline: |\n  作者　糜緒洋\n  編輯　謝丁\n  首發媒體　[端傳媒](https://theinitium.com/a)\n",
        );
        assert_eq!(
            byline.unwrap(),
            vec![
                "作者　糜緒洋",
                "編輯　謝丁",
                "首發媒體　[端傳媒](https://theinitium.com/a)"
            ]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn byline_accepts_a_list() {
        let (byline, warnings) = byline_of("byline:\n  - 作者　糜緒洋\n  - 編輯　謝丁\n");
        assert_eq!(byline.unwrap(), vec!["作者　糜緒洋", "編輯　謝丁"]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn byline_absent_or_empty_is_none() {
        assert_eq!(byline_of("title: T\n").0, None, "absent");
        assert_eq!(byline_of("byline: \"\"\n").0, None, "empty string");
        assert_eq!(byline_of("byline: []\n").0, None, "empty list");
    }

    #[test]
    fn colophon_parses_like_byline_and_is_independent_of_it() {
        let m: serde_yaml::Mapping = serde_yaml::from_str(
            "byline: 作者　糜緒洋\ncolophon: |\n  首發媒體　[端傳媒](https://theinitium.com/a)\n  封面　拍攝：糜緒洋\n",
        )
        .expect("yaml parses");
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.byline.unwrap(), vec!["作者　糜緒洋"]);
        assert_eq!(
            fm.colophon.unwrap(),
            vec!["首發媒體　[端傳媒](https://theinitium.com/a)", "封面　拍攝：糜緒洋"]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn byline_of_the_wrong_shape_drops_only_itself() {
        let m: serde_yaml::Mapping =
            serde_yaml::from_str("title: T\nbyline: 42\n").expect("yaml parses");
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.title.as_deref(), Some("T"), "neighbour survives");
        assert_eq!(fm.byline, None);
        let w = warnings.iter().find(|w| w.key == "byline").expect("byline flagged");
        assert!(
            w.message.contains("a string or a list of strings"),
            "the author is told what byline accepts, not 'untagged enum': {}",
            w.message
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // serde_yaml coerces numbers→String on its own, so these document behavior; the real
    // build-path guards are the *_via_json_path tests below.
    #[test]
    fn numeric_title_coerces_to_string() {
        let fm: FrontMatter = serde_yaml::from_str("title: 2024\ndate: 2024-01-01\n").expect("parse");
        assert_eq!(fm.title.as_deref(), Some("2024"));
        assert_eq!(fm.date.as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn numeric_uid_coerces_and_preserves_siblings() {
        let yaml = "title: Kept Title\nuid: 46160604\ndate: 2025-05-28\n";
        let fm: FrontMatter = serde_yaml::from_str(yaml).expect("must not fail to parse");
        assert_eq!(fm.uid.as_deref(), Some("46160604"), "integer uid round-trips exactly");
        assert_eq!(fm.title.as_deref(), Some("Kept Title"), "sibling title must survive");
        assert_eq!(fm.date.as_deref(), Some("2025-05-28"), "sibling date must survive");
    }
    #[test]
    fn float_like_uid_does_not_blank_struct() {
        let yaml = "title: T\nuid: 753659e7\n";
        let fm: FrontMatter = serde_yaml::from_str(yaml).expect("must not fail to parse");
        assert!(fm.uid.is_some(), "float-like uid yields some string");
        assert_eq!(fm.title.as_deref(), Some("T"));
    }
    #[test]
    fn string_uid_unchanged() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\nuid: 54ddc5c0\n").expect("parse");
        assert_eq!(fm.uid.as_deref(), Some("54ddc5c0"));
    }
    #[test]
    fn missing_uid_is_none() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\n").expect("parse");
        assert_eq!(fm.uid, None);
        assert_eq!(fm.title.as_deref(), Some("T"));
    }
    #[test]
    fn null_uid_is_none() {
        let fm: FrontMatter = serde_yaml::from_str("title: T\nuid:\n").expect("parse");
        assert_eq!(fm.uid, None);
    }

    // These mirror the BUILD path: gray_matter's Pod::deserialize lowers to
    // serde_json::Value (Pod::Integer => json!(val)) then serde_json::from_value,
    // which — unlike serde_yaml — does NOT coerce numbers to String. Without
    // deserialize_string_lenient these FAIL ("invalid type: integer, expected a
    // string") and the whole FrontMatter would blank. See ADR-020.
    #[test]
    fn numeric_uid_via_json_path_coerces_and_preserves_siblings() {
        let v = serde_json::json!({ "title": "Kept Title", "uid": 46160604u64, "date": "2025-05-28" });
        let fm: FrontMatter = serde_json::from_value(v).expect("build path must not fail on numeric uid");
        assert_eq!(fm.uid.as_deref(), Some("46160604"));
        assert_eq!(fm.title.as_deref(), Some("Kept Title"));
        assert_eq!(fm.date.as_deref(), Some("2025-05-28"));
    }
    #[test]
    fn numeric_title_via_json_path_coerces() {
        let v = serde_json::json!({ "title": 2024u64, "date": "2024-01-01" });
        let fm: FrontMatter = serde_json::from_value(v).expect("build path must not fail on numeric title");
        assert_eq!(fm.title.as_deref(), Some("2024"));
        assert_eq!(fm.date.as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn frontmatter_ref_to_stem_single_char_quote_no_panic() {
        // Regression: single-char `"` or `'` must not panic at [1..0]
        assert_eq!(frontmatter_ref_to_stem("\""), "\"");
        assert_eq!(frontmatter_ref_to_stem("'"), "'");
    }

    #[test]
    fn parses_home_marker() {
        let fm: FrontMatter = serde_yaml::from_str("home: true\n").expect("parse");
        assert_eq!(fm.home, Some(true));
    }

    #[test]
    fn parses_home_marker_simplified() {
        // Simplified frontmatter (no leading `---`, terminated by a `---` line).
        // Both the `home: true` key:value form and the bare `home` flag set it.
        let (fm_kv, _) = parse_simplified_frontmatter("home: true\n---\nbody\n");
        assert_eq!(fm_kv.home, Some(true));
        let (fm_flag, _) = parse_simplified_frontmatter("home\n---\nbody\n");
        assert_eq!(fm_flag.home, Some(true));
        let (fm_none, _) = parse_simplified_frontmatter("nav\n---\nbody\n");
        assert_eq!(fm_none.home, None);
    }

    #[test]
    fn series_field_flag_roundtrips() {
        let v: SeriesField = serde_yaml::from_str("true").unwrap();
        assert!(matches!(v, SeriesField::Flag(true)));
        let v: SeriesField = serde_yaml::from_str("false").unwrap();
        assert!(matches!(v, SeriesField::Flag(false)));
    }

    #[test]
    fn series_field_ordered_roundtrips() {
        let v: SeriesField = serde_yaml::from_str(r#"["[[Ch 1]]", "[[Ch 2]]"]"#).unwrap();
        assert!(matches!(v, SeriesField::Ordered(ref items) if items.len() == 2));
    }

    #[test]
    fn parses_listed_field_typed() {
        let off: FrontMatter = serde_yaml::from_str("listed: false\n").expect("parse");
        assert_eq!(off.listed, Some(false));
        let on: FrontMatter = serde_yaml::from_str("listed: true\n").expect("parse");
        assert_eq!(on.listed, Some(true));
        let absent: FrontMatter = serde_yaml::from_str("title: X\n").expect("parse");
        assert_eq!(absent.listed, None);
    }

    #[test]
    fn quoted_nav_string_coerces_instead_of_vanishing() {
        // Regression for #925: `nav: 'true'` (quoted YAML string) must not
        // silently drop the field. Every Option<bool> field shares the same
        // lenient deserializer, so this is a single behavior, not per-field.
        let fm: FrontMatter = serde_yaml::from_str("nav: 'true'\n").expect("parse");
        assert_eq!(fm.nav, Some(true), "quoted 'true' coerces to Some(true), not None");

        let fm: FrontMatter = serde_yaml::from_str("nav: 'false'\n").expect("parse");
        assert_eq!(fm.nav, Some(false));

        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("nav".into()),
            serde_yaml::Value::String("true".into()),
        );
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.nav, Some(true));
        assert!(warnings.is_empty(), "coerced bool must not warn as dropped");
    }

    #[test]
    fn quoted_bool_strings_coerce_for_every_option_bool_field() {
        // home/draft/listed/comments had the same gap as nav; close it uniformly.
        let yaml = "home: 'true'\ndraft: 'false'\nlisted: 'true'\ncomments: 'false'\n";
        let fm: FrontMatter = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(fm.home, Some(true));
        assert_eq!(fm.draft, Some(false));
        assert_eq!(fm.listed, Some(true));
        assert_eq!(fm.comments, Some(false));
    }

    #[test]
    fn invalid_bool_string_still_drops_with_warning() {
        // A genuinely invalid bool string (not "true"/"false") must still warn
        // and drop, not be silently accepted as truthy.
        let mut m = serde_yaml::Mapping::new();
        m.insert(
            serde_yaml::Value::String("nav".into()),
            serde_yaml::Value::String("yes".into()),
        );
        let (fm, warnings) = project_typed(&m);
        assert_eq!(fm.nav, None, "invalid bool string is dropped, not coerced");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "nav");
    }

    #[test]
    fn simplified_frontmatter_strips_quotes_from_bool_value() {
        // Regression: authors copying YAML habits into simplified frontmatter
        // (`nav: 'true'`) must not have the quotes poison the `== "true"`
        // comparison and silently land on `false`.
        let (fm, _) = parse_simplified_frontmatter("nav: 'true'\nhome: \"false\"\n---\nbody\n");
        assert_eq!(fm.nav, Some(true));
        assert_eq!(fm.home, Some(false));
    }

    #[test]
    fn simplified_frontmatter_invalid_bool_value_is_unset_not_false() {
        // A typo'd bool value (not "true"/"false") must not silently become
        // `Some(false)` — that's the same #925 failure mode (a page author
        // writes `nav: yes` expecting it to show, and it silently doesn't)
        // in this parser's own hand-rolled bool matching.
        let (fm, _) = parse_simplified_frontmatter("nav: yes\n---\nbody\n");
        assert_eq!(fm.nav, None);
    }

    #[test]
    fn parses_listed_field_simplified() {
        let (off, _) = parse_simplified_frontmatter("listed: false\n---\nbody\n");
        assert_eq!(off.listed, Some(false));
        let (on, _) = parse_simplified_frontmatter("listed: true\n---\nbody\n");
        assert_eq!(on.listed, Some(true));
        let (flag, _) = parse_simplified_frontmatter("listed\n---\nbody\n");
        assert_eq!(flag.listed, Some(true));
        let (none, _) = parse_simplified_frontmatter("nav\n---\nbody\n");
        assert_eq!(none.listed, None);
    }
}

#[cfg(test)]
mod url_path_tests {
    use super::*;

    // Tests for slugify_path_segments and compute_url_path
    // (folder-name URL slugify regression coverage)

    #[test]
    fn test_slugify_path_segments_lowercase_passthrough() {
        assert_eq!(crate::slug::slugify_path_segments("posts"), "posts");
        assert_eq!(crate::slug::slugify_path_segments("posts/2024"), "posts/2024");
    }

    #[test]
    fn test_slugify_path_segments_title_case_lowercased() {
        assert_eq!(crate::slug::slugify_path_segments("News"), "news");
        assert_eq!(crate::slug::slugify_path_segments("Projects"), "projects");
    }

    #[test]
    fn test_slugify_path_segments_spaces_kebabed() {
        assert_eq!(crate::slug::slugify_path_segments("My Section"), "my-section");
        assert_eq!(
            crate::slug::slugify_path_segments("News/Sub Section"),
            "news/sub-section"
        );
    }

    #[test]
    fn test_slugify_path_segments_empty() {
        assert_eq!(crate::slug::slugify_path_segments(""), "");
    }

    #[test]
    fn test_slugify_path_segments_unicode_preserved() {
        assert_eq!(crate::slug::slugify_path_segments("文章"), "文章");
        assert_eq!(crate::slug::slugify_path_segments("文章/Hello World"), "文章/hello-world");
    }

    #[test]
    fn test_compute_url_path_top_level_file() {
        // Top-level file: filename slugified, no parent.
        assert_eq!(
            compute_url_path("Funding.md", false, None, "Funding"),
            "funding/index.html"
        );
        assert_eq!(
            compute_url_path("Code of Conduct.md", false, None, "Code of Conduct"),
            "code-of-conduct/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_file_in_lowercase_folder() {
        // Regression: pre-existing lowercase folder still produces lowercase URL.
        assert_eq!(
            compute_url_path("posts/hello.md", false, None, "hello"),
            "posts/hello/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_file_in_title_case_folder() {
        // Bug fix: Title-Case folder must be lowercased in the URL.
        assert_eq!(
            compute_url_path(
                "News/2026-04-22-morbidelli.md",
                false,
                None,
                "2026-04-22-morbidelli"
            ),
            "news/2026-04-22-morbidelli/index.html"
        );
        assert_eq!(
            compute_url_path("Projects/giant-planets.md", false, None, "giant-planets"),
            "projects/giant-planets/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_file_in_nested_title_case_folders() {
        assert_eq!(
            compute_url_path(
                "My Section/Sub Page/page.md",
                false,
                None,
                "page"
            ),
            "my-section/sub-page/page/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_index_file_in_title_case_folder() {
        // Bug fix: folder-file (e.g., News/News.md) URL must be /news/.
        assert_eq!(
            compute_url_path("News/News.md", true, None, "News"),
            "news/index.html"
        );
        assert_eq!(
            compute_url_path("Projects/Projects.md", true, None, "Projects"),
            "projects/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_index_file_in_lowercase_folder() {
        // Regression: lowercase folder index unchanged.
        assert_eq!(
            compute_url_path("posts/index.md", true, None, "index"),
            "posts/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_root_index_file() {
        // Root index has no parent path; stays "index.html".
        assert_eq!(
            compute_url_path("index.md", true, None, "index"),
            "index.html"
        );
    }

    #[test]
    fn test_compute_url_path_url_override_with_title_case_grandparent() {
        // url: override slugifies to its own segment; the grandparent path
        // is also slugified so a Title-Case grandparent produces lowercase URL.
        assert_eq!(
            compute_url_path(
                "Section/Old Name/page.md",
                true,
                Some("New Name"),
                "page"
            ),
            "section/new-name/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_url_override_no_grandparent() {
        // url: override at top level (no grandparent) — slug is the only segment.
        assert_eq!(
            compute_url_path("News/index.md", true, Some("Custom Name"), "index"),
            "custom-name/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_non_index_with_url_override_in_title_case_folder() {
        // Non-index file with its own `url:` override, sitting in a Title-Case
        // folder. The override slug becomes the file segment; the parent folder
        // segment is independently slugified.
        assert_eq!(
            compute_url_path(
                "News/page.md",
                false,
                Some("Custom Slug"),
                "page"
            ),
            "news/custom-slug/index.html"
        );
    }

    #[test]
    fn test_compute_url_path_idempotent_for_already_lowercase() {
        // The fix must not break paths that were already correct.
        assert_eq!(
            compute_url_path("blog/post.md", false, None, "post"),
            "blog/post/index.html"
        );
        assert_eq!(
            compute_url_path("blog/index.md", true, None, "index"),
            "blog/index.html"
        );
    }

    #[test]
    fn simplified_frontmatter_ignores_dashes_inside_a_code_fence() {
        // Docs pages show YAML frontmatter examples inside fenced code blocks.
        // Those `---` lines belong to the example, not to the page.
        let content = "Intro.\n\n```yaml\n---\ntitle: Example\n---\n```\n\nOutro.";
        assert!(!is_simplified_frontmatter(content));

        let tilde = "Intro.\n\n~~~yaml\n---\ntitle: Example\n---\n~~~\n\nOutro.";
        assert!(!is_simplified_frontmatter(tilde));
    }

    #[test]
    fn simplified_frontmatter_ignores_dashes_inside_a_directive_block() {
        let content = "Intro.\n\n:::grid 3\n[A](/a/)\n\n---\n\n[B](/b/)\n:::\n";
        assert!(!is_simplified_frontmatter(content));
    }

    #[test]
    fn simplified_frontmatter_must_be_the_files_own_first_lines() {
        // Frontmatter is a prefix. Once a line of body has gone by — a
        // directive, a fence, anything — a later `nav\n---` is prose that
        // happens to sit above a thematic break, not a late frontmatter block.
        assert!(!is_simplified_frontmatter(":::note\nhi\n:::\nnav\n---\n\n# Body"));
        assert!(!is_simplified_frontmatter("```\ncode\n```\nnav\n---\n\n# Body"));
        // The prefix itself still works, blank lines and all.
        assert!(is_simplified_frontmatter("nav\ntitle: Hi\n\n---\n\n# Body"));
    }

    #[test]
    fn simplified_frontmatter_ignores_a_setext_heading() {
        // `Introduction\n---` is CommonMark for `<h2>Introduction</h2>`, and
        // Obsidian vaults are full of them. Reading it as a bare flag closed by
        // a delimiter would let uid stamping rewrite the heading on disk.
        assert_eq!(simplified_fields("Introduction\n---\n\nText.\n"), None);
        assert_eq!(simplified_fields("Note: this matters\n\n---\n"), None);
        // A known flag in the same position IS frontmatter.
        assert_eq!(simplified_fields("draft\n---\n\nText.\n"), Some(("draft\n", "\nText.\n")));
    }

    #[test]
    fn simplified_frontmatter_ignores_a_url_above_a_setext_underline() {
        // `https://example.com` split on its first colon reads as the field
        // `https`, so the `---` underneath it read as the delimiter and uid
        // stamping wrote `uid:` between an author's link and its own heading
        // underline. YAML says a mapping needs a space after the colon; a URL
        // scheme has none, and neither does `mailto:` or `tel:`.
        assert_eq!(simplified_fields("https://example.com/x\n---\n\nText.\n"), None);
        assert_eq!(simplified_fields("mailto:hi@example.com\n---\n"), None);
        // A key with nothing after the colon is still a field (`title:`).
        assert_eq!(simplified_fields("title:\n---\nBody\n"), Some(("title:\n", "Body\n")));
    }

    #[test]
    fn every_bare_flag_is_recognised_by_the_parser() {
        // The predicate gates on SIMPLIFIED_BARE_FLAGS; the parser has its own
        // match arm. If one grows a flag the other lacks, that flag either stops
        // being detected or stops being parsed — so pin them to each other.
        for flag in SIMPLIFIED_BARE_FLAGS {
            let content = format!("{flag}\n---\nbody\n");
            assert!(
                is_simplified_frontmatter(&content),
                "`{flag}` is listed as a bare flag but is not detected"
            );
            let (fm, _) = parse_simplified_frontmatter(&content);
            assert!(
                format!("{fm:?}").contains("Some(true)"),
                "`{flag}` is detected but the parser sets no field from it"
            );
        }
    }

    #[test]
    fn simplified_frontmatter_ignores_a_thematic_break_after_a_closed_fence() {
        // The shape that made this urgent: uid stamping writes its answer back
        // to disk, and nine of moss's own docs pages look exactly like this.
        let release_notes = "# Release notes\n\n```sh\nmoss build\n```\n\nOlder:\n\n---\n\n## 0.1\n";
        assert_eq!(simplified_fields(release_notes), None);
    }

    #[test]
    fn simplified_frontmatter_body_split_survives_crlf() {
        // `lines()` drops the `\r`, so summing its lengths lost a byte per line
        // and the body came back starting mid-delimiter.
        let (fm, body) = parse_simplified_frontmatter("nav\r\ntitle: Hi\r\n---\r\n\r\n# Body\r\n");
        assert_eq!(fm.title.as_deref(), Some("Hi"));
        assert_eq!(body, "\r\n# Body\r\n");
    }

    #[test]
    fn simplified_keys_include_the_names_the_typed_parser_drops() {
        // The whole point: `slug` never reaches FrontMatter, so a caller that
        // wants to tell the author it was ignored has to read it from here.
        let content = "title: Privacy\nslug: privacy\nnav\nmy_field: x\n---\n\nBody\n";
        let keys = simplified_frontmatter_keys(content);
        assert_eq!(keys, vec!["title", "slug", "nav", "my_field"]);
    }

    #[test]
    fn simplified_keys_stop_at_the_delimiter_and_survive_colons_in_values() {
        // A body line containing `x: y` must not be read as a field, and a
        // value that itself contains a colon must not split the key.
        let content = "title: Hello: World\n---\n\nNote: this is prose.\n";
        assert_eq!(simplified_frontmatter_keys(content), vec!["title"]);
    }

    #[test]
    fn simplified_keys_are_empty_without_simplified_frontmatter() {
        // Traditional YAML (leading `---`) is the other branch's business, and
        // a plain body has no frontmatter at all.
        assert_eq!(
            simplified_frontmatter_keys("---\ntitle: Hi\nslug: x\n---\n\nBody\n"),
            Vec::<String>::new()
        );
        assert_eq!(
            simplified_frontmatter_keys("Just prose.\n"),
            Vec::<String>::new()
        );
    }
}
