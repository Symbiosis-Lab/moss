//! Typed shortcode AST nodes.
//!
//! Each shortcode is a closed enum variant with fully-typed arguments.
//! Variants land per-shortcode in Phase B (one variant per migration
//! commit) of the typed-AST migration.
//!
//! Migration order (Phase B): Subscribe, Buttons, Gallery, Hero, Grid, Recent.

use serde::{Deserialize, Serialize};

use super::node::Block;
use super::url::Url;

/// A typed shortcode block.
///
/// Variants:
/// - [`Shortcode::Subscribe`] — inline subscribe form (description + button)
/// - [`Shortcode::Buttons`] — list of action buttons with markdown links
/// - [`Shortcode::Gallery`] — image gallery with optional column count
/// - [`Shortcode::Hero`] — full-width hero section with media + overlay
/// - [`Shortcode::Grid`] — flexible multi-cell layout
/// - [`Shortcode::Recent`] — recent-posts query with fallback markdown
/// - [`Shortcode::Apply`] — inline apply / membership-request form
///
/// Phase B migrations add one variant per commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Shortcode {
    /// `:::subscribe` — inline newsletter signup form.
    ///
    /// Configuration is via attributes (`placeholder`, `button`); body
    /// must be empty under the unified grammar. Description text and
    /// any framing prose live in the surrounding markdown.
    Subscribe(SubscribeShortcode),
    /// `:::buttons {.classname}` — list of action buttons.
    ///
    /// Body is one markdown link per line (`[text](url)`). The first
    /// button gets the primary class; subsequent buttons get secondary.
    /// Optional `{.classname}` extra classes attach to the wrapping div.
    ///
    /// URLs flow through [`Url::Unresolved`] at parse time;
    /// [`crate::ast::visit::visit_urls_mut`] (or src-tauri's
    /// `apply_typed_shortcodes`) classifies them into [`Url::Resolved`]
    /// before rendering. The resolver-bypass class is closed by
    /// construction: `RenderHooks::render_shortcode` reads `Url::Resolved`,
    /// so a missing visitor is a debug-time crash.
    Buttons(ButtonsShortcode),
    /// `:::gallery N {.classname}` — image gallery with optional columns.
    ///
    /// `N` (positional integer) sets `--moss-gallery-columns` CSS variable.
    /// Body is one image reference per line: `![alt](path)`, bare
    /// `path.jpg`, or `path|attrs` for media attributes (passed through
    /// to the renderer's inline style).
    Gallery(GalleryShortcode),
    /// `:::hero {image=path}` — full-width hero section with media + overlay.
    ///
    /// New grammar: `image` attribute carries the path. Backward-compat:
    /// when `image` is absent, the extractor scans the first non-empty
    /// body line for a media reference (`![[path]]`, `![alt](path)`, or
    /// bare media filename).
    ///
    /// The pipeline hoists the rendered hero HTML into the article
    /// template's hero slot — it does NOT render inline.
    Hero(HeroShortcode),
    /// `:::grid {cols=N}` or `:::grid N` — flexible multi-cell layout.
    ///
    /// Cells are split on `+++` (new grammar) or `---` (legacy moss-releases
    /// backward-compat — Step 3 of #613 rewrites these to `+++`). Each cell
    /// stores its raw markdown source; the renderer is responsible for any
    /// nested-shortcode extraction and markdown processing per cell.
    Grid(GridShortcode),
    /// `:::recent since=... last=... count=...` — list of recent posts
    /// scoped to the page's top-level folder (its scope).
    ///
    /// Body (between the opening and closing `:::`) is reserved for
    /// fallback content rendered when the query returns zero matches.
    /// Empty body means no fallback (the shortcode renders nothing).
    Recent(RecentShortcode),
    /// `:::apply` — inline membership/contributor application form.
    ///
    /// Posts to the moss-seta `/apply` endpoint with fields `email`,
    /// `matters`, `publish`, `scope`, and `website` (honeypot).
    /// Configuration is via attributes (`placeholder`, `button`); body
    /// must be empty. One-of {matters, publish} is server-enforced.
    /// Succeeds terminally (no auto-revert) via `data-revert="false"`.
    Apply(ApplyShortcode),
}

/// Arguments for [`Shortcode::Subscribe`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribeShortcode {
    /// Optional override for the email input's placeholder text.
    pub placeholder: Option<String>,
    /// Optional override for the submit button label.
    pub button: Option<String>,
}

/// Arguments for [`Shortcode::Buttons`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonsShortcode {
    /// Extra CSS classes for the wrapping `<div>` (from `{.foo .bar}`).
    pub classes: String,
    /// Each button's text + URL. The first item renders as primary, the
    /// rest as secondary. Empty list = the shortcode renders nothing.
    pub items: Vec<ButtonItem>,
}

/// One button in a [`ButtonsShortcode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonItem {
    /// Display text inside the `<a>` tag.
    pub text: String,
    /// Click target. Author input as parsed; flows through
    /// [`crate::ast::visit::visit_urls_mut`] before reaching the renderer.
    pub url: Url,
}

/// Arguments for [`Shortcode::Gallery`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GalleryShortcode {
    /// Optional column count for `--moss-gallery-columns` CSS variable.
    pub columns: Option<u32>,
    /// Extra CSS classes for the wrapping `<div>` (from `{.foo .bar}`).
    pub classes: String,
    /// Each gallery image's src + alt + media attrs.
    pub items: Vec<GalleryItem>,
    /// Spec § P9 width attribute: `body | wide | page | screen` (with
    /// `full` aliased to `screen`). `None` means the author did not set
    /// a width — the emitter omits `data-width` so the HTML stays sparse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
}

/// One image in a [`GalleryShortcode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GalleryItem {
    /// Image source URL. Flows through resolver before rendering.
    pub src: Url,
    /// Alt text (from `![alt](...)` syntax). Empty if author used bare path.
    pub alt: String,
    /// Pipe-suffix media attributes verbatim (e.g. "cover top",
    /// "1.5:1 contain"). Empty if no pipe in the source.
    /// The renderer parses this via `moss_core::media::parse_media_attrs`.
    pub attrs: String,
}

/// Arguments for [`Shortcode::Grid`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridShortcode {
    /// Column count. Defaults to 1 when neither positional nor `cols=`
    /// attribute is provided.
    pub columns: u32,
    /// Optional ratio string like `"1:2"` or `"1:1:2"`. When present, the
    /// renderer emits it as a custom property —
    /// `style="--moss-grid-ratio:minmax(0, 1fr) minmax(0, 2fr)"` — which the
    /// stylesheet reads. Never as an inline `grid-template-columns`: that
    /// outranks every rule, so the mobile single-column collapse could not
    /// reach a ratio grid.
    /// `cols=1:2:3` is equivalent to setting both `columns` (count = 3)
    /// and `ratio` to `"1:2:3"`.
    pub ratio: Option<String>,
    /// Extra CSS classes for the wrapping `<div>` (from `{.foo .bar}`).
    pub classes: String,
    /// Each cell's parsed block content. Phase 4 PR4.5 (2026-05-28)
    /// promoted this from `Vec<String>` (raw markdown source) to
    /// `Vec<Vec<Block>>` (fully-typed AST). Nested shortcodes inside a
    /// cell (`::::buttons` in `:::grid`) extract through
    /// [`crate::ast::parser::parse`] recursion in
    /// [`crate::ast::shortcode_extract::parse_grid`].
    ///
    /// Compound-link cells (the SoCiviC `[![[poster]] ### Title ...](/url)`
    /// pattern, where the entire cell is wrapped in a markdown link that
    /// spans block-level inner content) are represented as a single-element
    /// `vec![Block::LinkCard { url, children }]`. See
    /// [`Block::LinkCard`](crate::ast::Block::LinkCard) for the rationale.
    pub cells: Vec<Vec<Block>>,
    /// Spec § P9 width attribute: `body | wide | page | screen` (with
    /// `full` aliased to `screen`). `None` means the author did not set
    /// a width — the emitter omits `data-width` so the HTML stays sparse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
}

/// Arguments for [`Shortcode::Hero`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroShortcode {
    /// Primary image source URL. `None` if neither the `image` attribute
    /// nor a leading body media line provided one — the renderer emits a
    /// section with no `<img>` in that case. Flows through resolver before
    /// rendering. With `extra_images`, this is the first slide and the
    /// reduced-motion/static fallback.
    pub image: Option<Url>,
    /// Remaining background slides (2026-07-27 multi-image hero): every
    /// consecutive leading body media line after the first. Non-empty →
    /// the hero renders an ambient crossfade (one slide visible at a
    /// time, no controls — no slide may carry information the others
    /// don't; design: docs/archive/2026-07-27-import-conventions-engine-design.md).
    /// Empty for `image=`-attribute and directive-line heroes.
    pub extra_images: Vec<Url>,
    /// Pipe-suffix media attributes verbatim (e.g. "cover top",
    /// "1.5:1 contain"). Empty if no pipe in the source.
    pub attrs: String,
    /// Extra CSS classes for the wrapping `<section>` (from `{.foo .bar}`).
    pub classes: String,
    /// Parsed block content for the overlay. Phase 4 PR4.5 (2026-05-28)
    /// promoted this from `overlay_markdown: String` to `overlay: Vec<Block>`
    /// (fully-typed AST). Nested shortcodes inside the overlay
    /// (`::::buttons` inside `:::hero`) extract through
    /// [`crate::ast::parser::parse`] recursion in
    /// [`crate::ast::shortcode_extract::parse_hero`].
    pub overlay: Vec<Block>,
    /// Plain-text overlay source for downstream OG-fallback extraction.
    ///
    /// PR4.5 (2026-05-28): captured at parse time alongside the typed
    /// `overlay` because `crate::build::page::meta::extract_description`
    /// (the homepage-hero rung in the description chain) operates on
    /// markdown source — round-tripping `Vec<Block>` to markdown would
    /// invite drift. The renderer uses `overlay` for HTML; downstream
    /// consumers read `overlay_text` for description-chain extraction.
    ///
    /// Empty when the author wrote no overlay body.
    ///
    /// TODO(phase4-cleanup): replace with a Vec<Block>-walking
    /// `to_plain_text(blocks: &[Block]) -> String` helper in moss-core
    /// + consume `overlay` directly in `meta.rs::extract_description`,
    /// deleting this field. Carrying both `overlay: Vec<Block>` AND
    /// `overlay_text: String` makes the AST non-canonical (which is the
    /// source of truth?); per cross-SSG research, lossy or duplicate
    /// state is Gatsby's mistake. This is transitional — flag if it
    /// survives past PR7a. (Architecture review caveat 2026-05-28.)
    pub overlay_text: String,
    /// Spec § P9 width attribute: `body | wide | page | screen` (with
    /// `full` aliased to `screen`). `None` means the author did not set
    /// a width — the emitter omits `data-width` so the HTML stays sparse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
    /// Mobile layout override. `Some("overlay")` keeps text overlaid on a
    /// taller cropped image on mobile. `None` = default stacking behavior
    /// (image full-width at natural ratio, text block below).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mobile: Option<String>,
    /// Caption or credit for the image, from `caption="…"`. Rendered as a
    /// line of text BELOW the hero, never over it.
    ///
    /// The overlay and the caption answer different questions. The overlay is
    /// text laid *on* the photograph — a title, a standfirst — and it is
    /// styled to be read against the image. A caption says what the
    /// photograph is and who took it, and printing that across someone's
    /// picture is both unreadable and, for a credit, wrong: a photographer's
    /// name has to survive as text, not as part of the composition. So a hero
    /// carrying a cover credit («封面：…（拍攝：…）») has somewhere to put it
    /// that is not on top of the subject.
    ///
    /// A display string, rendered as inline markdown by the host — the same
    /// treatment `byline:` / `colophon:` rows get — so a credit can be a link.
    /// Empty when the author wrote no caption.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
}

/// Arguments for [`Shortcode::Apply`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyShortcode {
    /// Optional override for the email input's placeholder text.
    pub placeholder: Option<String>,
    /// Optional override for the submit button label.
    pub button: Option<String>,
}

/// Arguments for [`Shortcode::Recent`].
///
/// Parameters parsed at shortcode-extract time; the query runs at render
/// time against the full post set. Renderer lives in
/// `src-tauri/src/build/markdown/recent.rs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentShortcode {
    /// `since="YYYY-MM-DD"` — posts on or after this date. Stored as the
    /// raw string here; the rendering layer parses it into a DateTime.
    /// Mutually compatible with `last` — both set the cutoff, later wins.
    pub since: Option<String>,
    /// `last="week" | "month" | "Nd"` — relative window. The renderer
    /// converts this to a duration and subtracts from now.
    pub last: Option<String>,
    /// `count="N"` — cap at N most recent posts. The renderer applies a
    /// default of 10 when unset.
    pub count: Option<u32>,
    /// Body content rendered as fallback when zero posts match. Empty
    /// string means no fallback. Lives in the AST so the renderer doesn't
    /// need to re-read the source.
    pub fallback_markdown: String,
}

/// Identifier for a shortcode kind, used for AST queries (e.g.
/// `has_shortcode(&doc, ShortcodeKind::Subscribe)` to gate feature
/// detection without scanning source files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcodeKind {
    Subscribe,
    Buttons,
    Gallery,
    Hero,
    Grid,
    Recent,
    Apply,
}

impl ShortcodeKind {
    /// Root `moss-*` class this shortcode emits — bridge between the
    /// parser's authorable set and the COMPONENTS contract.
    pub fn root_class(self) -> &'static str {
        match self {
            ShortcodeKind::Hero => "moss-hero",
            ShortcodeKind::Grid => "moss-grid",
            ShortcodeKind::Gallery => "moss-gallery",
            ShortcodeKind::Buttons => "moss-buttons",
            ShortcodeKind::Subscribe => "moss-subscribe",
            ShortcodeKind::Recent => "moss-recent",
            ShortcodeKind::Apply => "moss-apply",
        }
    }

    /// The fence name authors write after `:::` — also the serde
    /// `snake_case` form (a unit test in `contract::shortcodes` pins the
    /// two together).
    pub fn name(self) -> &'static str {
        match self {
            ShortcodeKind::Subscribe => "subscribe",
            ShortcodeKind::Buttons => "buttons",
            ShortcodeKind::Gallery => "gallery",
            ShortcodeKind::Hero => "hero",
            ShortcodeKind::Grid => "grid",
            ShortcodeKind::Recent => "recent",
            ShortcodeKind::Apply => "apply",
        }
    }

    /// Whether editors offer this shortcode to authors (slash menu, fence
    /// autocomplete). Deliberate — keep hidden: `apply` is the membership
    /// application form, meaningful only on sites configured for it, so it
    /// parses and renders but is never suggested. The decision lives here,
    /// in Rust, once — the generated catalog
    /// (`frontend/app/editor/shortcodes.generated.ts`) carries it as the
    /// `authorable` flag (design: docs/archive/2026-08-11-cm6-extraction-design.md §4, §7.1).
    pub fn authorable(self) -> bool {
        !matches!(self, ShortcodeKind::Apply)
    }

    /// All shortcode variants, in a stable order.
    ///
    /// Used for enforcement and round-trip tests in `components_test.rs`.
    pub fn all() -> impl Iterator<Item = ShortcodeKind> {
        [
            ShortcodeKind::Subscribe,
            ShortcodeKind::Buttons,
            ShortcodeKind::Gallery,
            ShortcodeKind::Hero,
            ShortcodeKind::Grid,
            ShortcodeKind::Recent,
            ShortcodeKind::Apply,
        ]
        .into_iter()
    }
}

impl Shortcode {
    /// Return the [`ShortcodeKind`] of this shortcode.
    pub fn kind(&self) -> ShortcodeKind {
        match self {
            Shortcode::Subscribe(_) => ShortcodeKind::Subscribe,
            Shortcode::Buttons(_) => ShortcodeKind::Buttons,
            Shortcode::Gallery(_) => ShortcodeKind::Gallery,
            Shortcode::Hero(_) => ShortcodeKind::Hero,
            Shortcode::Grid(_) => ShortcodeKind::Grid,
            Shortcode::Recent(_) => ShortcodeKind::Recent,
            Shortcode::Apply(_) => ShortcodeKind::Apply,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcode_kind_variants_are_distinct() {
        let kinds = [
            ShortcodeKind::Subscribe,
            ShortcodeKind::Buttons,
            ShortcodeKind::Gallery,
            ShortcodeKind::Hero,
            ShortcodeKind::Grid,
            ShortcodeKind::Recent,
        ];
        let unique: std::collections::HashSet<_> = kinds.iter().collect();
        assert_eq!(unique.len(), kinds.len());
    }

    #[test]
    fn shortcode_kind_round_trips_through_serde() {
        for kind in [
            ShortcodeKind::Subscribe,
            ShortcodeKind::Buttons,
            ShortcodeKind::Gallery,
            ShortcodeKind::Hero,
            ShortcodeKind::Grid,
            ShortcodeKind::Recent,
        ] {
            let s = serde_json::to_string(&kind).expect("serialize");
            let back: ShortcodeKind = serde_json::from_str(&s).expect("deserialize");
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn subscribe_kind_method_returns_subscribe() {
        let sc = Shortcode::Subscribe(SubscribeShortcode::default());
        assert_eq!(sc.kind(), ShortcodeKind::Subscribe);
    }

    #[test]
    fn subscribe_with_placeholder_and_button() {
        let sc = Shortcode::Subscribe(SubscribeShortcode {
            placeholder: Some("you@example.com".to_string()),
            button: Some("Subscribe".to_string()),
        });
        match &sc {
            Shortcode::Subscribe(args) => {
                assert_eq!(args.placeholder.as_deref(), Some("you@example.com"));
                assert_eq!(args.button.as_deref(), Some("Subscribe"));
            }
            other => panic!("expected Subscribe, got {other:?}"),
        }
    }

    #[test]
    fn subscribe_default_has_none_placeholder_and_button() {
        let args = SubscribeShortcode::default();
        assert!(args.placeholder.is_none());
        assert!(args.button.is_none());
    }

    #[test]
    fn subscribe_round_trips_through_serde() {
        let sc = Shortcode::Subscribe(SubscribeShortcode {
            placeholder: Some("p".to_string()),
            button: Some("b".to_string()),
        });
        let s = serde_json::to_string(&sc).expect("serialize");
        let back: Shortcode = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(sc, back);
    }

    // ---- Buttons ----

    #[test]
    fn buttons_kind_method_returns_buttons() {
        let sc = Shortcode::Buttons(ButtonsShortcode::default());
        assert_eq!(sc.kind(), ShortcodeKind::Buttons);
    }

    #[test]
    fn buttons_items_carry_unresolved_urls() {
        let sc = Shortcode::Buttons(ButtonsShortcode {
            classes: String::new(),
            items: vec![
                ButtonItem {
                    text: "Docs".to_string(),
                    url: Url::unresolved("docs/"),
                },
                ButtonItem {
                    text: "GitHub".to_string(),
                    url: Url::unresolved("https://github.com"),
                },
            ],
        });
        match &sc {
            Shortcode::Buttons(args) => {
                assert_eq!(args.items.len(), 2);
                assert!(args.items[0].url.is_unresolved());
                assert!(args.items[1].url.is_unresolved());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn buttons_default_has_no_items() {
        let args = ButtonsShortcode::default();
        assert!(args.items.is_empty());
        assert!(args.classes.is_empty());
    }

    #[test]
    fn buttons_round_trips_through_serde() {
        let sc = Shortcode::Buttons(ButtonsShortcode {
            classes: "primary".to_string(),
            items: vec![ButtonItem {
                text: "Go".to_string(),
                url: Url::unresolved("/x"),
            }],
        });
        let s = serde_json::to_string(&sc).expect("serialize");
        let back: Shortcode = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(sc, back);
    }

    // ---- Gallery ----

    #[test]
    fn gallery_kind_method_returns_gallery() {
        let sc = Shortcode::Gallery(GalleryShortcode::default());
        assert_eq!(sc.kind(), ShortcodeKind::Gallery);
    }

    #[test]
    fn gallery_items_carry_unresolved_urls() {
        let sc = Shortcode::Gallery(GalleryShortcode {
            columns: Some(3),
            classes: String::new(),
            items: vec![
                GalleryItem {
                    src: Url::unresolved("a.jpg"),
                    alt: "A".to_string(),
                    attrs: String::new(),
                },
                GalleryItem {
                    src: Url::unresolved("b.jpg"),
                    alt: "B".to_string(),
                    attrs: "cover top".to_string(),
                },
            ],
            width: None,
        });
        match &sc {
            Shortcode::Gallery(args) => {
                assert_eq!(args.columns, Some(3));
                assert_eq!(args.items.len(), 2);
                assert!(args.items[0].src.is_unresolved());
                assert_eq!(args.items[1].attrs, "cover top");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn gallery_default_no_columns_no_items() {
        let args = GalleryShortcode::default();
        assert!(args.columns.is_none());
        assert!(args.items.is_empty());
        assert!(args.classes.is_empty());
    }

    #[test]
    fn gallery_round_trips_through_serde() {
        let sc = Shortcode::Gallery(GalleryShortcode {
            columns: Some(4),
            classes: "showcase".to_string(),
            items: vec![GalleryItem {
                src: Url::unresolved("p.png"),
                alt: "Photo".to_string(),
                attrs: "1:1 contain".to_string(),
            }],
            width: None,
        });
        let s = serde_json::to_string(&sc).expect("serialize");
        let back: Shortcode = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(sc, back);
    }

    // ---- Recent ----

    #[test]
    fn recent_kind_method_returns_recent() {
        let sc = Shortcode::Recent(RecentShortcode::default());
        assert_eq!(sc.kind(), ShortcodeKind::Recent);
    }

    #[test]
    fn recent_default_has_none_params_empty_fallback() {
        let args = RecentShortcode::default();
        assert!(args.since.is_none());
        assert!(args.last.is_none());
        assert!(args.count.is_none());
        assert!(args.fallback_markdown.is_empty());
    }

    #[test]
    fn recent_round_trips_through_serde() {
        let sc = Shortcode::Recent(RecentShortcode {
            since: Some("2026-04-01".to_string()),
            last: None,
            count: Some(5),
            fallback_markdown: "_No posts yet._".to_string(),
        });
        let s = serde_json::to_string(&sc).expect("serialize");
        let back: Shortcode = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(sc, back);
    }

    // ---- Hero ----

    #[test]
    fn hero_mobile_field_defaults_to_none() {
        let args = HeroShortcode::default();
        assert!(args.mobile.is_none());
    }

    #[test]
    fn hero_with_mobile_overlay_round_trips_serde() {
        let sc = Shortcode::Hero(HeroShortcode {
            mobile: Some("overlay".to_string()),
            ..Default::default()
        });
        let s = serde_json::to_string(&sc).expect("serialize");
        let back: Shortcode = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(sc, back);
    }
}
