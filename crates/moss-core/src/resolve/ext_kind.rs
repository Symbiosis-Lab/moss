//! Single source of truth mapping a file extension to its embed kind.
//! Pure; shared by build + editor. Replaces the duplication flagged at
//! `wikilink_dispatch.rs` (synth_kind_for_ext tables vs EmbedRenderer::extensions()).

use serde::{Deserialize, Serialize};

/// The render family a non-folder, non-link target belongs to, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ExtKind {
    Image,
    Iframe,
    Pdf,
    Video,
    Audio,
    Model,
    Transclusion, // .md / .markdown
    Notebook,     // .ipynb
    Table,        // .csv / .tsv
    Other,        // unknown extension → caller treats as a Link
}

/// Classify a lowercase extension (no leading dot). Unknown → `Other`.
/// Derives from the asset registry SSOT so ext_kind and the registry never drift.
pub fn reference_kind_for_ext(ext: &str) -> ExtKind {
    crate::resolve::asset_registry::asset_info(ext)
        .map(|a| a.kind)
        .unwrap_or(ExtKind::Other)
}

/// The diagnostic an *unresolvable* reference to this extension deserves.
///
/// Media the browser would have rendered in place leaves a visible hole when
/// the file is absent — which is what `missing_media::refuse_publish` exists
/// to catch, so it blocks. A markdown transclusion or an unknown extension
/// degrades to an ordinary link instead, so it stays advisory.
pub fn missing_reference_kind(ext: Option<&str>) -> crate::resolve::DiagnosticKind {
    use crate::resolve::DiagnosticKind;
    match ext.map(reference_kind_for_ext) {
        Some(ExtKind::Transclusion | ExtKind::Other) | None => DiagnosticKind::Other,
        Some(_) => DiagnosticKind::MissingAsset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_each_family() {
        assert_eq!(reference_kind_for_ext("png"), ExtKind::Image);
        assert_eq!(reference_kind_for_ext("html"), ExtKind::Iframe);
        assert_eq!(reference_kind_for_ext("pdf"), ExtKind::Pdf);
        assert_eq!(reference_kind_for_ext("mp4"), ExtKind::Video);
        assert_eq!(reference_kind_for_ext("mp3"), ExtKind::Audio);
        assert_eq!(reference_kind_for_ext("glb"), ExtKind::Model);
        assert_eq!(reference_kind_for_ext("md"), ExtKind::Transclusion);
        assert_eq!(reference_kind_for_ext("ipynb"), ExtKind::Notebook);
        assert_eq!(reference_kind_for_ext("csv"), ExtKind::Table);
        assert_eq!(reference_kind_for_ext("xyz"), ExtKind::Other);
    }

    #[test]
    fn only_media_blocks_a_publish_when_it_is_missing() {
        use crate::resolve::DiagnosticKind;
        for ext in ["png", "mp4", "m4a", "pdf", "glb", "html", "ipynb", "csv"] {
            assert_eq!(
                missing_reference_kind(Some(ext)),
                DiagnosticKind::MissingAsset,
                "a missing .{ext} is a hole in the page"
            );
        }
        for ext in [Some("md"), Some("xyz"), None] {
            assert_eq!(missing_reference_kind(ext), DiagnosticKind::Other, "{ext:?}");
        }
    }

    #[test]
    fn kind_matches_registry_for_all_known_exts() {
        use crate::resolve::asset_registry::all_assets;
        for a in all_assets() {
            assert_eq!(reference_kind_for_ext(a.ext), a.kind, "kind mismatch for {}", a.ext);
        }
        assert_eq!(reference_kind_for_ext("avif"), ExtKind::Image); // was Other before
    }
}
