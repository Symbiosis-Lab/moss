//! Cover image resolution chain for og:image / twitter:image.
//!
//! The page's own picture, most explicitly declared first, or nothing. Order
//! of precedence (first non-empty wins):
//!
//! 1. Page frontmatter `cover:`
//! 2. Filename convention: a file in the page's bundle directory whose stem
//!    contains "feature", "cover", or "thumbnail" (Hugo-style zero-config).
//! 3. The page's own hero image — its `:::hero` block's `image=` attribute or
//!    inline image, typed as `ParsedDocument.hero_image_url`.
//! 4. First markdown-origin body image (typed `ParsedDocument.body_cover_path`,
//!    captured at parse time by `transform_events`).
//!
//! A page with none of these gets the generated title card (the caller's
//! fallback in html.rs, which needs output_dir and an og sink) — never the
//! home page's picture. Until 2026-09-10 a site-wide tail followed rung 4: the
//! HOME page's `cover:`, hero and body image. It read the home page's own
//! picture as the site's brand, and on a site whose home page is a portrait
//! every text-only letter shared with that portrait. The tail's motivating
//! case (`docs/archive/2026-05-16-homepage-hero-og-fallback-design.md`, a hero
//! banner standing in for the site) was an inference from the home page's
//! picture, not a declared site image, and it is the client quote card's rule
//! that holds here too: `data-share-cover` is the page's own picture or absent.
//! `meta.rs`'s description chain keeps its homepage tail — the site's tagline
//! on a page with no lede is not wrong the way the site's picture is.
//!
//! Rungs 3 and 4 changed rank on 2026-09-07. A page's hero was consulted only
//! when the page WAS the home page, so an article whose one image was its hero
//! shared as a near-blank generated card — 21 of the 76 pages on one site.
//!
//! # Architectural status (post-Step-5, 2026-05-17)
//!
//! The body-image rung previously regex-scraped rendered HTML for `<img src>` /
//! `<video poster>` via `first_body_image`. Step 5 of the
//! structural-html-emission migration replaced that with a typed
//! capture during pulldown-cmark event iteration (see
//! `transform_events` in `build/markdown/pipeline.rs`). The regex is
//! retired.
//!
//! What the body-image rung no longer captures (intentional narrowing — all classes
//! are the same "non-`Tag::Image` source" exclusion):
//!
//! - Raw HTML `<img>` literally embedded in markdown source — pulldown-
//!   cmark emits these as `Event::Html` opaque pass-through; documented
//!   bare-img carve-out (`image_render.rs:56-93`).
//! - `<video poster="...">` — not a markdown construct in the first place.
//! - Shortcode-emitted images (`:::hero`, link-preview favicons, gallery
//!   thumbnails) — never were content covers in spirit; `:::hero` has its
//!   own rungs; favicons and thumbnails were footguns under
//!   the old regex (a link-preview favicon happening to come first in the
//!   body would 16×16-pixel the share card).
//! - Homepage feed `<img>` thumbnails — homepages splice folder-card /
//!   article-list HTML into `homepage_content` after `doc.html_content`
//!   is built. The old regex saw those splices; the new typed capture
//!   sees only the homepage's own markdown body. A homepage with no
//!   author-written images falls through to the auto-OG-card rather
//!   than picking up a feed thumbnail.
//!
//! Users wanting any of the above as cover should set frontmatter
//! `cover:` or use the `:::hero` cascade. See
//! `docs/reference/structural-html-emission.md`.

use crate::build::served_path::ServedPath;
use crate::build::page::meta::CoverRef;
use std::path::Path;

/// Long-edge floor for the raster this chain resolves to.
///
/// Every rung above yields a URL that `og:image` / `twitter:image` emit
/// verbatim, and for a local raster that URL is served by the re-encoded,
/// size-capped deploy raster `copy_deferred_assets` lands at the source's own
/// filename — never the author's original bytes. moss probes no cover, so it
/// emits `og:image:width`/`:height` only for the auto-generated 1200×630 card
/// and enforces no dimension floor of its own. The floor therefore lives
/// entirely in the cap that sizes that raster, and this is where it is
/// written down.
///
/// The external constraint: X/Twitter and Facebook both reject a card image
/// below 200×200, and `summary_large_image` wants at least 300×157. The encoder
/// caps the LONGEST edge and never upscales, so a cover clears 300px on its
/// short edge whenever `cap / aspect_ratio >= 300`. At 600 a 2:1 cover still
/// yields 600×300 — the widest ordinary cover shape. Anything more extreme than
/// ~2:1 is already under the large-card minimum at today's cap; that is an
/// aspect-ratio limit, not a cap one.
///
/// The guard belongs on whatever number sizes the bytes behind `og:image`, not
/// on any particular name. That is `FALLBACK_MAX_EDGE` (1200, a literal
/// independent of the ladder since moss#976 B1) and NOT `DEPLOY_MAX_EDGE`
/// (2400): a local cover resolves to the deployed raster original, which is
/// the `<picture>` fallback, and the fallback is what that constant sizes.
const OG_IMAGE_LONG_EDGE_FLOOR: u32 = 600;
const _: () = assert!(
    crate::build::media::fallback_raster::FALLBACK_MAX_EDGE >= OG_IMAGE_LONG_EDGE_FLOOR,
    "the raster og:image fallback's long-edge cap dropped below the floor at \
     which a 2:1 cover still clears the 300x157 social-card minimum"
);

/// Inputs to the cover-image resolver.
pub struct CoverChainInputs<'a> {
    /// Already-resolved relative URL or external URL from page frontmatter.
    pub page_cover: Option<&'a str>,
    /// THIS page's hero image, captured at hoisting from the typed
    /// `HeroShortcode.image` (post-resolution). `None` when the page has no
    /// hero or its hero had no image.
    pub page_hero_image_url: Option<&'a str>,
    /// First markdown-origin body image src, captured at parse time
    /// (`ParsedDocument.body_cover_path`). Already passed through
    /// `resolve_link` so it's a relative or external URL ready to use.
    /// Step-5 replacement for the legacy regex scrape of `body_html`.
    pub body_cover_path: Option<&'a str>,
    /// The page's bundle directory (absolute path), for filename-convention rung.
    pub bundle_dir: Option<&'a Path>,
    /// The site root (absolute path), used to compute relative paths for ServedPath.
    pub source_root: &'a Path,
}

/// Resolve the cover image by walking the fallback chain.
/// Returns `None` when all rungs are empty (caller falls through to auto-card).
pub fn resolve_cover_chain(inputs: &CoverChainInputs) -> Option<CoverRef> {
    cover_from_frontmatter(inputs.page_cover)
        .or_else(|| {
            cover_from_filename_convention(inputs.bundle_dir, inputs.source_root)
                .map(CoverRef::Local)
        })
        .or_else(|| cover_from_frontmatter(inputs.page_hero_image_url))
        .or_else(|| cover_from_frontmatter(inputs.body_cover_path))
}

/// Resolve a frontmatter cover string to CoverRef. External http(s) URLs
/// pass through; local paths route through ServedPath::from_source.
///
/// Used for every rung of `resolve_cover_chain` including the typed
/// `body_cover_path` (Step 5). The data-URI rejection lives here rather
/// than in `ServedPath::from_source` because data-URIs are an opaque
/// inline-payload scheme — they pass `ServedPath`'s structural rules
/// (no `/`, no `..`, no reserved-prefix) but aren't valid as a served
/// asset URL. Pre-Step-5 the retired `first_body_image` regex skipped
/// data-URIs explicitly; this guard preserves that behavior at the
/// chain layer where it's now relevant for every rung.
fn cover_from_frontmatter(s: Option<&str>) -> Option<CoverRef> {
    let raw = s?;
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(CoverRef::External(raw.to_string()));
    }
    if raw.starts_with("data:") {
        // Inline-payload URI. Not a valid OG card target.
        return None;
    }
    // A video is not a share card. `og:image`, `twitter:image` and the
    // JSON-LD `image` are all emitted verbatim from this URL, and every
    // consumer of them wants a still: crawlers reject a video, and the
    // author's original `.mov` is never deployed at all, so the tag
    // resolves to a 404. moss already renders a thumbnail for exactly this
    // frame — it is what `<video poster>` points at — so route the cover to
    // it rather than dropping the rung and falling through to the auto-card.
    //
    // `to_thumb_if_video` rather than a local extension test, because it is
    // the same function `<video poster>` derives its URL from: cover and
    // poster then name the same file by construction rather than by two
    // lists agreeing. What that leaves uncaught is a cover declared
    // `cover_type: video` on an extensionless path — and there is nothing to
    // point it at, since `to_thumb` could not name a thumbnail for it either.
    //
    // Local paths only. An external `https://cdn/x.mov` would become
    // `https://cdn/x.thumb.jpg` on a host that has no such file, which
    // trades a wrong image for a missing one.
    let raw = moss_core::asset_paths::to_thumb_if_video(raw)
        .map(std::borrow::Cow::Owned)
        .unwrap_or(std::borrow::Cow::Borrowed(raw));
    let stripped = raw.trim_start_matches('/');
    // `raw` arrives already percent-encoded (it is `PathResolver::resolve_url`
    // output), and `ServedPath::from_source` lowercases directory segments —
    // so og:image can emit `/%e7%8d%8e…/x.jpg` where `<img src>` emits
    // `/%E7%8D%8E…/x.jpg`. Same file, not a mangling: RFC 3986 makes escape
    // hex case-insensitive and every server decodes before lookup. Worth
    // knowing when diffing the two, not worth normalizing. (It is only the
    // case that moves — `slugify_dir_path` does not call `generate_slug`,
    // which would have rewritten `%` to the word "percent".)
    match ServedPath::from_source(stripped) {
        Ok(sp) => Some(CoverRef::Local(sp)),
        Err(e) => {
            log::warn!("cover path '{}' rejected by ServedPath: {}", stripped, e);
            None
        }
    }
}

/// Hugo-style filename-convention cover: a file in the page's bundle
/// directory whose stem contains "feature", "cover", or "thumbnail"
/// (case-insensitive substring match, in priority order: feature wins
/// over cover wins over thumbnail) is used as the cover image.
/// Per Hugo's actual behavior, partial-substring matches like
/// `featured-image.jpg`, `My Cover.png`, `thumbnail-large.webp` all qualify.
fn cover_from_filename_convention(
    bundle_dir: Option<&Path>,
    source_root: &Path,
) -> Option<ServedPath> {
    const NEEDLES: &[&str] = &["feature", "cover", "thumbnail"];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "avif", "gif"];

    let dir = bundle_dir?;
    if !dir.is_dir() {
        return None;
    }

    let entries: Vec<_> = std::fs::read_dir(dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    for needle in NEEDLES {
        for entry in &entries {
            let path = entry.path();
            let stem = path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let ext = path.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let (Some(stem), Some(ext)) = (stem, ext) else { continue };
            if !stem.contains(needle) || !EXTS.contains(&ext.as_str()) {
                continue;
            }
            let rel = path.strip_prefix(source_root).ok()?;
            // Normalize `\`→`/` (Windows strip_prefix yields backslashes) before
            // building the ServedPath, so the cover URL matches the output tree.
            let rel_str = moss_core::slug::normalize_separators(&rel.to_string_lossy());
            return ServedPath::from_source(&rel_str).ok();
        }
    }
    None
}

// `first_body_image` (regex scrape of `<img>` / `<video poster>` from
// rendered HTML) was retired in Step 5 of the structural-html-emission
// migration. The body-image rung now reads
// `ParsedDocument.body_cover_path`, captured at parse time by
// `transform_events`. See the module doc above.

/// The picture a reader's quote-card puts at the top of the card: the image
/// THIS page chose for itself, or nothing.
///
/// Two rungs only, in order:
/// 1. the page's `:::hero` image (`ParsedDocument.hero_image_url`, captured at
///    hoisting), when it is an image and not a video;
/// 2. the page's `cover:` frontmatter, when it resolves to an image.
///
/// Nothing else. In particular NOT the auto-generated 1200×630 og card: a page
/// with neither hero nor `cover:` has no cover, and the card must show none —
/// cramming the title card into the cover strip is the regression `05b1d85cb`
/// fixed, and this rung set is what keeps it fixed.
///
/// External URLs are dropped rather than emitted. The value is read back by
/// `share-card.ts` through a `crossOrigin = "anonymous"` `<img>` and drawn onto
/// a canvas, so a cross-origin cover either taints the canvas or fails CORS;
/// a same-origin URL is also, per ADR-013, guaranteed registered in the
/// `AssetRegistry` and therefore cannot 404 in preview.
///
/// `resolve_cover` turns a raw frontmatter path into a root-relative URL
/// (`PathResolver::resolve_url`); the hero URL is already resolved.
pub fn share_cover_url<F: Fn(&str) -> String>(
    hero_image_url: Option<&str>,
    cover: Option<&str>,
    cover_type: Option<&str>,
    resolve_cover: F,
) -> Option<String> {
    use crate::build::media::cover::{detect_cover_type, CoverType};

    // Rung 1 — the page's own hero image. A video hero falls through to `cover:`.
    if let Some(raw) = hero_image_url.filter(|s| !s.is_empty()) {
        let (path, _attrs) = moss_core::media::split_pipe(raw);
        if is_same_origin_url(path) && detect_cover_type(path, None) == CoverType::Image {
            return Some(path.to_string());
        }
    }

    // Rung 2 — `cover:` frontmatter, image covers only.
    let raw = cover.filter(|s| !s.is_empty())?;
    let (path, _attrs) = moss_core::media::split_pipe(raw);
    if detect_cover_type(path, cover_type) != CoverType::Image {
        return None;
    }
    let url = resolve_cover(path);
    is_same_origin_url(&url).then_some(url)
}

/// The `data-share-cover` attribute for a page, or an empty string when the
/// page has no drawable cover. Same shape as `qr::share_qr_attr`: the page
/// states what the card draws, and the card finds nothing by guessing.
pub fn share_cover_attr<F: Fn(&str) -> String>(
    hero_image_url: Option<&str>,
    cover: Option<&str>,
    cover_type: Option<&str>,
    resolve_cover: F,
) -> String {
    match share_cover_url(hero_image_url, cover, cover_type, resolve_cover) {
        Some(url) => format!(
            r#" data-share-cover="{}""#,
            crate::build::page::meta::escape_html_attr(&url)
        ),
        None => String::new(),
    }
}

/// Whether `url` points at the site itself — the only kind of cover URL the
/// share card can draw (see [`share_cover_url`]).
fn is_same_origin_url(url: &str) -> bool {
    !(url.is_empty()
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("//")
        || url.starts_with("data:")
        || url.starts_with("mailto:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_inputs(source_root: &Path) -> CoverChainInputs<'_> {
        CoverChainInputs {
            page_cover: None,
            page_hero_image_url: None,
            body_cover_path: None,
            bundle_dir: None,
            source_root,
        }
    }

    // ---- resolve_cover_chain tests ----

    // ---- video covers resolve to their thumbnail ----

    #[test]
    fn local_video_cover_resolves_to_its_thumbnail() {
        // `og:image` pointing at a `.mov` is doubly broken: crawlers want a
        // still, and the author's original is never deployed, so the URL
        // 404s. Observed live on liu-guo.com/video/lusheng-yelang/.
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("assets/Yelanggu-Lusheng.mov");
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::Local(sp) => assert_eq!(sp.as_str(), "assets/Yelanggu-Lusheng.thumb.jpg"),
            other => panic!("expected Local thumbnail, got {other:?}"),
        }
    }

    #[test]
    fn video_cover_thumbnail_swap_is_case_insensitive() {
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("clip.MOV");
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::Local(sp) => assert_eq!(sp.as_str(), "clip.thumb.jpg"),
            other => panic!("expected Local thumbnail, got {other:?}"),
        }
    }

    #[test]
    fn leading_slash_survives_the_thumbnail_swap() {
        // The swap runs before the leading slash is trimmed, so a rooted
        // path must still land on the same served path a relative one does.
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("/assets/clip.mp4");
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::Local(sp) => assert_eq!(sp.as_str(), "assets/clip.thumb.jpg"),
            other => panic!("expected Local thumbnail, got {other:?}"),
        }
    }

    #[test]
    fn external_video_cover_is_left_alone() {
        // We do not host the thumbnail for someone else's CDN, so deriving
        // one would trade a wrong image for a missing one.
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("https://cdn.example.com/clip.mov");
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::External(url) => assert_eq!(url, "https://cdn.example.com/clip.mov"),
            other => panic!("expected External passthrough, got {other:?}"),
        }
    }

    #[test]
    fn image_covers_are_untouched_by_the_video_swap() {
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("images/page.jpg");
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::Local(sp) => assert_eq!(sp.as_str(), "images/page.jpg"),
            other => panic!("expected the path unchanged, got {other:?}"),
        }
    }

    #[test]
    fn page_cover_wins() {
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: Some("images/page.jpg"),
            page_hero_image_url: None,
            body_cover_path: Some("images/body.jpg"),
            bundle_dir: None,
            source_root: &root,
        };
        let choice = resolve_cover_chain(&inputs).unwrap();
        assert!(matches!(choice, CoverRef::Local(_)));
        if let CoverRef::Local(sp) = choice {
            assert_eq!(sp.as_str(), "images/page.jpg");
        }
    }

    #[test]
    fn page_cover_external_passes_through() {
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: Some("https://cdn.example.com/photo.jpg"),
            page_hero_image_url: None,
            body_cover_path: None,
            bundle_dir: None,
            source_root: &root,
        };
        let choice = resolve_cover_chain(&inputs).unwrap();
        assert!(matches!(choice, CoverRef::External(_)));
        if let CoverRef::External(url) = choice {
            assert_eq!(url, "https://cdn.example.com/photo.jpg");
        }
    }

    #[test]
    fn body_cover_when_no_explicit_cover() {
        // Step-5: the body-image rung is now driven by the typed
        // `body_cover_path` captured at parse time, not a regex scan of
        // rendered HTML. The caller in pipeline.rs records the first
        // `Tag::Image` it sees; this test only confirms that whatever
        // string the caller hands in feeds the cover chain correctly.
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: None,
            page_hero_image_url: None,
            body_cover_path: Some("images/body.jpg"),
            bundle_dir: None,
            source_root: &root,
        };
        let choice = resolve_cover_chain(&inputs).unwrap();
        if let CoverRef::Local(sp) = choice {
            assert_eq!(sp.as_str(), "images/body.jpg");
        } else {
            panic!("expected Local cover ref");
        }
    }

    #[test]
    fn returns_none_when_chain_exhausted_without_auto_card() {
        let root = std::path::PathBuf::from("/tmp");
        assert!(resolve_cover_chain(&empty_inputs(&root)).is_none());
    }

    // `handles_video_poster` and `handles_single_quoted_attrs` were
    // tests of the retired `first_body_image` regex. Post-Step-5 the
    // body-image rung is fed by the typed `body_cover_path` captured
    // at parse time, which only records `Tag::Image` (markdown `![]()`
    // syntax) — never `<video poster>` (not a markdown construct) and
    // never raw HTML `<img>` (an `Event::Html` carve-out). Authors who
    // want a video poster as cover should set frontmatter `cover:`.

    // ---- the page's own hero rung ----

    #[test]
    fn declared_cover_still_beats_the_page_hero() {
        let root = std::path::PathBuf::from("/tmp");
        let mut inputs = empty_inputs(&root);
        inputs.page_cover = Some("images/declared.jpg");
        inputs.page_hero_image_url = Some("/paintings/plum.jpg");
        let choice = resolve_cover_chain(&inputs).unwrap();
        if let CoverRef::Local(sp) = choice {
            assert_eq!(sp.as_str(), "images/declared.jpg");
        } else {
            panic!("expected Local cover ref");
        }
    }

    /// The defect this rung closes: an article whose only image is its hero
    /// shared as a near-blank generated card, because the hero was read only
    /// when the page WAS the home page.
    #[test]
    fn page_hero_beats_the_body_image() {
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: None,
            page_hero_image_url: Some("/paintings/plum.jpg"),
            body_cover_path: Some("images/body.jpg"),
            bundle_dir: None,
            source_root: &root,
        };
        let choice = resolve_cover_chain(&inputs).unwrap();
        if let CoverRef::Local(sp) = choice {
            assert_eq!(sp.as_str(), "paintings/plum.jpg");
        } else {
            panic!("expected Local cover ref");
        }
    }

    #[test]
    fn body_cover_data_uri_is_rejected() {
        // A markdown `![](data:image/png;base64,…)` flows through
        // `resolve_link` unchanged (it's neither a wikilink nor a
        // page_map entry), gets stored in `body_cover_path`, then
        // arrives at `cover_from_frontmatter`. Neither `http://` nor
        // `https://` matches, so `ServedPath::from_source` is asked
        // to parse it — which rejects (colons + base64 garbage are
        // not a valid relative path). The rung short-circuits to
        // None and the chain falls through to the auto-OG-card.
        //
        // Mirrors the deleted `first_body_image_skips_data_uri` test
        // — the rejection path moved from regex-level to ServedPath-
        // level but the user-visible outcome is the same.
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: None,
            page_hero_image_url: None,
            body_cover_path: Some("data:image/png;base64,abc"),
            bundle_dir: None,
            source_root: &root,
        };
        assert!(
            resolve_cover_chain(&inputs).is_none(),
            "data: URI in body_cover_path must not produce a cover"
        );
    }

    #[test]
    fn body_cover_external_passes_through_as_external() {
        // The body_cover_path field stores whatever the resolver produced
        // for the first markdown image — including external http(s) URLs
        // when the author wrote `![](https://example.com/photo.jpg)`. The
        // chain treats them as External cover refs (not Local) so they
        // skip ServedPath validation.
        let root = std::path::PathBuf::from("/tmp");
        let inputs = CoverChainInputs {
            page_cover: None,
            page_hero_image_url: None,
            body_cover_path: Some("https://example.com/photo.jpg"),
            bundle_dir: None,
            source_root: &root,
        };
        let choice = resolve_cover_chain(&inputs).unwrap();
        match choice {
            CoverRef::External(url) => assert_eq!(url, "https://example.com/photo.jpg"),
            CoverRef::Local(_) => panic!("expected External cover ref"),
        }
    }

    // ---- filename convention tests ----

    #[test]
    fn filename_convention_finds_cover_jpg() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("posts/my-post");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("cover.jpg"), b"").unwrap();
        std::fs::write(bundle.join("index.md"), b"").unwrap();

        let result = cover_from_filename_convention(Some(&bundle), tmp.path());
        assert!(result.is_some(), "should find cover.jpg");
        let sp = result.unwrap();
        assert_eq!(sp.as_str(), "posts/my-post/cover.jpg");
    }

    #[test]
    fn filename_convention_prefers_feature_over_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("posts/my-post");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("cover.jpg"), b"").unwrap();
        std::fs::write(bundle.join("featured-image.png"), b"").unwrap();
        std::fs::write(bundle.join("index.md"), b"").unwrap();

        let result = cover_from_filename_convention(Some(&bundle), tmp.path());
        assert!(result.is_some(), "should find a cover");
        let sp = result.unwrap();
        // "feature" needle wins over "cover" needle
        assert_eq!(sp.as_str(), "posts/my-post/featured-image.png");
    }

    // ---- share-card cover (data-share-cover) ----

    /// Stand-in for `PathResolver::resolve_url`: root-relative, absolutes pass.
    fn resolve(raw: &str) -> String {
        if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else {
            format!("/{}", raw.trim_start_matches('/')) // allow:served-path-url-construct
        }
    }

    #[test]
    fn share_cover_prefers_the_page_hero_image() {
        assert_eq!(
            share_cover_url(Some("/posts/a/assets/hero.jpg"), Some("other.png"), None, resolve),
            Some("/posts/a/assets/hero.jpg".to_string())
        );
    }

    #[test]
    fn share_cover_falls_back_to_frontmatter_cover() {
        assert_eq!(
            share_cover_url(None, Some("posts/a/cover.png"), None, resolve),
            Some("/posts/a/cover.png".to_string())
        );
    }

    #[test]
    fn share_cover_skips_a_video_hero_for_the_image_cover() {
        // A video hero cannot be drawn onto the card; `cover:` still can.
        assert_eq!(
            share_cover_url(Some("/clips/intro.mp4"), Some("cover.jpg"), None, resolve),
            Some("/cover.jpg".to_string())
        );
    }

    #[test]
    fn share_cover_is_absent_for_a_video_cover() {
        assert_eq!(share_cover_url(None, Some("clips/intro.mp4"), None, resolve), None);
        // …including when only `cover_type:` says so.
        assert_eq!(
            share_cover_url(None, Some("stream/thing"), Some("video"), resolve),
            None
        );
    }

    #[test]
    fn share_cover_is_absent_without_hero_or_cover() {
        // The auto-generated og card is not a cover — it never reaches here,
        // and nothing else may stand in for it.
        assert_eq!(share_cover_url(None, None, None, resolve), None);
    }

    #[test]
    fn share_cover_drops_external_urls() {
        // Cross-origin bytes taint the card canvas; no cover beats a broken one.
        assert_eq!(
            share_cover_url(Some("https://cdn.example.com/hero.jpg"), None, None, resolve),
            None
        );
        assert_eq!(
            share_cover_url(None, Some("https://cdn.example.com/cover.jpg"), None, resolve),
            None
        );
    }

    #[test]
    fn share_cover_strips_pipe_display_attrs() {
        assert_eq!(
            share_cover_url(None, Some("cover.jpg|wide"), None, resolve),
            Some("/cover.jpg".to_string())
        );
    }

    #[test]
    fn filename_convention_returns_none_when_no_match() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("posts/my-post");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("index.md"), b"").unwrap();
        std::fs::write(bundle.join("diagram.png"), b"").unwrap();

        let result = cover_from_filename_convention(Some(&bundle), tmp.path());
        assert!(result.is_none(), "no cover/feature/thumbnail file present");
    }
}

// ── Which covers a platform can be trusted with ──────────────────────────────

/// The aspect-ratio band moss hands a platform verbatim.
///
/// Every major platform fill-and-centre-crops `og:image` to its own slot —
/// 1.91:1 on Facebook, LinkedIn and WhatsApp, 2:1 on X's
/// `summary_large_image`, ~1:1 on WeChat, Mastodon's small card, and iOS 17+
/// Messages. Slack is the one that documents scale-to-fit (360x500 box), and
/// Telegram probably does; neither is where reach comes from. So the question
/// for a cover is not "does it look good" but "how much of it does a crop
/// keep".
///
/// Inside this band a centre crop to 1.91:1 keeps roughly two thirds of the
/// picture and the shape still reads as a photograph, which is what those
/// slots were drawn for. Outside it — a portrait, a square, a hanging scroll,
/// a handscroll — the crop is not a crop but an excerpt: a 2290x4000 scroll
/// shows a 29%-tall band from its middle, a 3400x447 handscroll keeps 26% of
/// its width. Those get the plate card instead, which moss composes and no
/// platform re-crops below 2:1.
///
/// Sources (seen 2026-09-11): Facebook's own sharing/images doc — "try to
/// keep your images as close to 1.91:1 aspect ratio as possible to display
/// the full image in Feed without any cropping"; LinkedIn help a521928
/// (1200x627, 1.91:1); X card docs (2:1, min 300x157); Slack's
/// `page_attachments.md` (max width 360 / max height 500, aspect preserved).
/// Full survey: `docs/archive/2026-09-11-share-card-vertical-design.md`.
pub const PASSTHROUGH_MIN_AR: f32 = 1.25;
pub const PASSTHROUGH_MAX_AR: f32 = 2.4;

/// Is this cover's shape one a platform's centre crop would gut?
pub fn needs_plate(width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let ar = width as f32 / height as f32;
    !(PASSTHROUGH_MIN_AR..=PASSTHROUGH_MAX_AR).contains(&ar)
}

/// A cover moss will draw into the card rather than hand over, with the stat
/// triple that keys the rendered card. Owned, because [`CardPlate`] borrows
/// the path and the caller needs somewhere for it to live.
///
/// [`CardPlate`]: crate::build::page::og_card::CardPlate
pub struct PlateSource {
    file: std::path::PathBuf,
    /// `file`, relative to `source_root` — `meta.path` already IS this,
    /// since `file` was built as `source_root.join(&meta.path)` below.
    /// This is what `content_hash` keys on instead of `file` itself: the
    /// same vault built from two different absolute locations (a
    /// re-checkout, or build_parity_test's two temp-dir copies) must get
    /// the same OG-card hash, and `file`'s absolute form does not.
    relative_path: String,
    width: u32,
    height: u32,
    len: u64,
    mtime_secs: i64,
}

impl PlateSource {
    pub fn as_plate(&self) -> crate::build::page::og_card::CardPlate<'_> {
        crate::build::page::og_card::CardPlate {
            source: &self.file,
            relative_path: &self.relative_path,
            width: self.width,
            height: self.height,
            len: self.len,
            mtime_secs: self.mtime_secs,
        }
    }
}

/// The dimensions moss scanned for this cover, if it scanned it at all.
///
/// An `External` cover is somebody else's file on somebody else's host: moss
/// has neither its bytes nor its size, and says so by answering `None`.
pub fn scanned_dims(
    images: &[crate::types::content::MediaMetadata],
    cover: &CoverRef,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> Option<(u32, u32)> {
    match cover {
        CoverRef::External(_) => None,
        CoverRef::Local(sp) => {
            scanned_cover(images, &sp.to_relative_url(), dir_overrides).map(|(_, d)| d)
        }
    }
}

/// This cover as a plate, or `None` when its shape survives a platform crop
/// (or moss cannot reach the file to draw it).
pub fn plate_source(
    images: &[crate::types::content::MediaMetadata],
    cover: &CoverRef,
    dir_overrides: &std::collections::HashMap<String, String>,
    source_root: &Path,
) -> Option<PlateSource> {
    let CoverRef::Local(sp) = cover else {
        return None;
    };
    let (meta, (width, height)) = scanned_cover(images, &sp.to_relative_url(), dir_overrides)?;
    if !needs_plate(width, height) {
        return None;
    }
    let file = source_root.join(&meta.path);
    let stat = std::fs::metadata(&file).ok()?;
    Some(PlateSource {
        file,
        relative_path: meta.path.clone(),
        width,
        height,
        len: stat.len(),
        mtime_secs: stat
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    })
}

/// The scanned source file behind a resolved cover URL, with its dimensions.
///
/// Exact match only, against the two keys a scanned path can be emitted
/// under: its own, and its slugified output URL (`resolve_path_with_overrides`
/// — the same function `MediaDimensionLookup` indexes its second key with, so
/// a cover under a `slug:`-overridden folder resolves). Deliberately none of
/// `resolve_entry`'s stem-matching: this answer decides whether to REDRAW the
/// page's picture, and a near-miss would draw a different page's.
fn scanned_cover<'a>(
    images: &'a [crate::types::content::MediaMetadata],
    cover_url: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> Option<(&'a crate::types::content::MediaMetadata, (u32, u32))> {
    let decoded = moss_core::resolve::fuzzy_path::percent_decode_path(cover_url);
    let key = decoded.strip_prefix('/').unwrap_or(&decoded);
    let meta = images.iter().find(|m| {
        m.path == key
            || moss_core::resolve::output_url::resolve_path_with_overrides(&m.path, dir_overrides)
                == key
    })?;
    let dims = meta.dimensions?;
    Some((meta, dims))
}

#[cfg(test)]
mod plate_tests {
    use super::*;
    use crate::types::content::MediaMetadata;

    fn scanned(path: &str, dims: (u32, u32)) -> MediaMetadata {
        MediaMetadata {
            path: path.to_string(),
            file_type: "jpg".into(),
            size: 0,
            modified: None,
            dimensions: Some(dims),
            dominant_color: None,
            lqip_data_uri: None,
            is_animated: false,
        }
    }

    /// The band is about what a platform's centre crop would leave, not about
    /// portrait-vs-landscape: a 16:9 photo goes over untouched, a hanging
    /// scroll, a handscroll and a square seal do not.
    #[test]
    fn only_shapes_a_centre_crop_would_gut_become_plates() {
        assert!(!needs_plate(1200, 630), "1.91:1, the shape the slots were drawn for");
        assert!(!needs_plate(1920, 1080), "16:9");
        assert!(!needs_plate(2000, 1500), "4:3");
        assert!(needs_plate(2290, 4000), "a hanging scroll");
        assert!(needs_plate(3400, 447), "a handscroll");
        assert!(needs_plate(480, 480), "a square seal");
        assert!(needs_plate(1080, 1350), "a portrait photograph");
        // Nothing to divide by, and nothing to decide.
        assert!(!needs_plate(0, 100));
    }

    /// The cover URL is percent-encoded and its directory segments are
    /// lowercased by `ServedPath`; the scanned path is neither. A CJK site is
    /// where those two forms differ on every single asset, so a lookup that
    /// only tried the raw string would find nothing on exactly the sites this
    /// feature is for.
    #[test]
    fn a_percent_encoded_cjk_cover_finds_its_scanned_source() {
        let images = vec![scanned("\u{756b}/\u{96d9}\u{9df9}\u{5716}.jpg", (2454, 4000))];
        let overrides = std::collections::HashMap::new();
        let url = "/%e7%95%ab/%E9%9B%99%E9%B7%B9%E5%9C%96.jpg";
        let (meta, dims) = scanned_cover(&images, url, &overrides).expect("cover not found");
        assert_eq!(meta.path, "\u{756b}/\u{96d9}\u{9df9}\u{5716}.jpg");
        assert_eq!(dims, (2454, 4000));
        // A near-miss is not a hit: this answer decides whether to redraw the
        // page's picture, and another page's picture is the wrong answer.
        assert!(scanned_cover(&images, "/other/\u{96d9}\u{9df9}\u{5716}.jpg", &overrides).is_none());
    }
}
