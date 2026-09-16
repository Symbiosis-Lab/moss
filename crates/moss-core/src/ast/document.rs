//! Top-level parsed document.

use serde::{Deserialize, Serialize};

use super::node::Block;

/// Per-top-level-block metadata.
///
/// Holds parse-time annotations that don't belong on the `Block` enum
/// itself (which is shape-only). Lives in a parallel `Vec<BlockMeta>` on
/// [`Document`] so existing pattern matches over `Block` don't need to
/// unwrap a meta wrapper.
///
/// Today the only field is `source_line` (set by the parser when
/// [`crate::ast::ParseConfig::emit_source_lines`] is true). Additional
/// per-block annotations (block IDs, custom attrs, etc.) land here too
/// when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BlockMeta {
    /// 1-based source line where this block begins, or `None` when source
    /// tracking is off (or the block was synthesized — e.g. shortcode
    /// substitution — and has no faithful source position).
    ///
    /// Consumed by the renderer to emit `data-source-line="N"` on the
    /// opening tag, which the preview's `cm-scroll-sync` consumes for
    /// paragraph-level editor↔preview scroll sync.
    pub source_line: Option<usize>,
}

/// A parsed markdown document body.
///
/// Wraps `Vec<Block>` with document-level flags. The frontmatter sits in
/// [`crate::frontmatter::ParsedDocument`]; this struct is body-only.
/// Higher-level code combines the two as needed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Document {
    /// Body content as a flat list of block-level nodes.
    pub blocks: Vec<Block>,
    /// Parallel-to-`blocks` metadata. **Invariant:** `block_meta.len() ==
    /// blocks.len()`. The renderer asserts this in debug builds and
    /// gracefully degrades (treats missing entries as `BlockMeta::default()`)
    /// in release builds.
    ///
    /// Defaults to an all-`BlockMeta::default()` vec sized to match
    /// `blocks` when constructed via [`Document::from_blocks`]; only the
    /// parser populates `source_line` (under
    /// [`crate::ast::ParseConfig::emit_source_lines`]).
    #[serde(default)]
    pub block_meta: Vec<BlockMeta>,
    /// True if this document is a slot file (e.g. `footer.md`) — its
    /// content fills a layout slot rather than being rendered as an
    /// article. The renderer suppresses auto-injected article chrome
    /// (`<h1 class="moss-article-title">`) when this is set.
    ///
    /// Replaces the `heading: false` YAML synthesis hack at
    /// `src-tauri/src/build/footer.rs:160`. Lands in Phase A.5 of the
    /// typed-AST migration; in Phase A it's available but not yet
    /// consumed by src-tauri.
    pub slot_only: bool,
    /// Non-fatal problems found while parsing this body — today, every
    /// `:::name` fence whose name is not a registered shortcode.
    ///
    /// The extractor has collected these since the unknown-name fallback
    /// landed, but nothing carried them out of the parse, so the only trace
    /// a misspelling left was a `moss-unknown-shortcode` div in the built
    /// HTML. An author (or an agent) got a page that had silently lost its
    /// gallery and a build that said nothing. This field is that missing
    /// hop; `build::markdown::pipeline` prints each entry against the file.
    ///
    /// Scope, precisely: top-level fences, fences nested inside an
    /// *unknown* fence (that branch recurses through `extract_with_state`,
    /// threading one collection), AND fences nested inside a *valid*
    /// shortcode's body — grid cells (`parse_cell_to_blocks`) and the hero
    /// overlay (`parse_overlay_to_blocks`) each re-parse their body as an
    /// independent fragment via `parse_fragment_with_config`; both now
    /// return that fragment `Document`'s `warnings` alongside its blocks,
    /// and `parse_shortcode_block` merges them into the `Vec<String>` it
    /// already threads into `extract_with_state`'s shared collection. So a
    /// misspelling inside `:::grid` or `:::hero` warns exactly like a
    /// top-level one; see `unknown_shortcode_inside_a_valid_shortcode_warns`
    /// and `unknown_shortcode_inside_a_hero_overlay_warns`.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Document {
    /// Construct an empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a document from a list of blocks. All `block_meta` entries
    /// are default (no source-line tracking). Use
    /// [`Document::from_blocks_with_meta`] when meta is known.
    pub fn from_blocks(blocks: Vec<Block>) -> Self {
        let block_meta = vec![BlockMeta::default(); blocks.len()];
        Self {
            blocks,
            block_meta,
            slot_only: false,
            warnings: Vec::new(),
        }
    }

    /// Construct a document from blocks + parallel meta. Panics in debug if
    /// the two slices have different lengths.
    pub fn from_blocks_with_meta(blocks: Vec<Block>, block_meta: Vec<BlockMeta>) -> Self {
        debug_assert_eq!(
            blocks.len(),
            block_meta.len(),
            "Document::from_blocks_with_meta: blocks and block_meta must be equal length"
        );
        Self {
            blocks,
            block_meta,
            slot_only: false,
            warnings: Vec::new(),
        }
    }

    /// True if any top-level block is a shortcode of the given kind.
    ///
    /// Shallow check: does NOT descend into nested blocks (e.g. a
    /// `:::subscribe` inside a `:::grid` cell would not be found by this
    /// query alone). Phase A models the shallow case; deeper
    /// `has_shortcode_recursive` lands when nested-shortcode AST queries
    /// are first needed.
    pub fn has_shortcode(&self, kind: super::shortcode::ShortcodeKind) -> bool {
        self.blocks.iter().any(|b| match b {
            Block::Shortcode(sc) => sc.kind() == kind,
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::node::Inline;
    use super::*;

    #[test]
    fn empty_document_has_no_blocks() {
        let d = Document::new();
        assert!(d.blocks.is_empty());
        assert!(!d.slot_only);
    }

    #[test]
    fn from_blocks_constructs_with_blocks() {
        let d = Document::from_blocks(vec![Block::ThematicBreak]);
        assert_eq!(d.blocks.len(), 1);
        assert!(!d.slot_only);
    }

    #[test]
    fn slot_only_default_is_false() {
        // Crucial: an article doc must NOT silently become a slot.
        let d = Document::default();
        assert!(!d.slot_only);
    }

    #[test]
    fn slot_only_settable() {
        let mut d = Document::new();
        d.slot_only = true;
        assert!(d.slot_only);
    }

    #[test]
    fn document_round_trips_through_serde() {
        let mut original = Document::new();
        original.blocks.push(Block::Paragraph(vec![Inline::Text(
            "hello".to_string(),
        )]));
        original.slot_only = true;
        original.warnings.push("unknown shortcode `:::nope`".to_string());
        let s = serde_json::to_string(&original).expect("serialize");
        let back: Document = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn has_shortcode_returns_false_when_empty() {
        // The Shortcode enum is empty in Phase A; this test exercises the
        // query path on the empty case (which is the only constructable
        // case until Phase B). Extended per-kind tests land alongside
        // each shortcode migration.
        let d = Document::new();
        assert!(!d.has_shortcode(super::super::shortcode::ShortcodeKind::Subscribe));
    }
}
