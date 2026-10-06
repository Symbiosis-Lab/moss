//! The local-file import path: an MHTML web-archive or a plain `.html`
//! capture, read straight off disk — no network, no [`super::crawl_state::CrawlState`].
//! Sibling to [`super::run::scrape_to_folder`], the live-site path; the two
//! share note composition (`compose_note`) and the output-writing primitives
//! through `run`, and the fetch pipeline's URL/content-type helpers through
//! [`super::fetch`].

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::converter::extract_article;
use super::fetch::{content_type_to_extension, hash_url, strip_query};
use super::run::{compose_note, write_note, ScrapeResult, ASSETS_SUBDIR};
use super::writer::{rename_for_collision, sanitize_filename};

/// Import a local file (MHTML web-archive or plain `.html`) into `output_dir`.
///
/// Unlike [`super::run::scrape_to_folder`], nothing is fetched over the network. For an
/// MHTML archive the original page URL and every embedded asset come straight
/// out of the file; the archive's `Snapshot-Content-Location` becomes the
/// `origin` frontmatter. This is the local-file arm of `moss import`, letting
/// the user bring in a page they saved from a login-gated or JS-heavy site
/// (e.g. a douban note saved via "Save Page As").
pub(crate) async fn import_local_file(path: &Path, output_dir: &Path) -> Result<ScrapeResult, String> {
    // The user picked this path in a file dialog, so it is routinely inside an
    // iCloud or Drive folder and routinely evicted — a plain `fs::read` of one
    // answers `Resource deadlock avoided (os error 11)` and the import reports
    // that as the reason. Asking is what makes the file arrive.
    //
    // Blocking, inside an `async fn`, for up to `INTERACTIVE_DEADLINE`. Today
    // the only caller is `cli::import` on a runtime of its own, so it parks
    // nothing shared. An app-side caller must reach this through
    // `spawn_blocking`, or one evicted import stalls a runtime worker for 15s.
    let bytes = crate::build::cloud_readiness::read_with_materialize_wait(
        path,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    )
    .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

    // MHTML vs plain HTML. Trust an explicit extension; only content-sniff when
    // the extension is unknown, so a `.html` file that merely mentions
    // "multipart/related" in its text isn't misrouted to the MHTML parser.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let looks_mhtml = match ext.as_str() {
        "mhtml" | "mht" => true,
        "html" | "htm" => false,
        _ => {
            let head =
                String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
            head.contains("multipart/related")
        }
    };

    let (html, source_url, embedded): (
        String,
        Option<String>,
        HashMap<String, super::mhtml::MhtmlResource>,
    ) = if looks_mhtml {
        let doc = super::mhtml::parse_mhtml(&bytes)?;
        (doc.html, doc.source_url, doc.resources)
    } else {
        (
            String::from_utf8_lossy(&bytes).to_string(),
            None,
            HashMap::new(),
        )
    };

    // The archived page URL becomes the `origin` provenance field
    // (query/fragment stripped so tracking params don't leak into
    // frontmatter). Empty when unknown.
    let source_clean = source_url.as_deref().map(strip_query).unwrap_or_default();
    let base_for_urls = source_url.as_deref().unwrap_or("");

    let mut article = extract_article(&html, base_for_urls);

    let assets_dir = output_dir.join(ASSETS_SUBDIR);
    fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Cover: og:image, else the first in-body image (same policy as the URL
    // path — `compose_note` below owns the rest of the shared tail).
    let cover_remote = article
        .metadata
        .og_image
        .clone()
        .or_else(|| {
            super::metadata::fallback_cover(&article.markdown, &article.metadata.chrome_images, base_for_urls)
        });

    // Write embedded assets (MHTML) to disk; remote images in a plain .html
    // import are left as absolute URLs (the local path never hits the network).
    let mut remote_to_local: HashMap<String, String> = HashMap::new();
    for media_url in &article.media_urls {
        if remote_to_local.contains_key(media_url) {
            continue;
        }
        if let Some(res) = embedded.get(media_url) {
            let hash = hash_url(media_url);
            let extension = content_type_to_extension(&res.content_type);
            let filename = format!("{}.{}", hash, extension);
            // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — an imported media file under `assets/imported/`
            fs::write(assets_dir.join(&filename), &res.bytes)
                .map_err(|e| format!("Failed to write asset: {}", e))?;
            remote_to_local.insert(
                media_url.clone(),
                format!("./{}/{}", ASSETS_SUBDIR, filename),
            );
        }
    }

    // One file in, one file out: an empty body fails loudly here rather than
    // writing an empty note and reporting success.
    let full_content = compose_note(
        &mut article,
        &remote_to_local,
        cover_remote,
        &source_clean,
        None,
    )
    .ok_or_else(|| {
        format!(
            "no article content found in {} (unsupported page or empty body)",
            path.display()
        )
    })?;

    // Name the note after its title, falling back to the source file stem —
    // both through the one filename policy, so the fallback cannot be the
    // looser of the two.
    let base_name = [
        article.metadata.title.as_deref(),
        path.file_stem().and_then(|s| s.to_str()),
    ]
    .into_iter()
    .flatten()
    .map(sanitize_filename)
    .find(|s| !s.is_empty())
    .unwrap_or_else(|| "imported".to_string());
    let relative = rename_for_collision(output_dir, &format!("{base_name}.md"));
    write_note(output_dir, &relative, &full_content)?;

    Ok(ScrapeResult {
        success: true,
        total_pages: 1,
        failed_pages: 0,
        skipped_pages: 0,
        duplicate_pages: 0,
        unreachable_variants: 0,
        unreachable_files: 0,
        widgets_carried: article.widgets.carried,
        widgets_dropped: article.widgets.dropped,
        capped: false,
        remaining_urls: 0,
        sitemap_urls: 0,
        sitemap_truncated: false,
        rate_limited_hosts: Vec::new(),
        chrome: Default::default(),
        error: None,
    })
}

#[cfg(test)]
#[path = "import_local_tests.rs"]
mod tests;
