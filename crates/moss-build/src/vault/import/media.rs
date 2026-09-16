//! Media URL conventions shared across the internet, not per-builder:
//! CDN transform queries hiding originals, provider thumbnails standing in
//! for embeds, and file-carrying viewer URLs (Google Drive previews).

use url::Url;

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
    ["mp3", "m4a", "wav", "ogg", "pdf"]
        .iter()
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
