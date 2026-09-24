//! Extensible registry for embed renderers.
//!
//! Plugins register at pipeline init via [`RendererRegistryBuilder::with_boxed`].
//! moss-core shipped no built-in `EmbedRenderer` here as of the dead-code
//! sweep that removed the last one (see "No more built-ins," below) — every
//! call site now starts from [`RendererRegistry::empty`]. Pipelines with
//! plugin renderers build their own registry and thread it through
//! [`super::wikilink_dispatch::dispatch_wikilink_embed_with_registry`]
//! (Phase 3 PR2 retired the older Stage 1 `resolve_wikilinks_with_registry`
//! string-rewriter that consumed this registry).
//!
//! # Two-pass dispatch (plugin renderers)
//!
//! Plugin renderers are declared in a plugin's manifest (a `script` path
//! pointing at a JS function) but moss-core can't execute JavaScript. The
//! pipeline handles this with a two-pass design:
//!
//! ```text
//! Pass 1 (moss-core, pure):
//!   ![[diagram.dot]]
//!     → PluginEmbedRenderer::render(&parsed)     (desktop-app adapter)
//!     → RenderedEmbed::Deferred { marker: "<!-- moss-embed-plugin-graphviz:... -->" }
//!     → marker spliced into content
//!
//! Pass 2 (the desktop app, async + I/O):
//!   resolve_embeds_with_handlers scans for marker prefix
//!     → MarkerHandlers registry dispatches to plugin IPC
//!     → plugin script runs `dot -Tsvg`
//!     → returned HTML spliced back into content
//! ```
//!
//! The first pass stays pure; the second pass does I/O. Plugin author
//! writes one JS function; they never touch Rust.
//!
//! # No more built-ins — what actually stops a plugin shadowing a core kind
//!
//! This registry used to seed itself with the built-in `EmbedRenderer`
//! impls first specifically so a plugin claiming a colliding extension
//! (e.g. `.html`) couldn't shadow one — first-match-wins lookup meant the
//! built-in, registered first, always won. That mechanism was **removed**,
//! not merely unused: the last built-in `EmbedRenderer` struct was deleted
//! as dead code (each one's `render()` was unreachable — the live pipeline
//! never dispatched an embed through this registry to reach it), and
//! `RendererRegistry::builtin()`/`RendererRegistryBuilder::with_builtins`
//! were deleted with it. `empty()` is the only constructor left.
//!
//! A plugin still can't shadow a core embed kind — three other, earlier
//! mechanisms claim those extensions before this registry's `lookup(ext)`
//! fallback is ever reached:
//!
//! 1. `resolve.rs`'s pre-pass (`lower_transclusion_and_folder_wikilinks`)
//!    claims folder / `.md` / `.ipynb` / `.csv` / `.tsv` before
//!    pulldown-cmark ever parses the document.
//! 2. `wikilink_dispatch.rs`'s `synth_kind_for_ext` claims image / video /
//!    pdf / audio / 3D extensions and routes straight to a synthesizer.
//! 3. The dispatcher's separate `IMAGE_EXTENSIONS` branch claims image
//!    extensions ahead of everything else.
//!
//! # When to use which API
//!
//! | Pipeline type | Function | Registry used |
//! |---|---|---|
//! | No plugins | [`super::wikilink_dispatch::dispatch_wikilink_embed`] | empty (via `lookup_renderer`) |
//! | With plugins | [`super::wikilink_dispatch::dispatch_wikilink_embed_with_registry`] | custom registry built at init |

use super::embed_renderer::EmbedRenderer;

/// A registry of embed renderers, built from any custom renderers (typically
/// plugin adapters) added at construction time — see the module doc's
/// "No more built-ins" section for why there is no built-in set to seed it
/// with any more.
///
/// Built in one pass at pipeline init and then treated as immutable. Lookup
/// is first-match-wins by extension.
pub struct RendererRegistry {
    renderers: Vec<&'static dyn EmbedRenderer>,
}

impl RendererRegistry {
    /// Empty builder.
    pub fn empty() -> RendererRegistryBuilder {
        RendererRegistryBuilder::new()
    }

    /// Look up a renderer by extension (case-insensitive, no leading dot).
    pub fn lookup(&self, ext: &str) -> Option<&'static dyn EmbedRenderer> {
        if ext.is_empty() {
            return None;
        }
        self.renderers
            .iter()
            .copied()
            .find(|r| r.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }
}

/// Builder for [`RendererRegistry`].
pub struct RendererRegistryBuilder {
    renderers: Vec<&'static dyn EmbedRenderer>,
}

impl RendererRegistryBuilder {
    fn new() -> Self {
        Self {
            renderers: Vec::new(),
        }
    }

    /// Add a `'static` renderer (zero-size unit struct).
    pub fn with_static(mut self, r: &'static dyn EmbedRenderer) -> Self {
        self.renderers.push(r);
        self
    }

    /// Add a heap-allocated renderer (e.g., a plugin adapter).
    ///
    /// Leaks the `Box` to produce a `&'static dyn` reference. Acceptable
    /// because plugin registration happens at pipeline init (one-time, not
    /// hot path); one leak per plugin renderer is negligible.
    pub fn with_boxed(mut self, r: Box<dyn EmbedRenderer>) -> Self {
        let leaked: &'static dyn EmbedRenderer = Box::leak(r);
        self.renderers.push(leaked);
        self
    }

    /// Finalize the registry.
    pub fn build(self) -> RendererRegistry {
        RendererRegistry {
            renderers: self.renderers,
        }
    }
}

impl Default for RendererRegistry {
    fn default() -> Self {
        RendererRegistry::empty().build()
    }
}

#[cfg(test)]
mod tests {
    use super::super::embed_renderer::{ParsedEmbed, RenderedEmbed};
    use super::*;

    #[derive(Debug)]
    struct CustomRenderer;
    impl EmbedRenderer for CustomRenderer {
        fn extensions(&self) -> &[&'static str] {
            &["xyz"]
        }
        fn render(&self, _: &ParsedEmbed<'_>) -> RenderedEmbed {
            RenderedEmbed::Inline("custom".to_string())
        }
    }

    #[test]
    fn test_builder_adds_custom_renderer() {
        let reg = RendererRegistry::empty()
            .with_boxed(Box::new(CustomRenderer))
            .build();
        assert!(reg.lookup("xyz").is_some());
    }

    #[test]
    fn test_empty_registry() {
        let reg = RendererRegistry::empty().build();
        assert!(reg.lookup("jpg").is_none());
    }

    #[test]
    fn test_lookup_case_insensitive() {
        // No built-in extension exists to test this against (the registry
        // has none left; see the module doc), so exercise the still-live
        // mechanism — RendererRegistry::lookup's case folding — through a
        // boxed (plugin-shaped) renderer instead.
        let reg = RendererRegistry::empty()
            .with_boxed(Box::new(CustomRenderer))
            .build();
        assert!(reg.lookup("XYZ").is_some());
        assert!(reg.lookup("Xyz").is_some());
    }
}
