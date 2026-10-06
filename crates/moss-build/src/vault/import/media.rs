//! Media URL conventions shared across the internet, not per-builder:
//! CDN transform queries hiding originals, provider thumbnails standing in
//! for embeds, and file-carrying viewer URLs (Google Drive previews).

use url::Url;

/// `Accept` header for a media/asset download — asks for the file as
/// uploaded, not a format the server might substitute for it.
///
/// ureq's own default is `Accept: */*` (sent whenever a request sets no
/// `Accept` of its own), and on at least one real CDN that default is enough
/// on its own to trigger auto-transcoding: a Squarespace image request with
/// `Accept: */*` comes back `image/webp` even with `?format=original` on the
/// URL (see `original_media_url`'s doc comment — the query never controlled
/// this), while the same request with no `Accept` header, or one that
/// doesn't mention WebP/AVIF, gets the real uploaded JPEG back. Verified
/// against the live CDN that `;q=0` does NOT suppress this — `image/webp;q=0`
/// alone still comes back as WebP, so the fix is to never name `image/webp`
/// or `image/avif` in the header at all, not to down-rank them.
///
/// `image/*` asks for any image format by name (satisfying that CDN's
/// negotiation without mentioning the two it auto-converts to), and the
/// trailing low-priority `*/*` is the same permissive fallback a real
/// browser sends, so a non-negotiating host (plain file server, a CDN that
/// ignores `Accept` entirely) serves exactly what it already serves any
/// other client — verified byte-identical against a Wikimedia upload URL, a
/// WordPress-hosted upload, and a Jetpack Photon URL with and without this
/// header.
pub(crate) const MEDIA_ACCEPT: &str = "image/*,*/*;q=0.1";

/// Query keys that are image-transform hints (resize/crop/quality), shared
/// by rmcdn, imgix, wsrv and countless custom CDNs.
const TRANSFORM_KEYS: &[&str] = &[
    "w", "h", "q", "fit", "crop", "cx", "cy", "cw", "ch", "dpr", "fm", "auto",
];

/// Strip a transform query to request the original asset. Only fires when
/// EVERY query key is a known transform hint — a signed or content-bearing
/// query must never be mangled. `None` = leave the URL alone.
///
/// Residual: the stripped URL is committed to markdown before its download
/// is proven; a CDN that 404s the query-less form would leave a broken
/// remote reference. Not observed on any transform CDN so far.
pub(crate) fn strip_query_transform(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let mut keys = parsed.query_pairs().peekable();
    keys.peek()?;
    if !keys.all(|(k, _)| TRANSFORM_KEYS.contains(&k.to_ascii_lowercase().as_str())) {
        return None;
    }
    let mut stripped = parsed;
    stripped.set_query(None);
    Some(stripped.to_string())
}

/// One row of the "original media" family: a platform CDN whose bare (or
/// size-hinted) URLs cap resolution, and the query that asks it for the
/// largest rendition it serves. Keyed on the CDN host, not a site — the
/// same host backs unrelated sites on this platform.
struct OriginalMediaRule {
    host: &'static str,
    query: &'static str,
}

/// Squarespace's image CDN (current host `images.squarespace-cdn.com`;
/// `static1.squarespace.com` for older assets) doesn't take free-form
/// transform queries — its own `format` key accepts only `100w`…`2500w` or
/// `original`, and its docs list no bucket past 2500px: a bare URL and
/// `format=2500w` both return the same 2500px-capped rendition, and
/// `format=original` is not documented as exceeding that cap either.
/// Verified against a live corpus image, it doesn't: same 2500px width.
///
/// It does NOT, on its own, avoid the CDN's WebP transcoding — that is
/// decided by the request's `Accept` header, not the query (see
/// [`MEDIA_ACCEPT`]; a prior version of this comment claimed otherwise and
/// was wrong, because it was tested only with the download path's actual
/// `Accept` header, which was the real cause). This rule's real, verified
/// payoff is collapsing every query variant of one photo (bare,
/// `?format=750w`, `?format=2500w`, …) onto a single canonical URL, which is
/// what lets the asset pass dedupe by URL and download once instead of once
/// per variant referenced on the page.
const ORIGINAL_MEDIA_RULES: &[OriginalMediaRule] = &[
    OriginalMediaRule {
        host: "images.squarespace-cdn.com",
        query: "format=original",
    },
    OriginalMediaRule {
        host: "static1.squarespace.com",
        query: "format=original",
    },
];

/// Rewrite a URL through the original-media table. `None` when the host has
/// no row (leave the URL alone) or it is already in the row's canonical
/// form (nothing to rewrite).
pub(crate) fn original_media_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let rule = ORIGINAL_MEDIA_RULES.iter().find(|r| r.host == host)?;
    if parsed.query() == Some(rule.query) {
        return None;
    }
    let mut rewritten = parsed;
    rewritten.set_query(Some(rule.query));
    Some(rewritten.to_string())
}

/// Recover a YouTube watch URL from a lightbox thumbnail
/// (`i.ytimg.com/vi/{id}/…jpg`). Lightbox players are created by site JS,
/// so the thumbnail is often the only trace of the video in static HTML.
pub(crate) fn youtube_watch_from_thumb(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.host_str() != Some("i.ytimg.com") {
        return None;
    }
    let mut segs = parsed.path_segments()?;
    if segs.next() != Some("vi") {
        return None;
    }
    let id = segs.next()?;
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| format!("https://www.youtube.com/watch?v={id}"))
}

/// Turn a Google Drive preview/view iframe URL into the direct-download
/// form (`uc?export=download&id=…`), which publicly serves the file bytes
/// for link-shared files. The asset pass then localizes it into the vault.
pub(crate) fn drive_download_from_preview(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.host_str() != Some("drive.google.com") {
        return None;
    }
    let segs: Vec<_> = parsed.path_segments()?.collect();
    match segs.as_slice() {
        ["file", "d", id, rest] if matches!(*rest, "preview" | "view") && !id.is_empty() => {
            Some(format!("https://drive.google.com/uc?export=download&id={id}"))
        }
        _ => None,
    }
}

/// PDF file extensions — named so [`is_localizable_file_url`] and the
/// crawl's own pre-fetch non-page check (`scrape::crawler::is_non_page_file_url`)
/// share one list instead of each carrying its own copy of `"pdf"`.
pub(crate) const PDF_EXTENSIONS: &[&str] = &["pdf"];

/// Audio file extensions — same reasoning as [`PDF_EXTENSIONS`]. Kept to the
/// four this module already recognized rather than grown with this split, so
/// [`is_localizable_file_url`]'s behavior is unchanged.
pub(crate) const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "wav", "ogg"];

/// Whether a remote URL names a downloadable FILE that the import should
/// localize into the vault (as opposed to a provider embed, which stays
/// remote). Drive direct-downloads and bare file-extension URLs qualify.
pub(crate) fn is_localizable_file_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.host_str() == Some("drive.google.com")
        && parsed.path() == "/uc"
        && parsed
            .query_pairs()
            .any(|(k, v)| k == "export" && v == "download")
    {
        return true;
    }
    let path = parsed.path().to_ascii_lowercase();
    PDF_EXTENSIONS
        .iter()
        .chain(AUDIO_EXTENSIONS)
        .any(|ext| path.ends_with(&format!(".{ext}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_pure_transform_queries() {
        assert_eq!(
            strip_query_transform("https://i-p.rmcdn.net/a/b/image-x.jpg?w=960").as_deref(),
            Some("https://i-p.rmcdn.net/a/b/image-x.jpg")
        );
        assert_eq!(
            strip_query_transform("https://i-p.rmcdn.net/a/b/i.jpg?w=644&cX=1&cW=500").as_deref(),
            Some("https://i-p.rmcdn.net/a/b/i.jpg")
        );
    }

    #[test]
    fn leaves_signed_or_plain_urls_alone() {
        // Non-transform key present → never strip (could be a signature).
        assert_eq!(
            strip_query_transform("https://cdn.example.com/i.jpg?w=960&token=abc"),
            None
        );
        assert_eq!(strip_query_transform("https://cdn.example.com/i.jpg"), None);
    }

    #[test]
    fn squarespace_bare_and_sized_urls_both_rewrite_to_original() {
        assert_eq!(
            original_media_url(
                "https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg"
            )
            .as_deref(),
            Some("https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=original")
        );
        assert_eq!(
            original_media_url(
                "https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=2500w"
            )
            .as_deref(),
            Some("https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=original")
        );
        // Already canonical → nothing to rewrite.
        assert_eq!(
            original_media_url(
                "https://images.squarespace-cdn.com/content/v1/abc/def/photo.jpg?format=original"
            ),
            None
        );
    }

    #[test]
    fn squarespace_static1_host_also_rewrites() {
        assert_eq!(
            original_media_url("https://static1.squarespace.com/static/abc/t/def/1234/photo.jpg?format=750w")
                .as_deref(),
            Some("https://static1.squarespace.com/static/abc/t/def/1234/photo.jpg?format=original")
        );
    }

    #[test]
    fn original_media_leaves_other_hosts_alone() {
        assert_eq!(
            original_media_url("https://cdn.example.com/photo.jpg?format=2500w"),
            None
        );
        assert_eq!(original_media_url("https://cdn.example.com/photo.jpg"), None);
    }

    #[test]
    fn recovers_youtube_watch_from_thumbnail() {
        assert_eq!(
            youtube_watch_from_thumb("https://i.ytimg.com/vi/sPuJnLOmIJo/maxresdefault.jpg")
                .as_deref(),
            Some("https://www.youtube.com/watch?v=sPuJnLOmIJo")
        );
        assert_eq!(
            youtube_watch_from_thumb("https://i.ytimg.com/other/x.jpg"),
            None
        );
        assert_eq!(
            youtube_watch_from_thumb("https://example.com/vi/abc/maxresdefault.jpg"),
            None
        );
    }

    #[test]
    fn converts_drive_preview_to_direct_download() {
        assert_eq!(
            drive_download_from_preview("https://drive.google.com/file/d/1IL9m6ov/preview")
                .as_deref(),
            Some("https://drive.google.com/uc?export=download&id=1IL9m6ov")
        );
        assert_eq!(
            drive_download_from_preview("https://drive.google.com/drive/folders/xyz"),
            None
        );
    }

    #[test]
    fn localizable_covers_drive_downloads_and_file_extensions_not_providers() {
        assert!(is_localizable_file_url(
            "https://drive.google.com/uc?export=download&id=abc"
        ));
        assert!(is_localizable_file_url("https://example.com/audio/song.mp3"));
        assert!(!is_localizable_file_url(
            "https://www.youtube.com/watch?v=abc123"
        ));
        assert!(!is_localizable_file_url("https://example.com/page.html"));
    }
}
