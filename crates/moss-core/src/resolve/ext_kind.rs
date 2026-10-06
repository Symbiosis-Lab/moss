//! Single source of truth mapping a file extension to its embed kind.
//! Pure; shared by build + editor. Replaces the duplication once flagged
//! between `wikilink_dispatch.rs`'s `synth_kind_for_ext` table and the
//! per-extension renderers it used to delegate to.

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

/// Whether `dest` points outside the site: a URL, a data URI, a `mailto:` or
/// an in-page `#anchor`.
fn is_external_dest(dest: &str) -> bool {
    ["http://", "https://", "//", "data:", "mailto:", "#"].iter().any(|p| dest.starts_with(p))
}

/// Whether a standard `![alt](dest)` image should be rendered by its file
/// kind (like `![[name]]`) instead of as an `<img>`: `dest` names a file in
/// the site (not `http:`, `https:`, `//`, `data:`, `mailto:` or an in-page
/// `#anchor`) and its extension is a typed embed other than an image.
pub fn is_typed_site_embed(dest: &str) -> bool {
    if is_external_dest(dest) {
        return false;
    }
    let ext = crate::path_ext::path_extension_lower(dest);
    matches!(
        reference_kind_for_ext(&ext),
        ExtKind::Video | ExtKind::Audio | ExtKind::Pdf | ExtKind::Iframe | ExtKind::Model
    )
}

/// Whether a standard `![alt](dest)` names a site target that `![[name]]`
/// transcludes instead of embedding as media: a page (`.md`), a table
/// (`.csv`, `.tsv`), a notebook (`.ipynb`) or a folder (a trailing `/`).
/// These are lowered to their markers before the markdown is parsed, in the
/// same pass that lowers the wiki spelling, so the two cannot drift.
pub fn is_transclusion_site_embed(dest: &str) -> bool {
    if is_external_dest(dest) {
        return false;
    }
    let path = dest.split(['?', '#']).next().unwrap_or(dest);
    path.ends_with('/')
        || matches!(
            reference_kind_for_ext(&crate::path_ext::path_extension_lower(dest)),
            ExtKind::Transclusion | ExtKind::Notebook | ExtKind::Table
        )
}

/// Whether `dest` is a web page the wiki spelling turns into a player:
/// an `http(s)` URL one of the known providers (YouTube, Vimeo, CodePen)
/// recognises. Any other URL, an image URL included, stays an `<img>` in the
/// standard spelling.
pub fn is_remote_player_embed(dest: &str) -> bool {
    (dest.starts_with("http://") || dest.starts_with("https://"))
        && !crate::render::url_embed::detect_provider(dest).provider_name.is_empty()
}

/// Whether `dest` is an `http(s)` URL of an image file that no provider
/// claims. Both spellings render it as `<img>`; a frame around an image file
/// shows nothing a browser would not show on its own.
pub fn is_remote_image_url(dest: &str) -> bool {
    (dest.starts_with("http://") || dest.starts_with("https://"))
        && !is_remote_player_embed(dest)
        && reference_kind_for_ext(&crate::path_ext::path_extension_lower(dest)) == ExtKind::Image
}

/// Whether the wikilink-embed dispatcher handles this image: every wiki embed
/// (`![[name]]`), and a standard `![alt](dest)` only when `dest` is a typed
/// site file ([`is_typed_site_embed`]), a page, table, notebook or folder
/// ([`is_transclusion_site_embed`]: the pre-pass lowers these in a page body,
/// but not inside a page that is itself embedded) or a provider URL
/// ([`is_remote_player_embed`]). Widening the standard form to more targets
/// means changing one of those.
pub(crate) fn dispatcher_takes_embed(is_wikilink: bool, dest: &str) -> bool {
    is_wikilink
        || is_typed_site_embed(dest)
        || is_remote_player_embed(dest)
        || is_transclusion_site_embed(dest)
}

/// Whether a lone image must stay a `Paragraph` so the dispatcher (which only
/// visits paragraphs) can replace it, instead of being promoted to an image
/// `Figure` at parse time.
///
/// This differs from [`dispatcher_takes_embed`] on one input: a wiki embed of
/// an image extension is taken by the dispatcher but promoted by the parser,
/// because the figure it would build there is the same one. An extension-less
/// wiki embed (`![[draft|55%]]`) stays a paragraph: only the dispatcher, with
/// the content graph, can say what it is.
pub(crate) fn embed_stays_paragraph(is_wikilink: bool, dest: &str) -> bool {
    let ext = crate::path_ext::path_extension_lower(dest);
    dispatcher_takes_embed(is_wikilink, dest)
        && !(is_wikilink && reference_kind_for_ext(&ext) == ExtKind::Image)
}

/// The diagnostic an *unresolvable* reference to this extension deserves.
///
/// Media the browser would have rendered in place leaves a visible hole when
/// the file is absent — which is what `deploy::refuse_publish` exists
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
    fn typed_site_embed_excludes_images_externals_and_unknown() {
        for d in ["a/clip.mp4", "song.MP3", "/x/paper.pdf", "w.html", "m.glb", "my%20clip.mp4", "clip.mp4?t=1"] {
            assert!(is_typed_site_embed(d), "{d}");
        }
        for d in [
            "photo.jpg", "photo.jpg?v=2", "n.md", "t.csv", "b.ipynb", "x.xyz", "noext", "gallery/",
            "https://e.com/clip.mp4", "http://e.com/a.pdf", "//e.com/a.mp3", "data:video/mp4;base64,AA",
            "mailto:a@b.c.pdf", "#a.mp4",
        ] {
            assert!(!is_typed_site_embed(d), "{d}");
        }
    }

    #[test]
    fn the_standard_form_transcludes_pages_tables_notebooks_and_folders() {
        for d in ["n.md", "a/b.MD", "t.csv", "t.tsv", "b.ipynb", "gallery/", "/journal/", "n.md#Heading", "n%20x.md", "gallery/?x=1"] {
            assert!(is_transclusion_site_embed(d), "{d}");
        }
        for d in ["photo.jpg", "clip.mp4", "noext", "x.xyz", "https://e.com/n.md", "//e.com/g/", "data:text/csv,a", "#n.md", ""] {
            assert!(!is_transclusion_site_embed(d), "{d}");
        }
    }

    #[test]
    fn a_provider_url_is_a_player_and_an_image_url_is_an_image() {
        for d in ["https://www.youtube.com/watch?v=dQw4w9WgXcQ", "http://youtu.be/dQw4w9WgXcQ", "https://vimeo.com/76979871"] {
            assert!(is_remote_player_embed(d), "{d}");
            assert!(!is_remote_image_url(d), "{d}");
        }
        for d in ["https://e.com/a.jpg", "http://e.com/a.PNG?w=2"] {
            assert!(is_remote_image_url(d), "{d}");
            assert!(!is_remote_player_embed(d), "{d}");
        }
        for d in ["https://e.com/page", "https://www.youtube.com/about", "media/clip.mp4", "a.jpg"] {
            assert!(!is_remote_player_embed(d) && !is_remote_image_url(d), "{d}");
        }
    }

    #[test]
    fn the_dispatcher_and_the_parser_ask_two_related_questions() {
        // (is_wikilink, dest, dispatcher takes it, parser keeps it a paragraph)
        for (wiki, dest, takes, stays) in [
            (true, "photo.jpg", true, false),
            (true, "clip.mp4", true, true),
            (true, "draft", true, true),
            (false, "photo.jpg", false, false),
            (false, "clip.mp4", true, true),
            (false, "https://e.com/clip.mp4", false, false),
            (false, "https://youtu.be/dQw4w9WgXcQ", true, true),
            (false, "https://e.com/a.jpg", false, false),
            (false, "n.md", true, true),
        ] {
            assert_eq!(dispatcher_takes_embed(wiki, dest), takes, "takes {wiki} {dest}");
            assert_eq!(embed_stays_paragraph(wiki, dest), stays, "stays {wiki} {dest}");
        }
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
