//! Normalized content roles shared by all import producers.
//!
//! The import IR: builder-specific trees (Strikingly `$S` sections, Readymag
//! widgets, ...) map into this small vocabulary of roles; one emitter
//! (`super::emit`) renders them to markdown. Namespace-distinct from
//! `moss_core::ast::Block` — this is import-side IR, not the render AST.
//!
//! Variants exist only when a producer emits them (consumer-before-ship).

/// One normalized content block, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// Prose as HTML (already entity-unescaped) — emitted via htmd.
    RichText {
        html: String,
    },
    /// Quoted prose as HTML — emitted via htmd, then `> `-prefixed.
    Quote {
        html: String,
    },
    /// Image reference; `alt` is raw (emitter sanitizes for markdown).
    Image {
        src: String,
        alt: String,
    },
    /// External video URL — emitted as a `![[url]]` moss embed
    /// (provider-aware players via moss-core `url_embed`).
    Video {
        url: String,
    },
    /// Audio file — emitted as a `![[src]]` moss embed (`<audio controls>`
    /// after the asset pass localizes the file into the vault).
    Audio {
        src: String,
    },
    Button {
        text: String,
        url: String,
    },
    Separator,
}
