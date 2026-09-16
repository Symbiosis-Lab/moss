//! Block-level and inline-level AST nodes.
//!
//! Closed enums; pattern matching is the visitor framework. The variants
//! model every construct [`super::parser::parser_options`] turns on —
//! CommonMark plus the GFM tables, strikethrough, footnotes and task lists
//! moss enables (ADR-035 § Task lists, amended) — so nothing pulldown-cmark
//! emits for those constructs reaches a catch-all.
//!
//! `Block::Other` / `Inline::Other` are NOT a general catch-all. They carry
//! **raw HTML only** — `Tag::HtmlBlock` and `Event::Html`/`InlineHtml` in
//! [`super::parser`] — plus payloads moss synthesizes itself (math, ADR-030;
//! dispatched wikilink embeds). Anything pulldown-cmark emits that has no arm
//! is **dropped, not passed through**: `parse_block`, `parse_block_with_tag`
//! and `parse_inline` all end in `_ => (None, 1)`, and
//! `parse_inline_event`'s whitelist ends in `_ => None`.
//!
//! So turning on a new `Options` bit is never a one-line flag flip. It needs a
//! variant here PLUS arms in `parse_block_with_tag` / `parse_inline` /
//! `parse_inline_event`'s whitelist, in the same change, or the construct
//! silently disappears from published pages. That is not hypothetical: before
//! ADR-035, `~~` was a construct the AST hadn't modeled and it did not flow
//! through `Inline::Other` — `~~gone~~ stays` published `gone stays` for two
//! months. See ADR-035 § Why now.

use serde::{Deserialize, Serialize};

use super::shortcode::Shortcode;
use super::url::Url;

/// Canonical callout kind. Obsidian-dialect aliases canonicalize via
/// [`CalloutKind::from_raw`] (e.g. `tldr`/`summary` → [`CalloutKind::Abstract`]).
/// Unknown kinds fall back to [`CalloutKind::Note`]; the parser logs at
/// trace level (Diagnostic threading is a Phase 4 followup — see
/// `validation::Diagnostic`, today scoped to frontmatter validation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Note,
    Abstract,
    Info,
    Todo,
    Tip,
    Success,
    Question,
    Warning,
    Failure,
    Danger,
    Bug,
    Example,
    Quote,
    Important,
    Summary,
    Help,
}

/// Per-column table alignment declared in GFM source (`|:--|` left,
/// `|:-:|` center, `|--:|` right; a bare `|---|` is [`ColumnAlignment::None`]).
///
/// This is **source-faithful**: it records only what the author wrote, so the
/// editor and any AST consumer see the same alignment the source declares.
/// Numeric auto-right-alignment (for columns the author left unaligned) is a
/// render-time transform in `ast::render`, deliberately *not* stored here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnAlignment {
    None,
    Left,
    Center,
    Right,
}

impl CalloutKind {
    /// Canonicalize a raw callout name (case-insensitive) to a
    /// [`CalloutKind`]. Returns `None` if the name is not a recognized
    /// canonical kind or alias.
    ///
    /// Alias table (Obsidian-dialect, per shape-spec § 1):
    /// - `tldr` / `summary` → `Abstract`
    /// - `hint` / `important` → `Tip`
    /// - `check` / `done` → `Success`
    /// - `help` / `faq` → `Question`
    /// - `caution` / `attention` → `Warning`
    /// - `fail` / `missing` → `Failure`
    /// - `error` → `Danger`
    /// - `cite` → `Quote`
    ///
    /// `pending` is also accepted as an alias for `Todo` (used by
    /// SoCiviC Theatre voices.md; carried over from pre-Phase-4 Stage 1
    /// support in `crates/moss-core/src/resolve/callouts.rs`).
    ///
    /// Note: the [`CalloutKind`] enum reserves `Important`, `Summary`,
    /// and `Help` as canonical variants for future Stage 2 use (e.g.
    /// editor-emitted callouts that should not name-clash with the
    /// Obsidian aliases above). Author markdown can't currently produce
    /// these three through `from_raw`; they're reachable only via
    /// programmatic construction.
    pub fn from_raw(raw: &str) -> Option<Self> {
        let lower = raw.to_lowercase();
        let canonical = match lower.as_str() {
            // Canonical kinds (exact match, alias-free names)
            "note" => Self::Note,
            "abstract" => Self::Abstract,
            "info" => Self::Info,
            "todo" => Self::Todo,
            "tip" => Self::Tip,
            "success" => Self::Success,
            "question" => Self::Question,
            "warning" => Self::Warning,
            "failure" => Self::Failure,
            "danger" => Self::Danger,
            "bug" => Self::Bug,
            "example" => Self::Example,
            "quote" => Self::Quote,
            // Obsidian-dialect aliases (shape-spec § 1)
            "tldr" | "summary" => Self::Abstract,
            "hint" | "important" => Self::Tip,
            "check" | "done" => Self::Success,
            "help" | "faq" => Self::Question,
            "caution" | "attention" => Self::Warning,
            "fail" | "missing" => Self::Failure,
            "error" => Self::Danger,
            "cite" => Self::Quote,
            // Legacy alias retained from pre-Phase-4 Stage 1
            // (`crates/moss-core/src/resolve/callouts.rs`). SoCiviC
            // Theatre's voices.md uses `> [!pending]`; map to Todo.
            "pending" => Self::Todo,
            _ => return None,
        };
        Some(canonical)
    }

    /// Slug form used in the rendered `data-type` attribute.
    pub fn as_slug(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Abstract => "abstract",
            Self::Info => "info",
            Self::Todo => "todo",
            Self::Tip => "tip",
            Self::Success => "success",
            Self::Question => "question",
            Self::Warning => "warning",
            Self::Failure => "failure",
            Self::Danger => "danger",
            Self::Bug => "bug",
            Self::Example => "example",
            Self::Quote => "quote",
            Self::Important => "important",
            Self::Summary => "summary",
            Self::Help => "help",
        }
    }

    /// Default display title (capitalized canonical kind) used when the
    /// author wrote `> [!type]` with no inline title text.
    pub fn default_title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Abstract => "Abstract",
            Self::Info => "Info",
            Self::Todo => "Todo",
            Self::Tip => "Tip",
            Self::Success => "Success",
            Self::Question => "Question",
            Self::Warning => "Warning",
            Self::Failure => "Failure",
            Self::Danger => "Danger",
            Self::Bug => "Bug",
            Self::Example => "Example",
            Self::Quote => "Quote",
            Self::Important => "Important",
            Self::Summary => "Summary",
            Self::Help => "Help",
        }
    }
}

/// Foldable callout state. `> [!type]+` → [`Fold::Open`] (foldable,
/// open by default); `> [!type]-` → [`Fold::Closed`] (foldable, closed
/// by default). Non-foldable callouts have `fold: None` on the
/// containing [`Block::Callout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fold {
    Open,
    Closed,
}

/// A block-level AST node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Block {
    /// `# Heading` (level 1) through `###### Heading` (level 6).
    Heading {
        level: u8,
        children: Vec<Inline>,
        /// Heading anchor id (slug). Computed by the parser via
        /// [`crate::heading::anchor::obsidian_heading_anchor`].
        id: Option<String>,
    },
    /// A paragraph of inline content.
    Paragraph(Vec<Inline>),
    /// `> [!type] body` — typed callouts. The `kind` is canonicalized
    /// via [`CalloutKind::from_raw`] (Obsidian-dialect aliases collapse
    /// to the canonical 16-kind set). Foldable callouts (`> [!type]+`
    /// open by default, `> [!type]-` closed) carry the [`Fold`] state;
    /// non-foldable callouts have `fold: None`.
    ///
    /// Phase 4 PR4 extended the shape from `kind: String` to
    /// `kind: CalloutKind` + added `fold: Option<Fold>` and `title: Option<String>`.
    /// Title is the optional inline text following the marker
    /// (`> [!note] My title` → `title: Some("My title")`).
    Callout {
        kind: CalloutKind,
        fold: Option<Fold>,
        title: Option<String>,
        children: Vec<Block>,
    },
    /// `- item` / `1. item`. Each item is a list of blocks (so list items
    /// can carry paragraphs, sub-lists, etc).
    ///
    /// `item_source_lines` is a parallel-to-`items` vec of 1-based source
    /// line numbers, populated by the parser only when
    /// [`crate::ast::ParseConfig::emit_source_lines`] is true. When tracking
    /// is off (production publish builds, the ~40 in-crate `parse()` callers
    /// that use the default config), the vec is empty (`vec![]`) and the
    /// renderer treats every item as `None` — no `data-source-line` on the
    /// emitted `<li>`. When tracking is on, length matches `items.len()`
    /// exactly; individual entries may still be `None` for synthesized
    /// items that have no faithful source position (none today, but kept
    /// for symmetry with [`crate::ast::document::BlockMeta::source_line`]).
    ///
    /// Phase 4 source-lines followup (2026-05-28): added because the
    /// preview's scroll-sync (cm-scroll-sync via
    /// `frontend/bridge/iframe-bridge.ts`) interpolates editor positions
    /// proportionally between annotated DOM elements. A 30-item list
    /// spanning 50 source lines without per-`<li>` annotations forces
    /// interpolation between the outer `<ul>` and the next top-level
    /// block — potentially 100 lines away. Legacy `transform_events`
    /// (commit f91aca8fa, 2026-04-01) emitted on `<li>` and `<tr>` for
    /// this reason; the typed-AST renderer now matches.
    List {
        ordered: bool,
        /// Explicit ordered-list start number (pulldown-cmark's
        /// `Tag::List(Option<u64>)` payload). `Some(N)` when the source
        /// is `N. item` and the renderer should emit `<ol start="N">`;
        /// `None` for unordered lists and for ordered lists where N is
        /// the implicit default `1`. CommonMark only honors the FIRST
        /// item's number as the list start; subsequent numbers are
        /// re-derived. Phase 4 followup B (2026-05-28): added because
        /// `<ol>` was previously emitted for any ordered list,
        /// silently dropping the explicit start number — `3. foo`
        /// rendered as `<ol><li>foo</li></ol>` instead of
        /// `<ol start="3"><li>foo</li></ol>`.
        #[serde(default)]
        start: Option<u64>,
        items: Vec<Vec<Block>>,
        #[serde(default)]
        item_source_lines: Vec<Option<usize>>,
    },
    /// A fenced code block.
    CodeBlock { lang: Option<String>, value: String },
    /// Markdown table.
    ///
    /// `header_source_line` and `row_source_lines` are populated by the
    /// parser only when [`crate::ast::ParseConfig::emit_source_lines`] is
    /// true. When tracking is off, `header_source_line` is `None` and
    /// `row_source_lines` is empty (`vec![]`); the renderer emits no
    /// `data-source-line` attributes. When tracking is on,
    /// `row_source_lines.len() == rows.len()`.
    ///
    /// Phase 4 source-lines followup (2026-05-28): see the corresponding
    /// doc comment on `Block::List` for the scroll-sync interpolation
    /// rationale.
    Table {
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        /// Per-column GFM alignment, parallel to `header`. Empty (the common
        /// case) means the author declared no alignment on any column.
        /// `skip_serializing_if` keeps previously-serialized ASTs and snapshot
        /// fixtures byte-stable when there is no alignment to record.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        alignments: Vec<ColumnAlignment>,
        #[serde(default)]
        header_source_line: Option<usize>,
        #[serde(default)]
        row_source_lines: Vec<Option<usize>>,
    },
    /// `> blockquote`
    BlockQuote(Vec<Block>),
    /// A typed shortcode block (`:::name ...args\n body :::`).
    Shortcode(Shortcode),
    /// `<hr>` thematic break.
    ThematicBreak,
    /// Image-only paragraph promoted to a typed figure.
    ///
    /// Detected by the parser's `Tag::Paragraph` arm (Phase 4 PR3,
    /// 2026-05-27): a paragraph that contains exactly one
    /// [`Inline::Image`] modulo whitespace text and line breaks. The
    /// renderer emits `<figure class="moss-image">…<figcaption>…</figcaption></figure>`,
    /// wrapping the image hook's output and appending the caption when
    /// present.
    ///
    /// `image` is constrained by the parser to be an [`Inline::Image`];
    /// the renderer pattern-matches and falls back gracefully if the
    /// variant is anything else.
    ///
    /// `caption` defaults to the image's alt text at parse time. `None`
    /// means "figure wrap but no `<figcaption>`" — reserved for the
    /// empty-alt case (omit caption when there is nothing to read).
    ///
    /// The figure-level display params (`width`, `align`, `class_names`,
    /// `img_style`) are populated only when a figure originates from a
    /// parameterized wikilink embed (`![[photo.jpg|wide cover]]`) — the
    /// image-embed synth-collapse routes such embeds through this typed
    /// node so width/fit/position/align survive (previously dropped by the
    /// markdown round-trip). The CommonMark `![](url)` promotion path
    /// (`try_promote_to_figure`) leaves them at their defaults, so its
    /// rendered output is byte-identical to before the collapse.
    Figure {
        image: Inline,
        caption: Option<Vec<Inline>>,
        /// Canonical width token (`body | wide | page | screen`) emitted as
        /// `data-width="…"` on the `<figure>`. `None` omits the attribute.
        /// `String` (not `&'static str`) so `Block` keeps its `Deserialize`
        /// derive; the value is always one of the canonical tokens.
        width: Option<String>,
        /// Figure-level align class (`moss-align-left` / `moss-align-right`),
        /// appended to the `<figure>` class list. `None` omits it.
        align: Option<String>,
        /// Extra author-supplied class names appended to the `<figure>` class
        /// list (after `moss-image` and any align class). Empty = none.
        class_names: Vec<String>,
        /// Inline `style=` fragment for the INNER `<img>` (e.g.
        /// `object-fit:cover;object-position:left` from a `|cover left`
        /// embed). `None` omits it. Distinct from any figure-level attribute:
        /// fit/position style belongs on the image element, not the figure.
        img_style: Option<String>,
    },
    /// Compound-link grid cell: the entire cell is a single markdown
    /// link `[inner](url)` whose `inner` is parsed as block-level content
    /// (images, headings, paragraphs, emphasis). The SoCiviC Theatre
    /// pattern: `[![[poster]] ### Title *date* description](/url)`.
    ///
    /// Phase 4 PR4.5 (2026-05-28): added because CommonMark restricts
    /// `Inline::Link.children` to inline-level content; a markdown link
    /// wrapping `### Heading` + paragraphs cannot round-trip through
    /// pulldown-cmark's inline parser. The cell-string-level shape
    /// (`[...](url)` with multi-paragraph inner content) is detected by
    /// `crate::ast::shortcode_extract::parse_grid` BEFORE the cell flows
    /// through `crate::ast::parser::parse`; the matched cell yields a
    /// single-element `vec![Block::LinkCard { url, children }]` with the
    /// inner markdown parsed into typed blocks.
    ///
    /// Render shape (matches today's `render_compound_link_cell` byte
    /// shape):
    /// - External URL (`http(s)://...`): `<a href=URL class="moss-grid-card link-preview" target="_blank" rel="noopener">children</a>`.
    /// - Internal URL: `<a href=URL class="moss-grid-card" data-kind="link">children</a>`.
    LinkCard { url: Url, children: Vec<Block> },
    /// `[^label]: body` — a GFM footnote definition, wherever the author
    /// wrote it (pulldown-cmark nests one written inside a blockquote or a
    /// list item under that container). The renderer hoists it out to the
    /// document's endnote section; see ADR-035.
    FootnoteDefinition { label: String, children: Vec<Block> },
    /// Raw HTML passthrough: a `Tag::HtmlBlock` the author wrote, or a
    /// payload moss synthesized itself (the shortcode sentinel pass in
    /// `dispatch_wikilink_embeds`). Emitted verbatim. NOT a fallback for
    /// unmodeled pulldown constructs — see the module doc.
    Other(String),
}

/// An inline-level AST node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inline {
    Text(String),
    /// `[content](url "title")` or `[[wikilink]]`.
    ///
    /// `is_wikilink` preserves pulldown-cmark's `LinkType::WikiLink`
    /// discriminator at parse time so the renderer can emit
    /// `class="wikilink"` on the `<a>` tag and downstream consumers
    /// (graph builder, link-resolver) can distinguish wikilink targets
    /// from standard markdown links. Added Phase 4 PR7a (2026-05-28) as
    /// the smallest AST change matching mdast convention (Link node +
    /// extension flag, mirroring `LinkType::WikiLink` as a tag).
    Link {
        url: Url,
        title: Option<String>,
        children: Vec<Inline>,
        /// True when pulldown-cmark emitted `Tag::Link { link_type:
        /// LinkType::WikiLink, .. }` (i.e. the markdown source was
        /// `[[target]]` or `[[target|alias]]`, post-Stage-1 rewrite).
        /// Renderer adds `class="wikilink"` for true.
        #[serde(default)]
        is_wikilink: bool,
    },
    /// `![alt](src "title")` or `![[wikilink]]`.
    ///
    /// `is_wikilink` + `wikilink_pothole` (Phase 4 PR7a-flip-core-B,
    /// 2026-05-28) preserve pulldown-cmark's `LinkType::WikiLink`
    /// discriminator and the original pothole text so the
    /// `dispatch_wikilink_embeds` visitor can route `![[v.mp4|width=400]]`
    /// → per-extension renderer (video / pdf / audio / iframe / 3D /
    /// notebook / etc) with the typed params intact. The parser's
    /// `Tag::Image` arm captures the raw pothole BEFORE PR3.5's
    /// wikilink-alt classification consumes it into the `alt` field;
    /// without preservation, the `width=400` token is erased after
    /// alt-classification.
    ///
    /// `is_wikilink: false` and `wikilink_pothole: None` for standard
    /// `![alt](src)` markdown images.
    Image {
        src: Url,
        alt: String,
        title: Option<String>,
        /// True when pulldown-cmark emitted `Tag::Image { link_type:
        /// LinkType::WikiLink, .. }` (i.e. the markdown source was
        /// `![[target]]` / `![[target|pothole]]`). Mirrors
        /// `Inline::Link.is_wikilink`.
        #[serde(default)]
        is_wikilink: bool,
        /// Original pothole text (after `|`) preserved verbatim from the
        /// pulldown-cmark text events for wikilink images. `None` for
        /// non-wikilink images and for wikilinks without a pothole
        /// (pulldown-cmark synthesizes the dest as text when no pothole
        /// is present).
        #[serde(default)]
        wikilink_pothole: Option<String>,
    },
    /// `*emphasis*`
    Emphasis(Vec<Inline>),
    /// `**strong**`
    Strong(Vec<Inline>),
    /// `` `code` ``
    Code(String),
    /// Hard line break.
    LineBreak,
    /// `~~struck~~`
    Strikethrough(Vec<Inline>),
    /// `[^label]` — a GFM footnote marker. Carries the author's label, not
    /// the printed number: numbering is first-reference order over the whole
    /// document, which is a render-time fact (same rule as
    /// [`ColumnAlignment`]'s numeric auto-alignment). See ADR-035.
    FootnoteRef(String),
    /// The `[ ]` / `[x]` of a GFM task-list item, carrying its checked state.
    ///
    /// Modeled as an INLINE, not as a field on [`Block::List`], because that is
    /// where pulldown-cmark puts it: `Event::TaskListMarker` is the first event
    /// *inside* `Tag::Item`, ahead of the item's own content. Keeping it inline
    /// leaves `Block::List`'s shape untouched and avoids a third vector parallel
    /// to `items` / `item_source_lines`. See ADR-035 § Task lists.
    TaskMarker(bool),
    /// Raw inline HTML passthrough (`Event::Html` / `Event::InlineHtml`),
    /// plus the math node `math_text::math_inline` synthesizes (ADR-030).
    /// NOT a fallback for unmodeled pulldown constructs — see the module doc.
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::super::url::{Url, UrlKind};
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Text(s.to_string())
    }

    #[test]
    fn block_heading_constructable() {
        let b = Block::Heading {
            level: 1,
            children: vec![text("Hello")],
            id: Some("hello".to_string()),
        };
        match b {
            Block::Heading {
                level,
                children,
                id,
            } => {
                assert_eq!(level, 1);
                assert_eq!(children.len(), 1);
                assert_eq!(id.as_deref(), Some("hello"));
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn block_paragraph_holds_inlines() {
        let b = Block::Paragraph(vec![text("hi"), Inline::LineBreak, text("there")]);
        match b {
            Block::Paragraph(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected Paragraph"),
        }
    }

    #[test]
    fn block_list_each_item_is_block_vec() {
        let b = Block::List {
            ordered: false,
            start: None,
            items: vec![
                vec![Block::Paragraph(vec![text("first")])],
                vec![Block::Paragraph(vec![text("second")])],
            ],
            item_source_lines: vec![],
        };
        match b {
            Block::List {
                ordered,
                start,
                items,
                ..
            } => {
                assert!(!ordered);
                assert!(start.is_none());
                assert_eq!(items.len(), 2);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn block_list_carries_explicit_start_number() {
        // Phase 4 followup B (2026-05-28): ordered lists with an
        // explicit non-default start number round-trip through the AST.
        let b = Block::List {
            ordered: true,
            start: Some(3),
            items: vec![vec![Block::Paragraph(vec![text("foo")])]],
            item_source_lines: vec![],
        };
        match b {
            Block::List {
                ordered,
                start,
                items,
                ..
            } => {
                assert!(ordered);
                assert_eq!(start, Some(3));
                assert_eq!(items.len(), 1);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn block_table_two_dim_rows() {
        let b = Block::Table {
            header: vec![vec![text("A")], vec![text("B")]],
            rows: vec![
                vec![vec![text("1")], vec![text("2")]],
                vec![vec![text("3")], vec![text("4")]],
            ],
            alignments: Vec::new(),
            header_source_line: None,
            row_source_lines: vec![],
        };
        match b {
            Block::Table { header, rows, .. } => {
                assert_eq!(header.len(), 2);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
            }
            _ => panic!("expected Table"),
        }
    }

    #[test]
    fn block_other_carries_raw_html() {
        let b = Block::Other("<custom>raw</custom>".to_string());
        match b {
            Block::Other(s) => assert_eq!(s, "<custom>raw</custom>"),
            _ => panic!("expected Other"),
        }
    }

    #[test]
    fn block_thematic_break_is_unit_variant() {
        let b = Block::ThematicBreak;
        assert!(matches!(b, Block::ThematicBreak));
    }

    #[test]
    fn block_figure_carries_image_and_optional_caption() {
        // Phase 4 PR3: Block::Figure wraps a single Inline::Image and an
        // optional caption (vector of inlines so emphasis/strong can ride
        // through). Caption defaults to the image's alt text at parse time;
        // None is reserved for the empty-alt case.
        let image = Inline::Image {
            src: Url::resolved("photo.jpg", UrlKind::Asset),
            alt: "A photo".to_string(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        };
        let b = Block::Figure {
            image: image.clone(),
            caption: Some(vec![text("A photo")]),
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
        };
        match b {
            Block::Figure {
                image: img,
                caption,
                ..
            } => {
                assert!(matches!(img, Inline::Image { .. }));
                let cap = caption.expect("caption present");
                assert_eq!(cap.len(), 1);
            }
            _ => panic!("expected Figure"),
        }
    }

    #[test]
    fn block_figure_without_caption_serializes() {
        // Empty-alt case: caption: None means "no figcaption emission."
        let b = Block::Figure {
            image: Inline::Image {
                src: Url::resolved("x.jpg", UrlKind::Asset),
                alt: String::new(),
                title: None,
                is_wikilink: false,
                wikilink_pothole: None,
            },
            caption: None,
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
        };
        let s = serde_json::to_string(&b).expect("serialize");
        let back: Block = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(b, back);
    }

    #[test]
    fn inline_link_carries_url_and_children() {
        let i = Inline::Link {
            url: Url::unresolved("docs/"),
            title: None,
            children: vec![text("Documentation")],
            is_wikilink: false,
        };
        match i {
            Inline::Link {
                url,
                title,
                children,
                is_wikilink,
            } => {
                assert!(url.is_unresolved());
                assert!(title.is_none());
                assert_eq!(children.len(), 1);
                assert!(!is_wikilink);
            }
            _ => panic!("expected Link"),
        }
    }

    #[test]
    fn inline_image_uses_url_for_src() {
        // Per R6: Inline::Image carries Url (not a separate Src type).
        // UrlKind::Asset is the relevant variant after resolution.
        let i = Inline::Image {
            src: Url::resolved("img/cat.jpg", UrlKind::Asset),
            alt: "Cat".to_string(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        };
        match i {
            Inline::Image {
                src,
                alt,
                title: _,
                is_wikilink: _,
                wikilink_pothole: _,
            } => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.kind, UrlKind::Asset);
                assert_eq!(alt, "Cat");
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn inline_emphasis_and_strong_nest() {
        let i = Inline::Strong(vec![Inline::Emphasis(vec![text("nested")])]);
        match i {
            Inline::Strong(children) => match &children[0] {
                Inline::Emphasis(inner) => assert_eq!(inner.len(), 1),
                _ => panic!("expected Emphasis"),
            },
            _ => panic!("expected Strong"),
        }
    }

    #[test]
    fn block_round_trips_through_serde() {
        let original = Block::Heading {
            level: 2,
            children: vec![Inline::Text("Setup".to_string())],
            id: Some("setup".to_string()),
        };
        let s = serde_json::to_string(&original).expect("serialize");
        let back: Block = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(original, back);
    }

    // -----------------------------------------------------------------
    // Phase 4 PR4: CalloutKind canonicalization
    // -----------------------------------------------------------------

    #[test]
    fn callout_kind_canonicalizes_canonical_names() {
        assert_eq!(CalloutKind::from_raw("note"), Some(CalloutKind::Note));
        assert_eq!(CalloutKind::from_raw("tip"), Some(CalloutKind::Tip));
        assert_eq!(CalloutKind::from_raw("warning"), Some(CalloutKind::Warning));
        assert_eq!(CalloutKind::from_raw("danger"), Some(CalloutKind::Danger));
        assert_eq!(CalloutKind::from_raw("info"), Some(CalloutKind::Info));
        assert_eq!(CalloutKind::from_raw("todo"), Some(CalloutKind::Todo));
        assert_eq!(CalloutKind::from_raw("success"), Some(CalloutKind::Success));
        assert_eq!(
            CalloutKind::from_raw("question"),
            Some(CalloutKind::Question)
        );
        assert_eq!(CalloutKind::from_raw("failure"), Some(CalloutKind::Failure));
        assert_eq!(CalloutKind::from_raw("bug"), Some(CalloutKind::Bug));
        assert_eq!(CalloutKind::from_raw("example"), Some(CalloutKind::Example));
        assert_eq!(CalloutKind::from_raw("quote"), Some(CalloutKind::Quote));
        assert_eq!(
            CalloutKind::from_raw("abstract"),
            Some(CalloutKind::Abstract)
        );
    }

    #[test]
    fn callout_kind_canonicalizes_all_obsidian_aliases() {
        // The 8 alias mappings from shape-spec § 1.
        assert_eq!(CalloutKind::from_raw("tldr"), Some(CalloutKind::Abstract));
        assert_eq!(
            CalloutKind::from_raw("summary"),
            Some(CalloutKind::Abstract)
        );
        assert_eq!(CalloutKind::from_raw("hint"), Some(CalloutKind::Tip));
        assert_eq!(CalloutKind::from_raw("important"), Some(CalloutKind::Tip));
        assert_eq!(CalloutKind::from_raw("check"), Some(CalloutKind::Success));
        assert_eq!(CalloutKind::from_raw("done"), Some(CalloutKind::Success));
        assert_eq!(CalloutKind::from_raw("help"), Some(CalloutKind::Question));
        assert_eq!(CalloutKind::from_raw("faq"), Some(CalloutKind::Question));
        assert_eq!(CalloutKind::from_raw("caution"), Some(CalloutKind::Warning));
        assert_eq!(
            CalloutKind::from_raw("attention"),
            Some(CalloutKind::Warning)
        );
        assert_eq!(CalloutKind::from_raw("fail"), Some(CalloutKind::Failure));
        assert_eq!(CalloutKind::from_raw("missing"), Some(CalloutKind::Failure));
        assert_eq!(CalloutKind::from_raw("error"), Some(CalloutKind::Danger));
        assert_eq!(CalloutKind::from_raw("cite"), Some(CalloutKind::Quote));
        // Legacy alias for SoCiviC Theatre's `> [!pending]` syntax.
        assert_eq!(CalloutKind::from_raw("pending"), Some(CalloutKind::Todo));
    }

    #[test]
    fn callout_kind_is_case_insensitive() {
        assert_eq!(CalloutKind::from_raw("NOTE"), Some(CalloutKind::Note));
        assert_eq!(CalloutKind::from_raw("Warning"), Some(CalloutKind::Warning));
        assert_eq!(CalloutKind::from_raw("TLDR"), Some(CalloutKind::Abstract));
    }

    #[test]
    fn callout_kind_unknown_returns_none() {
        assert_eq!(CalloutKind::from_raw("xyz"), None);
        assert_eq!(CalloutKind::from_raw(""), None);
        assert_eq!(CalloutKind::from_raw("not-a-kind"), None);
    }

    #[test]
    fn callout_kind_slug_matches_canonical_name() {
        assert_eq!(CalloutKind::Note.as_slug(), "note");
        assert_eq!(CalloutKind::Abstract.as_slug(), "abstract");
        assert_eq!(CalloutKind::Warning.as_slug(), "warning");
        assert_eq!(CalloutKind::Danger.as_slug(), "danger");
    }

    #[test]
    fn callout_kind_default_title_is_capitalized() {
        assert_eq!(CalloutKind::Note.default_title(), "Note");
        assert_eq!(CalloutKind::Warning.default_title(), "Warning");
        assert_eq!(CalloutKind::Abstract.default_title(), "Abstract");
    }

    #[test]
    fn block_callout_round_trips_through_serde() {
        let original = Block::Callout {
            kind: CalloutKind::Warning,
            fold: Some(Fold::Open),
            title: Some("Hey".to_string()),
            children: vec![Block::Paragraph(vec![Inline::Text("body".into())])],
        };
        let s = serde_json::to_string(&original).expect("serialize");
        let back: Block = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn inline_link_with_resolved_url_round_trips() {
        let original = Inline::Link {
            url: Url::resolved("../docs/", UrlKind::Wikilink),
            title: Some("Docs".to_string()),
            children: vec![Inline::Text("see".to_string())],
            is_wikilink: true,
        };
        let s = serde_json::to_string(&original).expect("serialize");
        let back: Inline = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn inline_link_is_wikilink_serde_defaults_to_false() {
        // When deserializing AST JSON authored before PR7a, missing
        // `is_wikilink` field must default to `false` (back-compat for
        // any serialized snapshots that pre-date the wikilink AST work).
        let json =
            r#"{"link":{"url":{"unresolved":"docs/"},"title":null,"children":[{"text":"Docs"}]}}"#;
        let back: Inline = serde_json::from_str(json).expect("deserialize");
        match back {
            Inline::Link { is_wikilink, .. } => assert!(!is_wikilink),
            _ => panic!("expected Link"),
        }
    }
}
