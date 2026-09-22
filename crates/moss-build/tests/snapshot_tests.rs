//! Snapshot Tests for moss Build Output
//!
//! These tests verify that moss build produces consistent, deterministic output.
//!
//! Moved here from the private desktop-app repo (`src-tauri/tests/snapshot_tests.rs`)
//! so a render change and its fixture refresh land in one commit, in the repo that
//! owns the pipeline. Fixture sites are a COPY of the private repo's
//! `tests/fixtures/<site>` — several of those (`basic-site`, `media-site`,
//! `hooks-site` at least) are also read by the private repo's `build_parity_test.rs`
//! and stay there; see that repo's phase-B plan before deleting anything.
//!
//! ## Test Strategy
//! - **Full content comparison** for HTML files (with normalization)
//! - **Structure-only comparison** for assets (presence check)
//!
//! ## Running Tests
//! ```bash
//! cargo test -p moss-build --test snapshot_tests
//! ```
//!
//! ## Updating Snapshots
//! ```bash
//! SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests
//! ```
//!
//! ## Parallel-safety and determinism
//!
//! Each `run_snapshot_test` call copies its fixture input to a fresh
//! `$TMPDIR/moss_snapshot_test_<uuid>/` directory before building, so
//! concurrent tests cannot share `.moss/build.nosync/` state via the fixture
//! input path. Tests are safe to run with the default `--test-threads`
//! value (number of logical CPUs).
//!
//! The early observed flakiness (Extra/Missing file across runs) was caused
//! by stale expected-output snapshots that diverged from the current build
//! output after code changes, not by an actual concurrency bug. Re-run
//! `SNAPSHOTS=overwrite cargo test -p moss-build --test snapshot_tests -- --test-threads=1`
//! any time you change output-affecting code to bring the expected directory
//! back in sync.
//!
//! ### Env-var discipline
//!
//! A `[channels.email]` config in a fixture just adds the subscribe form +
//! email CSS to the output — no build path performs network fetches
//! (`fetch_buttondown_username` has no production caller; see
//! docs/archive/2026-06-10-email-footer-mode-independent-design.md), so
//! snapshot tests stay deterministic and offline.
//!
//! One process-global env var is still read during some code paths:
//!
//! - `MOSS_WATCH_NO_GATE` — read by `build::watch::evaluate_gate` (only
//!   reached from the file-watcher path, never from `build_sync`). Tests
//!   that mutate this var hold `KILL_SWITCH_ENV_LOCK` (private to
//!   `build::watch` tests) for the full set→call→unset sequence.
//!
//! No current snapshot fixture exercises it.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/copy_dir.rs"]
mod copy_dir;
use copy_dir::copy_dir_recursive;
use walkdir::WalkDir;

/// What moss writes as a site's favicon when the site has none of its own —
/// the same bytes `shell.rs` includes as `DEFAULT_FAVICON`, read through
/// moss-build's own public API (this test binary is part of the moss-build
/// crate, not a downstream consumer of it).
const MOSS_MARK: &str = moss_build::build::page::shell::DEFAULT_FAVICON;

/// Reconstruction of the desktop app's `moss::build_sync` (`src-tauri/src/build.rs`)
/// from moss-build's own public API, so this suite has no dependency on the
/// desktop crate. `cli_host_ports` is the exact headless host `moss-cli build`
/// uses in production — not a parallel build path invented for this test.
///
/// The one behavioral difference from the desktop wrapper: `HostStore::
/// run_vault_migrations` here is `CliStore`'s (migrates `.moss/config.toml` in
/// memory only), where the desktop wrapper's `AppStore` also rewrites the file
/// on disk. Every fixture's checked-in config is already current, so both
/// arms produce the same build output; this only matters for a fixture whose
/// config is intentionally stale (there is none in this suite today).
fn build_sync(folder_path: &str, auto_serve: bool) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: auto_serve,
        host: cli_host_ports(folder_path),
        trigger: BuildTrigger::Full,
        // Mirrors `build_sync`'s own doc comment on `PipelineConfig::
        // exits_after_build`: this process drops its runtime right after
        // `run_pipeline` returns, so link-meta must prewarm synchronously
        // rather than on a background task that would be killed first.
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

/// Get the project root directory (where Cargo.toml is)
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the test fixtures directory
fn fixtures_dir() -> PathBuf {
    project_root().join("tests").join("fixtures").join("snapshot-sites")
}

/// Normalize HTML content for snapshot comparison.
/// Removes/redacts values that may change between runs:
/// - Timestamps
/// - Absolute paths
/// - Generated IDs
/// - Temp directory names (UUIDs)
fn normalize_html(content: &str) -> String {
    let mut normalized = content.to_string();

    // Remove/normalize any potential timestamp comments
    // Example: <!-- Generated: 2024-01-15 12:34:56 -->
    let timestamp_re = regex::Regex::new(r"<!-- Generated:.*?-->").unwrap();
    normalized = timestamp_re.replace_all(&normalized, "<!-- Generated: [TIMESTAMP] -->").to_string();

    // Normalize any absolute paths that might leak into output
    // Replace /Users/... or /home/... paths with [PATH]
    let path_re = regex::Regex::new(r#"(?:/Users|/home)/[^"'\s<>]+"#).unwrap();
    normalized = path_re.replace_all(&normalized, "[PATH]").to_string();

    // Normalize temp directory names (moss_snapshot_test_UUID pattern)
    // These appear in titles and site names
    let temp_dir_re = regex::Regex::new(r"moss_snapshot_test_[a-f0-9-]{36}").unwrap();
    normalized = temp_dir_re.replace_all(&normalized, "[TEST_DIR]").to_string();

    // Normalize the SPACE-form of the temp-dir name. When a fixture's homepage
    // has no `title:`, the site name falls back to the project folder name,
    // resolved through `moss_core::heading::filename_text`, which replaces the
    // `_`/`-` in `moss_snapshot_test_<uuid>` with spaces. This surfaces
    // as `<title>moss snapshot test <uuid-words></title>` on every page — a
    // per-run, non-deterministic value, redact it like the underscore form.
    let temp_dir_spaced_re =
        regex::Regex::new(r"moss snapshot test [a-f0-9]{8} [a-f0-9]{4} [a-f0-9]{4} [a-f0-9]{4} [a-f0-9]{12}")
            .unwrap();
    normalized = temp_dir_spaced_re
        .replace_all(&normalized, "[TEST_DIR]")
        .to_string();

    // Also normalize any other UUID-like patterns that might appear
    let uuid_re = regex::Regex::new(r"[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}").unwrap();
    normalized = uuid_re.replace_all(&normalized, "[UUID]").to_string();

    // Normalize LQIP (Low-Quality Image Placeholder) data URIs. The pure-Rust
    // image crate's JPEG encoder is not bit-identical across platforms when
    // downscaling produces sub-pixel rounding differences (macOS vs Linux).
    // The LQIP byte-for-byte content is not part of the user-facing contract;
    // its presence and shape are. Replace the base64 payload with a marker so
    // snapshots are platform-independent.
    let lqip_re = regex::Regex::new(r#"data:image/jpeg;base64,[A-Za-z0-9+/=]+"#).unwrap();
    normalized = lqip_re.replace_all(&normalized, "data:image/jpeg;base64,[LQIP]").to_string();

    // Strip auto-generated OG card meta tags. Card rendering uses resvg with
    // system fonts and silently falls back to no card on environments where
    // resvg can't resolve a font (Linux CI). The card's existence is not
    // part of the snapshot's user-facing contract; filenames in `_moss/og/`
    // are content-hashed and skipped via the file walker filter below.
    // Drop og:image, og:image:alt, og:image:width, og:image:height, twitter:card,
    // twitter:image, twitter:image:alt so HTML comparison ignores their absence.
    let og_meta_re = regex::Regex::new(
        r#"\s*<meta\s+(property|name)="(og:image(?::alt|:width|:height)?|twitter:card|twitter:image(?::alt)?)"[^>]*>\n?"#
    ).unwrap();
    normalized = og_meta_re.replace_all(&normalized, "\n").to_string();

    // Normalize the content-hashed OG-card filename wherever it survives the
    // meta-tag stripping above — notably the JSON-LD structured-data
    // `"image": "/_moss/og/<hash>.png"` field. The hash is derived from the
    // card's rendered content (which includes the site name); when the site
    // name falls back to the per-run temp-dir folder name (a title-less
    // homepage), the hash is non-deterministic across runs. Like the og:image
    // meta tags, the card's existence — not its hash — is the contract.
    let og_hash_re = regex::Regex::new(r#"/_moss/og/[a-f0-9]+\.png"#).unwrap();
    normalized = og_hash_re.replace_all(&normalized, "/_moss/og/[OG_HASH].png").to_string();

    // Strip staging/ preview annotation attributes so snapshot golden files
    // remain valid. staging/ carries data-source-line, data-source-fm,
    // data-source-none, and data-moss-preview for editor scroll-sync.
    // ship_phase strips them for deploy-ready output; normalize_html must
    // strip them too so snapshots are portable between staging/ and deploy.
    let anno_line_re = regex::Regex::new(r#" data-source-line="\d+""#).unwrap();
    normalized = anno_line_re.replace_all(&normalized, "").to_string();
    let anno_fm_re = regex::Regex::new(r#" data-source-fm="[^"]*""#).unwrap();
    normalized = anno_fm_re.replace_all(&normalized, "").to_string();
    let anno_none_re = regex::Regex::new(r#" data-source-none"#).unwrap();
    normalized = anno_none_re.replace_all(&normalized, "").to_string();
    let anno_preview_re = regex::Regex::new(r#" data-moss-preview"#).unwrap();
    normalized = anno_preview_re.replace_all(&normalized, "").to_string();
    let anno_range_re = regex::Regex::new(r#" data-source-range="[^"]*""#).unwrap();
    normalized = anno_range_re.replace_all(&normalized, "").to_string();

    // Normalize dates in format "YYYY · M · D" or "YYYY · M" (used in article listings)
    // This ensures tests don't fail when run on different dates
    let date_full_re = regex::Regex::new(r"\d{4} · \d{1,2} · \d{1,2}").unwrap();
    normalized = date_full_re.replace_all(&normalized, "[DATE]").to_string();

    let date_month_re = regex::Regex::new(r"\d{4} · \d{1,2}").unwrap();
    normalized = date_month_re.replace_all(&normalized, "[DATE]").to_string();

    // Normalize year headings in article listings (e.g., <h2>2025</h2>)
    let year_heading_re = regex::Regex::new(r"<h2>(20\d{2})</h2>").unwrap();
    normalized = year_heading_re.replace_all(&normalized, "<h2>[YEAR]</h2>").to_string();

    // Sort moss-card elements to handle filesystem ordering differences between macOS/Linux
    // Extract all moss-card elements, sort them, and replace the section
    let article_item_re = regex::Regex::new(r#"<p class="moss-card"><a href="[^"]+"><span class="date">\[DATE\]</span><span class="title">[^<]+</span></a></p>"#).unwrap();
    let mut article_items: Vec<&str> = article_item_re.find_iter(&normalized).map(|m| m.as_str()).collect();
    if article_items.len() > 1 {
        article_items.sort();
        // Replace all article items with sorted versions
        let sorted_items = article_items.join("\n");
        // Find the section containing article items and replace
        let section_re = regex::Regex::new(r#"(<section class="moss-cards-minimal-year-group minimal">\s*<h2>\[YEAR\]</h2>\s*)(<p class="moss-card">.*?</p>\s*)+"#).unwrap();
        if let Some(cap) = section_re.find(&normalized) {
            let section_start = cap.as_str();
            if let Some(prefix_match) = regex::Regex::new(r#"<section class="moss-cards-minimal-year-group minimal">\s*<h2>\[YEAR\]</h2>\s*"#).unwrap().find(section_start) {
                let new_section = format!("{}{}\n", prefix_match.as_str(), sorted_items);
                normalized = section_re.replace(&normalized, new_section.as_str()).to_string();
            }
        }
    }

    normalized
}

/// Paths whose presence depends on the build environment (font availability,
/// image-encoding determinism) and shouldn't gate the snapshot diff.
///
/// `_moss/og/<hash>.png` — auto-generated OG cards. resvg silently falls back
/// to no card on environments where it can't resolve a font, so Linux CI may
/// emit no PNGs while macOS local emits several. The og:image meta tags that
/// reference these are stripped in normalize_html so HTML comparison is
/// platform-independent too.
fn is_platform_variant_path(rel: &Path) -> bool {
    // `rel` may carry OS separators (backslash on Windows); normalize to `/`
    // before the prefix check or the og-card filter silently fails on Windows
    // and the PNGs leak into the comparison.
    let s = moss_core::slug::normalize_separators(&rel.to_string_lossy());
    s.starts_with("_moss/og/") && s.ends_with(".png")
}

/// Compare two directories for snapshot testing.
/// Returns a list of differences found.
/// Text artifacts whose content `compare_directories` checks.
///
/// Until 2026-08-04 this was `html` and `css` only, and everything else
/// was presence-checked but never read. Artifacts with a stable filename rotted
/// where no test could see it — a QR SVG drifted across two regenerations with
/// no QR source change, no dependency change and no URL change anywhere in the
/// tree. (Content-hashed names like `theme.<hash>.js` were already caught, but
/// by the "Missing file" arm, not by content.)
///
/// Deliberately a list of *text* formats rather than "everything that is not an
/// image": a new binary format should have to opt in here after someone has
/// thought about whether its bytes are reproducible, rather than silently
/// joining the compared set and going red on an unrelated dependency bump.
///
/// Raster images (png/jpg/jpeg/webp) stay presence-only ON PURPOSE. Their bytes
/// come out of an encoder and can legitimately differ between libwebp /
/// image-crate versions and between platforms, so comparing them would turn a
/// routine dependency bump into a red release gate on someone else's machine.
/// The HTML that references them already asserts their dimensions and srcset
/// rungs. Pixel-level regressions want a perceptual diff, not this function.
fn is_content_compared(extension: &str) -> bool {
    matches!(
        extension,
        "html" | "css" | "js" | "svg" | "txt" | "json" | "xml"
    )
}

fn compare_directories(actual: &Path, expected: &Path) -> Vec<String> {
    let mut diffs = Vec::new();

    // Collect all files in expected directory. Key each relative path on its
    // `/`-form string so set membership is separator-agnostic — on Windows
    // WalkDir yields backslash paths that would otherwise never equal the
    // `/`-form expected manifest.
    let expected_files: Vec<String> = WalkDir::new(expected)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(expected).unwrap().to_path_buf())
        .filter(|p| !is_platform_variant_path(p))
        .map(|p| moss_core::slug::normalize_separators(&p.to_string_lossy()))
        .collect();

    // Collect all files in actual directory
    let actual_files: Vec<String> = WalkDir::new(actual)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(actual).unwrap().to_path_buf())
        .filter(|p| !is_platform_variant_path(p))
        .map(|p| moss_core::slug::normalize_separators(&p.to_string_lossy()))
        .collect();

    // Check for missing files (in expected but not in actual)
    for expected_file in &expected_files {
        // `expected_file` is a `/`-form relative string; `Path::join` accepts
        // `/`-separated input on Windows, so `.exists()` resolves correctly.
        let actual_path = actual.join(expected_file);
        if !actual_path.exists() {
            diffs.push(format!("Missing file: {}", expected_file));
            continue;
        }

        // Compare file contents for HTML files
        let extension = Path::new(expected_file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if is_content_compared(extension) {
            let expected_content = fs::read_to_string(expected.join(expected_file))
                .unwrap_or_else(|_| String::new());
            let actual_content = fs::read_to_string(&actual_path)
                .unwrap_or_else(|_| String::new());

            let normalized_expected = normalize_html(&expected_content);
            let normalized_actual = normalize_html(&actual_content);

            if normalized_expected != normalized_actual {
                // Find first difference position for debugging
                let first_diff = normalized_expected
                    .chars()
                    .zip(normalized_actual.chars())
                    .enumerate()
                    .find(|(_, (a, b))| a != b)
                    .map(|(i, _)| i);

                let diff_info = if let Some(pos) = first_diff {
                    let start = pos.saturating_sub(20);
                    let end = (pos + 50).min(normalized_expected.len()).min(normalized_actual.len());
                    format!(
                        "\n  First diff at char {}: expected '{}' vs actual '{}'",
                        pos,
                        &normalized_expected[start..end],
                        &normalized_actual[start..end.min(normalized_actual.len())]
                    )
                } else if normalized_expected.len() != normalized_actual.len() {
                    format!("\n  Length differs: {} vs {}", normalized_expected.len(), normalized_actual.len())
                } else {
                    String::new()
                };

                diffs.push(format!(
                    "Content differs: {}\n  Expected ({} chars): {}...\n  Actual ({} chars): {}...{}",
                    expected_file,
                    normalized_expected.len(),
                    &normalized_expected.chars().take(100).collect::<String>(),
                    normalized_actual.len(),
                    &normalized_actual.chars().take(100).collect::<String>(),
                    diff_info
                ));
            }
        }
        // Everything else (raster images) is presence-only — see
        // `is_content_compared` for why that is deliberate.
    }

    // Check for extra files (in actual but not in expected)
    for actual_file in &actual_files {
        if !expected_files.contains(actual_file) {
            diffs.push(format!("Extra file: {}", actual_file));
        }
    }

    diffs
}

/// Update expected snapshots with actual output.
/// Only run when SNAPSHOTS=overwrite is set.
fn update_snapshots(actual: &Path, expected: &Path) -> std::io::Result<()> {
    // Remove existing expected directory
    if expected.exists() {
        fs::remove_dir_all(expected)?;
    }

    // Write the SAME normalized form that `compare_directories` compares.
    copy_dir_normalized(actual, expected)?;

    Ok(())
}

/// Copy `src` to `dst` as the snapshot's stored form: the same normalization
/// `compare_directories` applies, and the same exclusions its walker applies.
///
/// Why this is not a plain `copy_dir_recursive`: comparison normalizes BOTH
/// sides and skips platform-variant paths entirely, so per-run values —
/// temp-dir UUIDs, absolute paths, LQIP base64, OG card bytes — are invisible
/// to it. Storing them raw therefore costs nothing at compare time and no test
/// can ever fail because of them, which is exactly why they accumulated: every
/// `SNAPSHOTS=overwrite` rewrote them with fresh per-run garbage and produced a
/// huge diff that had nothing to do with the change under review. Writing the
/// compared form makes a regen's diff mean something.
///
/// Two rules, both mirroring the compare side rather than inventing a second
/// policy:
///
///   - `is_platform_variant_path` files are NOT written. `_moss/og/<hash>.png`
///     is the case that matters: resvg's output is not byte-stable and the
///     filename is a content hash of it, so every regen replaced ~18 PNGs per
///     fixture that the walker then skipped. The card's existence, not its
///     bytes, is the contract.
///   - Every file that reads as UTF-8 is normalized, not just `.html`/`.css`.
///     The extension filter is a compare-side detail (only those two are
///     content-compared); on the WRITE side it left `llms.txt` and
///     `_moss/previews.json` carrying the per-run site name — the title-less
///     homepage falls back to the temp-dir folder name — where nothing ever
///     compared them and nothing ever caught it. Binary files fail
///     `read_to_string` and fall through to a byte copy.
///
/// This does NOT apply to `copy_dir_recursive`'s other caller, which stages a
/// fixture's `input/` into the temp build dir — that side is the build's
/// source and must stay byte-exact.
fn copy_dir_normalized(src: &Path, dst: &Path) -> std::io::Result<()> {
    copy_dir_normalized_rel(src, dst, Path::new(""))
}

fn copy_dir_normalized_rel(src: &Path, dst: &Path, rel: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let child_rel = rel.join(entry.file_name());
        let dst_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_normalized_rel(&path, &dst_path, &child_rel)?;
            continue;
        }
        if is_platform_variant_path(&child_rel) {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(content) => fs::write(&dst_path, normalize_html(&content))?,
            Err(_) => {
                fs::copy(&path, &dst_path)?;
            }
        }
    }
    Ok(())
}

/// `copy_dir_normalized` stores normalized text, and `compare_directories` then
/// normalizes it AGAIN before comparing. That is only sound if normalization is
/// idempotent — otherwise a stored fixture would drift a second time on read and
/// every regen would still churn, which is the bug the normalizing copy exists to
/// fix. This pins the property for each redaction rule.
#[test]
fn normalize_html_is_idempotent() {
    let raw = concat!(
        "<!-- Generated: 2026-07-28 12:00:00 -->\n",
        "<title>moss snapshot test 3f2a1b0c 4d5e 6f70 8192 a3b4c5d6e7f8</title>\n",
        "<p>/Users/someone/repo/moss/fixture.md</p>\n",
        "<p>moss_snapshot_test_3f2a1b0c-4d5e-6f70-8192-a3b4c5d6e7f8</p>\n",
        "<p>bare uuid 0e1d2c3b-4a59-6879-8a9b-0c1d2e3f4a5b</p>\n",
        "<img src=\"data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQ==\">\n",
        "<meta property=\"og:image\" content=\"https://example.com/card.png\">\n",
        "<meta name=\"twitter:card\" content=\"summary_large_image\">\n",
    );

    let once = normalize_html(raw);
    let twice = normalize_html(&once);
    assert_eq!(
        once, twice,
        "normalize_html must be a fixed point after one pass — a rule that re-matches its own \
         output would make every SNAPSHOTS=overwrite churn again"
    );

    // And it must actually have redacted, or the assertion above is vacuous.
    for leaked in [
        "2026-07-28 12:00:00",
        "/Users/someone",
        "moss_snapshot_test_3f2a1b0c",
        "0e1d2c3b-4a59-6879-8a9b-0c1d2e3f4a5b",
        "/9j/4AAQSkZJRgABAQAAAQ==",
        "og:image",
        "twitter:card",
    ] {
        assert!(!once.contains(leaked), "{leaked:?} survived normalization");
    }
}

/// Run a snapshot test for a given fixture.
fn run_snapshot_test(fixture_name: &str) {
    let fixture_dir = fixtures_dir().join(fixture_name);
    let input_dir = fixture_dir.join("input");
    let expected_dir = fixture_dir.join("expected");

    // Skip if input doesn't exist
    if !input_dir.exists() {
        eprintln!("Skipping {}: input directory not found", fixture_name);
        return;
    }

    // Create temp directory with unique name
    let temp_dir = std::env::temp_dir().join(format!("moss_snapshot_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    // Cleanup on drop
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Copy input to temp directory
    copy_dir_recursive(&input_dir, &temp_dir).expect("Failed to copy input files");

    // Run build using build_sync
    let result = build_sync(&temp_dir.to_string_lossy(), false);

    // Check build succeeded
    assert!(result.is_ok(), "Build failed for {}: {:?}", fixture_name, result);

    let output_dir = temp_dir.join(".moss/build.nosync/staging");

    // Check if we should update snapshots
    let should_update = std::env::var("SNAPSHOTS").map(|v| v == "overwrite").unwrap_or(false);

    if should_update {
        update_snapshots(&output_dir, &expected_dir)
            .expect("Failed to update snapshots");
        println!("Updated snapshots for: {}", fixture_name);
    } else if expected_dir.exists() {
        // Compare with expected
        let diffs = compare_directories(&output_dir, &expected_dir);

        if !diffs.is_empty() {
            panic!(
                "Snapshot mismatch for {}:\n{}",
                fixture_name,
                diffs.join("\n")
            );
        }
    } else {
        panic!(
            "No expected snapshots found for {}. Run with SNAPSHOTS=overwrite to generate.",
            fixture_name
        );
    }
}

// =============================================================================
// Snapshot Tests
// =============================================================================

// Two fixture sites are not included here yet.

#[test]
fn snapshot_basic_site() {
    run_snapshot_test("basic-site");
}

#[test]
fn snapshot_cascade_sort_site() {
    run_snapshot_test("cascade-sort-site");
}

#[test]
fn snapshot_collection_site() {
    run_snapshot_test("collection-site");
}

#[test]
fn snapshot_empty_site() {
    run_snapshot_test("empty-site");
}

/// A site with no home file at the root, and image directories the
/// `[editor].attachment_folder` setting names as storage.
///
/// Two things only a whole-build snapshot can see: the synthesized-homepage
/// arm of `html.rs` (no other fixture reaches it with actual content), and
/// that no `index.html` is emitted under any `assets/` directory while every
/// image variant beneath one still is.
#[test]
fn snapshot_no_home_site() {
    run_snapshot_test("no-home-site");
}

#[test]
fn snapshot_multilingual_site() {
    run_snapshot_test("multilingual-site");
}

#[test]
fn snapshot_customization_site() {
    run_snapshot_test("customization-site");
}

// Pre-existing fixture directories that were never registered as snapshot
// tests. Registering them here so the next snapshot regeneration also
// refreshes their `expected/` output. CI now diffs them on every run.
#[test]
fn snapshot_callouts_site() { run_snapshot_test("callouts-site"); }
#[test]
fn snapshot_catalog_site() { run_snapshot_test("catalog-site"); }
#[test]
fn snapshot_folder_note_site() { run_snapshot_test("folder-note-site"); }
#[test]
fn snapshot_hooks_site() { run_snapshot_test("hooks-site"); }
#[test]
fn snapshot_immersive_site() { run_snapshot_test("immersive-site"); }
#[test]
fn snapshot_media_site() { run_snapshot_test("media-site"); }
#[test]
fn snapshot_navigation_site() { run_snapshot_test("navigation-site"); }
#[test]
fn snapshot_reading_controls_site() { run_snapshot_test("reading-controls-site"); }
#[test]
fn snapshot_slots_site() { run_snapshot_test("slots-site"); }
#[test]
fn snapshot_structure_site() { run_snapshot_test("structure-site"); }
#[test]
fn snapshot_bilingual_lang_tree_site() { run_snapshot_test("bilingual-lang-tree-site"); }

#[test]
fn snapshot_flat_site() {
    run_snapshot_test("flat-site");
}

#[test]
fn snapshot_folder_embed_site() {
    // Exercises `![[/folder/|limit:N,sort:axis]]` end-to-end:
    // - first embed: Date axis (limit:3) → newest three + automatic "More →" to /journal/
    // - second embed: all five entries, no limit, no More link
    // - third embed: sort overridden to Title (alphabetical, limit:3) → More link automatic
    // More → is emitted automatically whenever limit truncates; no opt-in flag needed.
    run_snapshot_test("folder-embed-site");
}

#[test]
fn snapshot_folder_passthrough_iframe() {
    // Companion to folder-embed-site: that fixture exercises the
    // markdown-index folder LISTING branch (FolderListing); this one
    // exercises the static-index PASSTHROUGH branch end-to-end.
    //
    // `index.md` embeds `![[/app/]]` where `app/` is a passthrough subtree
    // with a source `index.html` and NO markdown index. classify_reference
    // routes it to FolderIndexIframe → try_render_folder_index_iframe, which
    // synthesizes `<iframe src="app/index.html">`. The embedding page is the
    // root index (is_index_source ⇒ true), so no pretty-URL `../` is added —
    // the src is the depth-correct, output-space (lowercase) relative path.
    run_snapshot_test("folder-passthrough-iframe");
}

/// Regression: homepage with `children_depth: all` and dateless folder
/// children but dated grandchildren. Pre-fix, axis was inferred from
/// direct children only (Title — no dates), `skip_resort=true` fired,
/// year grouping was suppressed even though the flattened corpus has
/// dated essays. Post-fix, `resolve_for_flatten` recomputes the axis
/// on the scope the reader actually sees (Date), and year-grouped
/// output appears.
#[test]
fn snapshot_flatten_year_group_site() {
    run_snapshot_test("flatten-year-group-site");
}

/// Negative pair to flatten-year-group-site: same content but no
/// `children_depth: all`. The renderer iterates direct children only
/// (a single dateless essays/ folder), so it must NOT year-group. The
/// pair locks in conditional behavior — a future refactor that makes
/// year grouping fire unconditionally fails this fixture.
#[test]
fn snapshot_flatten_no_year_group_site() {
    run_snapshot_test("flatten-no-year-group-site");
}

#[test]
fn snapshot_embed_style_grid() {
    run_snapshot_test("embed-style-grid");
}

#[test]
fn snapshot_embed_depth_all() {
    run_snapshot_test("embed-depth-all");
}

/// A single page with a `.glb` wikilink embed, on otherwise-unmodified
/// code, so the `<model-viewer>` head script is byte-pinned before the
/// head-asset plumbing that decides whether to inject it moves anywhere.
#[test]
fn snapshot_model_embed_site() {
    run_snapshot_test("model-embed-site");
}

#[test]
fn snapshot_children_limit_more() {
    run_snapshot_test("children-limit-more");
}

// =============================================================================
// Directory Structure Tests (verify file presence without content comparison)
// =============================================================================

/// Test that basic site generates expected file structure
#[test]
fn test_basic_site_structure() {
    let temp_dir = std::env::temp_dir().join(format!("moss_struct_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create minimal test content
    fs::write(temp_dir.join("index.md"), "# Home\nWelcome.").unwrap();
    fs::write(temp_dir.join("about.md"), "# About\nAbout us.").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    // Verify expected files exist
    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    assert!(site_dir.join("index.html").exists(), "index.html should exist");
    assert!(site_dir.join("about/index.html").exists(), "about/index.html should exist");
    // CSS is now emitted with a content-hash filename; verify by prefix scan.
    let has_hashed_css = fs::read_dir(site_dir.join("_moss"))
        .map(|entries| entries.flatten().any(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.starts_with("style.") && s.ends_with(".css")
        }))
        .unwrap_or(false);
    assert!(has_hashed_css, "_moss/style.<hash>.css should exist");
}

/// Test that collection site generates index for collection folder
#[test]
fn test_collection_site_structure() {
    let temp_dir = std::env::temp_dir().join(format!("moss_coll_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create collection content with index.md in the folder
    fs::write(temp_dir.join("index.md"), "# Home\nWelcome.").unwrap();
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    fs::write(
        temp_dir.join("blog/index.md"),
        "---\ntitle: Blog\n---\n# Blog\nMy blog."
    ).unwrap();
    fs::write(
        temp_dir.join("blog/post-1.md"),
        "---\ntitle: Post 1\n---\n# Post 1\nContent."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    assert!(site_dir.join("index.html").exists(), "index.html should exist");
    assert!(site_dir.join("blog/index.html").exists(), "blog/index.html should exist");
    assert!(site_dir.join("blog/post-1/index.html").exists(), "blog/post-1/index.html should exist");
}

/// Test that homepage displays collection cards via {{cards}} shortcode
#[test]
fn test_homepage_collection_cards_with_covers() {
    let temp_dir = std::env::temp_dir().join(format!("moss_card_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage with grid shortcode containing folder link
    fs::write(
        temp_dir.join("index.md"),
        "# My Site\nWelcome.\n\n:::grid\n[Blog](blog)\n:::\n"
    ).unwrap();

    // Create collection with cover image in index.md
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    fs::write(
        temp_dir.join("blog/index.md"),
        "---\ntitle: Blog\ncover: https://example.com/cover.jpg\n---\n# Blog\nMy blog posts."
    ).unwrap();
    fs::write(
        temp_dir.join("blog/post-1.md"),
        "---\ntitle: Post 1\ndate: 2024-01-15\n---\n# Post 1\nContent."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let homepage_content = fs::read_to_string(site_dir.join("index.html"))
        .expect("Failed to read index.html");

    // Should preserve the moss-grid wrapper while replacing the resolved
    // folder-link cell with a folder card. The .moss-grid[data-columns=N]
    // CSS handles layout — no swap to moss-cards[data-layout=grid].
    assert!(
        homepage_content.contains(r#"class="moss-grid""#),
        "Homepage should keep moss-grid wrapper (with its data-columns) after folder-link resolution. Got:\n{}",
        &homepage_content[..homepage_content.len().min(2000)]
    );

    // Should have moss-card elements
    assert!(
        homepage_content.contains(r#"class="moss-card""#),
        "Homepage should have moss-card elements"
    );

    // Should have cover image using <img> tag (not CSS background-image)
    assert!(
        homepage_content.contains("moss-card-cover") && homepage_content.contains("<img"),
        "Collection cards should use <img> tags for covers. Got:\n{}",
        &homepage_content[..homepage_content.len().min(8000)]
    );

    // Cover <img> should route through the synthesizer with media_lookup —
    // proven by `<picture><source srcset="…webp">` wrapping (the unconditional
    // raster-original promise added 2026-05-20). Before ea23c7543 the
    // `process_folder_links_in_grids` path emitted a bare `<img>`; after, the
    // cover goes through `render_item_with_typesetting(..., media_lookup, eager)`
    // which feeds the synthesizer. `data-placeholder-src` was removed from the
    // emitter in 6f9f260e2 — iframe-bridge now matches by URL substring against
    // `src`/`srcset`, so the test asserts the surviving shape: <picture> wrap
    // + a webp <source>.
    assert!(
        homepage_content.contains("<picture><source srcset=")
            && homepage_content.contains("cover.webp"),
        "Cover should be wrapped in <picture> with a webp <source srcset> (synthesizer with media_lookup). Got:\n{}",
        &homepage_content[..homepage_content.len().min(2000)]
    );

    // Should have title and count in card content
    assert!(
        homepage_content.contains("moss-card-content"),
        "Collection cards should have moss-card-content section"
    );
}

/// Test partial grid resolution: resolved links become collection-cards,
/// unresolved links stay as plain moss-grid-card cells.
#[test]
fn test_collection_requires_index_md() {
    let temp_dir = std::env::temp_dir().join(format!("moss_reqidx_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Grid with two links: blog (has index.md) and articles (no index.md).
    // Both resolve as collection-cards because articles/ gets a synthetic
    // index entry in the page tree (auto-generated folder index).
    fs::write(
        temp_dir.join("index.md"),
        "# My Site\nWelcome.\n\n:::grid\n[Blog](blog)\n---\n[Articles](articles)\n:::\n"
    ).unwrap();

    // Create a folder WITH index.md
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    fs::write(
        temp_dir.join("blog/index.md"),
        "---\ntitle: Blog\n---\n# Blog\nMy blog posts."
    ).unwrap();
    fs::write(
        temp_dir.join("blog/post-1.md"),
        "---\ntitle: Blog Post 1\ndate: 2024-01-15\n---\n# Post 1\nContent."
    ).unwrap();

    // Create a folder WITHOUT index.md — still resolves as a collection
    // because the pipeline synthesizes an index entry for it.
    fs::create_dir_all(temp_dir.join("articles")).unwrap();
    fs::write(
        temp_dir.join("articles/article-1.md"),
        "---\ntitle: Article 1\ndate: 2024-01-10\n---\n# Article 1\nContent."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let homepage_content = fs::read_to_string(site_dir.join("index.html"))
        .expect("Failed to read index.html");

    // Both links resolve to folder cards within the preserved moss-grid wrapper.
    // The .moss-grid[data-columns=N] CSS handles layout — no swap to moss-cards.
    assert!(
        homepage_content.contains(r#"class="moss-grid""#),
        "Should keep moss-grid wrapper when all links resolve (articles/ has synthetic index). Got:\n{}",
        &homepage_content[..homepage_content.len().min(2000)]
    );

    // Both links should become moss-card elements
    assert!(
        homepage_content.contains(r#"class="moss-card""#),
        "Both links should become moss-card elements. Got:\n{}",
        &homepage_content[..homepage_content.len().min(2000)]
    );

    // Articles folder gets auto-generated index.html, so articles appear there
    // (not on the homepage). Verify the auto-generated index exists.
    let articles_index = site_dir.join("articles/index.html");
    assert!(
        articles_index.exists(),
        "articles/ should get auto-generated index.html"
    );
    let articles_content = fs::read_to_string(&articles_index)
        .expect("Failed to read articles/index.html");
    assert!(
        articles_content.contains("Article 1"),
        "Article 1 should appear in auto-generated articles index"
    );
}

/// Test that collection index pages show their article list
#[test]
fn test_collection_index_shows_article_list() {
    let temp_dir = std::env::temp_dir().join(format!("moss_collidx_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage
    fs::write(temp_dir.join("index.md"), "# My Site\nWelcome.").unwrap();

    // Create collection with multiple articles
    fs::create_dir_all(temp_dir.join("tutorials")).unwrap();
    fs::write(
        temp_dir.join("tutorials/index.md"),
        "---\ntitle: Tutorials\n---\n# Tutorials\nLearn something new."
    ).unwrap();
    fs::write(
        temp_dir.join("tutorials/getting-started.md"),
        "---\ntitle: Getting Started\ndate: 2024-01-10\n---\n# Getting Started\nFirst steps."
    ).unwrap();
    fs::write(
        temp_dir.join("tutorials/advanced.md"),
        "---\ntitle: Advanced Topics\ndate: 2024-01-15\n---\n# Advanced Topics\nDeep dive."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let collection_index = fs::read_to_string(site_dir.join("tutorials/index.html"))
        .expect("Failed to read tutorials/index.html");

    // Collection index should show its articles
    assert!(
        collection_index.contains("Getting Started"),
        "Collection index should list 'Getting Started' article. Got:\n{}",
        &collection_index
    );
    assert!(
        collection_index.contains("Advanced Topics"),
        "Collection index should list 'Advanced Topics' article"
    );

    // Should have moss-cards structure (v1)
    assert!(
        collection_index.contains(r#"data-layout="list""#) || collection_index.contains(r#"data-layout="minimal""#),
        "Collection index should have article listing structure"
    );
}

/// Test that article links in collection index have correct relative paths
/// Bug: Links were all pointing to "index.html" instead of "article-slug/index.html"
#[test]
fn test_collection_index_article_links_have_correct_href() {
    let temp_dir = std::env::temp_dir().join(format!("moss_href_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage
    fs::write(temp_dir.join("index.md"), "# My Site\nWelcome.").unwrap();

    // Create collection with multiple articles
    fs::create_dir_all(temp_dir.join("tutorials")).unwrap();
    fs::write(
        temp_dir.join("tutorials/index.md"),
        "---\ntitle: Tutorials\n---\n# Tutorials\nLearn something new."
    ).unwrap();
    fs::write(
        temp_dir.join("tutorials/getting-started.md"),
        "---\ntitle: Getting Started\ndate: 2024-01-10\n---\n# Getting Started\nFirst steps."
    ).unwrap();
    fs::write(
        temp_dir.join("tutorials/advanced.md"),
        "---\ntitle: Advanced Topics\ndate: 2024-01-15\n---\n# Advanced Topics\nDeep dive."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let collection_index = fs::read_to_string(site_dir.join("tutorials/index.html"))
        .expect("Failed to read tutorials/index.html");

    // Article links should use pretty URLs (directory paths without index.html)
    // generate_children uses depth-aware PathResolver, so links are root-relative via "../"
    assert!(
        collection_index.contains(r#"tutorials/getting-started/"#),
        "Article link should contain 'tutorials/getting-started/' path. Got:\n{}",
        &collection_index
    );
    assert!(
        collection_index.contains(r#"tutorials/advanced/"#),
        "Article link should contain 'tutorials/advanced/' path. Got:\n{}",
        &collection_index
    );

    // Verify it does NOT have the buggy pattern where all links point to just "index.html"
    // Count occurrences of href="index.html" (without path prefix)
    let buggy_pattern = r#"href="index.html""#;
    let buggy_count = collection_index.matches(buggy_pattern).count();
    assert!(
        buggy_count == 0,
        "Found {} occurrences of buggy href=\"index.html\" pattern (should be 0). Links should have article folder prefix.",
        buggy_count
    );
}

/// Test that flatten: true lists all descendants recursively
#[test]
fn test_flatten_lists_all_descendants() {
    let temp_dir = std::env::temp_dir().join(format!("moss_flatten_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage with children_depth: all (flattens all descendants)
    fs::write(
        temp_dir.join("index.md"),
        "---\ntitle: My Site\nchildren_depth: all\n---\n# My Site\nAll articles."
    ).unwrap();

    // Create nested folder structure
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    fs::write(
        temp_dir.join("blog/index.md"),
        "---\ntitle: Blog\n---\n# Blog"
    ).unwrap();
    fs::write(
        temp_dir.join("blog/post-1.md"),
        "---\ntitle: Blog Post 1\ndate: 2024-01-15\n---\n# Post 1\nContent."
    ).unwrap();

    fs::create_dir_all(temp_dir.join("tutorials")).unwrap();
    fs::write(
        temp_dir.join("tutorials/index.md"),
        "---\ntitle: Tutorials\n---\n# Tutorials"
    ).unwrap();
    fs::write(
        temp_dir.join("tutorials/intro.md"),
        "---\ntitle: Intro Tutorial\ndate: 2024-01-10\n---\n# Intro\nContent."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let homepage = fs::read_to_string(site_dir.join("index.html"))
        .expect("Failed to read index.html");

    // With flatten: true, homepage should list articles from ALL subfolders
    assert!(
        homepage.contains("Blog Post 1"),
        "Flattened homepage should list 'Blog Post 1' from blog/. Got:\n{}",
        &homepage[..homepage.len().min(3000)]
    );
    assert!(
        homepage.contains("Intro Tutorial"),
        "Flattened homepage should list 'Intro Tutorial' from tutorials/"
    );
}

/// Test that without flatten, a folder index only lists direct children
#[test]
fn test_no_flatten_only_direct_children() {
    let temp_dir = std::env::temp_dir().join(format!("moss_noflatten_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create folder with nested structure (no flatten)
    fs::write(temp_dir.join("index.md"), "# Home").unwrap();
    fs::create_dir_all(temp_dir.join("articles")).unwrap();
    fs::write(
        temp_dir.join("articles/index.md"),
        "---\ntitle: Articles\n---\n# Articles"
    ).unwrap();
    fs::write(
        temp_dir.join("articles/direct-child.md"),
        "---\ntitle: Direct Child\ndate: 2024-01-15\n---\n# Direct\nContent."
    ).unwrap();

    // Nested subfolder
    fs::create_dir_all(temp_dir.join("articles/sub")).unwrap();
    fs::write(
        temp_dir.join("articles/sub/index.md"),
        "---\ntitle: Sub\n---\n# Sub"
    ).unwrap();
    fs::write(
        temp_dir.join("articles/sub/nested-article.md"),
        "---\ntitle: Nested Article\ndate: 2024-01-10\n---\n# Nested\nContent."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let articles_index = fs::read_to_string(site_dir.join("articles/index.html"))
        .expect("Failed to read articles/index.html");

    // Should list direct child
    assert!(
        articles_index.contains("Direct Child"),
        "Folder index should list direct child 'Direct Child'. Got:\n{}",
        &articles_index[..articles_index.len().min(3000)]
    );

    // Should NOT list nested article (it's in a subfolder)
    assert!(
        !articles_index.contains("Nested Article"),
        "Folder index without flatten should NOT list nested 'Nested Article'"
    );
}

/// Test that breadcrumbs auto-enable when no nav items exist.
/// A site with only folders (no root-level .md besides index.md) has no nav items,
/// so breadcrumbs should auto-enable for nested pages.
#[test]
fn test_breadcrumbs_auto_enable_when_no_nav_items() {
    let temp_dir = std::env::temp_dir().join(format!("moss_bread_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    fs::write(temp_dir.join("index.md"), "# Home").unwrap();
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    fs::write(
        temp_dir.join("blog/index.md"),
        "---\ntitle: Blog\n---\n# Blog"
    ).unwrap();
    fs::write(
        temp_dir.join("blog/my-post.md"),
        "---\ntitle: My Post\ndate: 2024-01-15\n---\n# My Post\nContent."
    ).unwrap();
    // A folder with no index.md: its index page is synthesized by the
    // render loop, which carried its own breadcrumb rule until 2026-09-05
    // (explicit `breadcrumb: true` only) — so on a nav-less site the
    // article had a trail and the folder between it and home did not.
    fs::create_dir_all(temp_dir.join("notes")).unwrap();
    fs::write(temp_dir.join("notes/a-note.md"), "---\ntitle: A Note\n---\ntext").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // With no nav items, breadcrumbs should auto-enable for nested pages
    let article = fs::read_to_string(site_dir.join("blog/my-post/index.html"))
        .expect("Failed to read blog/my-post/index.html");
    assert!(article.contains("breadcrumb"), "Nested article should have breadcrumb when no nav items");
    // …and for synthesized folder indexes by the same rule. A trail's crumbs
    // carry `data-trail-crumb`; the plain site name does not.
    let folder = fs::read_to_string(site_dir.join("notes/index.html"))
        .expect("Failed to read notes/index.html");
    assert!(
        folder.contains("data-trail-crumb"),
        "Synthesized folder index should have the same auto-enabled breadcrumb as its articles"
    );
}

/// Test that relative image paths are adjusted for pretty URLs
#[test]
fn test_image_paths_adjusted_for_pretty_urls() {
    let temp_dir = std::env::temp_dir().join(format!("moss_imgpath_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    fs::write(temp_dir.join("index.md"), "# Home").unwrap();
    fs::create_dir_all(temp_dir.join("blog")).unwrap();
    // Article with relative image path
    fs::write(
        temp_dir.join("blog/my-post.md"),
        "---\ntitle: My Post\n---\n# My Post\n\n![photo](../../assets/photo.jpg)\n\n![abs](/assets/abs.jpg)\n\n![ext](https://example.com/img.jpg)"
    ).unwrap();

    // Create the image asset
    fs::create_dir_all(temp_dir.join("assets")).unwrap();
    fs::write(temp_dir.join("assets/photo.jpg"), "fake-image").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let article = fs::read_to_string(site_dir.join("blog/my-post/index.html"))
        .expect("Failed to read article");

    // Pretty URL adds one directory level: blog/my-post.md → blog/my-post/index.html
    // So ../../assets/photo.jpg should become ../../../assets/photo.jpg
    assert!(
        article.contains("../../../assets/photo.jpg"),
        "Relative image path should be adjusted by one level for pretty URLs. Got:\n{}",
        &article[..article.len().min(3000)]
    );

    // Absolute paths should be unchanged
    assert!(
        article.contains("/assets/abs.jpg"),
        "Absolute paths should be unchanged"
    );

    // External URLs should be unchanged
    assert!(
        article.contains("https://example.com/img.jpg"),
        "External URLs should be unchanged"
    );
}

// Series navigation tests removed: classification system was removed.
// Series navigation will be re-added using the page tree in a future PR.

/// Test that CSS has no default underlines on links
#[test]
fn test_css_no_default_link_underlines() {
    let temp_dir = std::env::temp_dir().join(format!("moss_csstest_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create minimal site
    fs::write(temp_dir.join("index.md"), "# Test\nContent.").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    // CSS is now emitted with a content-hash filename; find it by prefix scan.
    let moss_dir = temp_dir.join(".moss/build.nosync/staging/_moss");
    let hashed_css_path = fs::read_dir(&moss_dir)
        .expect("_moss dir must exist")
        .flatten()
        .find(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.starts_with("style.") && s.ends_with(".css")
        })
        .map(|e| e.path())
        .expect("_moss/style.<hash>.css must exist");
    let css_content = fs::read_to_string(&hashed_css_path)
        .expect("Failed to read hashed style.css");

    // The global `a` rule should have text-decoration:none (not underline).
    // CSS may be minified so check for the minified form.
    assert!(
        css_content.contains("text-decoration:none"),
        "Global link style should have text-decoration: none. Got first 500 chars:\n{}",
        &css_content[..css_content.len().min(500)]
    );
}

/// Test that articles in a folder with `order` get prev/next series navigation
#[test]
fn test_series_navigation_with_order() {
    let temp_dir = std::env::temp_dir().join(format!("moss_series_nav_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage
    fs::write(temp_dir.join("index.md"), "---\ntitle: My Site\n---\n# My Site").unwrap();

    // Create series folder with series (explicit order)
    fs::create_dir_all(temp_dir.join("series")).unwrap();
    fs::write(
        temp_dir.join("series/index.md"),
        "---\ntitle: My Series\nseries:\n  - part-1\n  - part-2\n  - part-3\n---\nA three-part series."
    ).unwrap();

    fs::write(
        temp_dir.join("series/part-1.md"),
        "---\ntitle: Part One\ndate: 2024-01-01\n---\nFirst part."
    ).unwrap();
    fs::write(
        temp_dir.join("series/part-2.md"),
        "---\ntitle: Part Two\ndate: 2024-01-02\n---\nSecond part."
    ).unwrap();
    fs::write(
        temp_dir.join("series/part-3.md"),
        "---\ntitle: Part Three\ndate: 2024-01-03\n---\nThird part."
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // Part 1 (first): should have next but no prev
    let part1 = fs::read_to_string(site_dir.join("series/part-1/index.html"))
        .expect("Failed to read part-1");
    assert!(
        part1.contains("moss-series-nav"),
        "Part 1 should have series navigation. Got:\n{}",
        &part1[..part1.len().min(3000)]
    );
    assert!(
        part1.contains("Part Two"),
        "Part 1 should have 'next' link to Part Two"
    );
    // With the two-row layout, empty placeholders are used for proper flexbox alignment
    // Check that prev is an empty placeholder (not an actual link)
    assert!(
        part1.contains(r#"series-nav-prev empty"#),
        "Part 1 (first article) should have empty prev placeholder for layout"
    );

    // Part 2 (middle): should have both prev and next
    let part2 = fs::read_to_string(site_dir.join("series/part-2/index.html"))
        .expect("Failed to read part-2");
    assert!(
        part2.contains("Part One"),
        "Part 2 should have 'prev' link to Part One"
    );
    assert!(
        part2.contains("Part Three"),
        "Part 2 should have 'next' link to Part Three"
    );

    // Part 3 (last): should have prev but no next
    let part3 = fs::read_to_string(site_dir.join("series/part-3/index.html"))
        .expect("Failed to read part-3");
    assert!(
        part3.contains("Part Two"),
        "Part 3 should have 'prev' link to Part Two"
    );
    // With the two-row layout, empty placeholders are used for proper flexbox alignment
    // Check that next is an empty placeholder (not an actual link)
    assert!(
        part3.contains(r#"series-nav-next empty"#),
        "Part 3 (last article) should have empty next placeholder for layout"
    );

    // Series name should appear in nav
    assert!(
        part2.contains("My Series"),
        "Series nav should show the series name 'My Series'"
    );
}

/// Regression test: when both comments and series-nav render on an article
/// page, the comment section must appear BEFORE series-nav in DOM order so
/// the comment toggle hugs series-nav's `border-top` divider (not the
/// footer's). See `.moss-comments + .moss-series-nav` rule in site.css and
/// the `{post_article}` placement in article.html.
#[test]
fn test_comments_precede_series_nav_in_dom_order() {
    let temp_dir = std::env::temp_dir().join(format!(
        "moss_comments_before_series_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    fs::write(temp_dir.join("index.md"), "---\ntitle: My Site\n---\n# My Site").unwrap();

    fs::create_dir_all(temp_dir.join("series")).unwrap();
    fs::write(
        temp_dir.join("series/index.md"),
        "---\ntitle: My Series\nseries:\n  - part-1\n  - part-2\n  - part-3\n---\nA three-part series."
    ).unwrap();
    fs::write(
        temp_dir.join("series/part-1.md"),
        "---\ntitle: Part One\n---\nFirst part."
    ).unwrap();
    fs::write(
        temp_dir.join("series/part-2.md"),
        "---\ntitle: Part Two\n---\nSecond part."
    ).unwrap();
    fs::write(
        temp_dir.join("series/part-3.md"),
        "---\ntitle: Part Three\n---\nThird part."
    ).unwrap();

    // Enable native comments rendering via [services.comments]. This is the
    // path that emits `<section class="moss-comments">` (the Waline-style
    // toggle), which is what the new spacing/divider CSS targets. Moss-host
    // Artalk emits a different `id="artalk-comments"` widget that doesn't
    // exercise the layout under test.
    fs::create_dir_all(temp_dir.join(".moss")).unwrap();
    fs::write(
        temp_dir.join(".moss/config.toml"),
        "[services.comments]\nprovider = \"waline\"\nserver_url = \"https://example.com/comments\"\n",
    )
    .unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");
    let part2 =
        fs::read_to_string(site_dir.join("series/part-2/index.html")).expect("read part-2");

    let comments_idx = part2
        .find("class=\"moss-comments\"")
        .expect("part-2 should render moss-comments section");
    let series_idx = part2
        .find("class=\"moss-series-nav\"")
        .expect("part-2 should render moss-series-nav");

    assert!(
        comments_idx < series_idx,
        "moss-comments must precede moss-series-nav in DOM order so the \
         comment toggle hugs series-nav's border-top divider. \
         comments_idx={}, series_idx={}",
        comments_idx,
        series_idx
    );

    // Both must be siblings in <main>, not nested inside <article>: the </article>
    // closing tag must come before the moss-series-nav opening.
    let article_close_idx = part2
        .rfind("</article>")
        .expect("part-2 should have </article>");
    assert!(
        article_close_idx < series_idx,
        "moss-series-nav must be a sibling of <article>, not nested inside. \
         article_close_idx={}, series_idx={}",
        article_close_idx,
        series_idx
    );
}

/// Regression test: series navigation must work when order entries are Obsidian
/// wikilinks whose titles contain punctuation (e.g. commas). Obsidian resolves
/// `[[Title]]` by filename, and `frontmatter_ref_to_stem` extracts the raw title.
/// The render pipeline must match by filename (clean_stem), not URL slug,
/// because `generate_slug()` converts punctuation to hyphens.
#[test]
fn test_series_navigation_with_wikilink_punctuation_titles() {
    let temp_dir = std::env::temp_dir().join(format!(
        "moss_series_punct_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create homepage
    fs::write(
        temp_dir.join("index.md"),
        "---\ntitle: My Site\n---\n# My Site",
    )
    .unwrap();

    // Create series folder with wikilink series entries containing punctuation
    fs::create_dir_all(temp_dir.join("travel")).unwrap();
    fs::write(
        temp_dir.join("travel/index.md"),
        "---\ntitle: Travel Series\nseries:\n  - \"[[Departure]]\"\n  - \"[[Rivers, Mountains, and Freedom]]\"\n  - \"[[The Real Journey]]\"\n---\nA travel series.",
    )
    .unwrap();

    // Filenames match wikilink targets exactly (Obsidian convention)
    fs::write(
        temp_dir.join("travel/Departure.md"),
        "---\ntitle: Departure\ndate: 2024-01-01\n---\nFirst article.",
    )
    .unwrap();
    fs::write(
        temp_dir.join("travel/Rivers, Mountains, and Freedom.md"),
        "---\ntitle: Rivers, Mountains, and Freedom\ndate: 2024-01-02\n---\nSecond article.",
    )
    .unwrap();
    fs::write(
        temp_dir.join("travel/The Real Journey.md"),
        "---\ntitle: The Real Journey\ndate: 2024-01-03\n---\nThird article.",
    )
    .unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // The comma in "Rivers, Mountains, and Freedom" becomes a hyphen in the slug:
    // "rivers-mountains-and-freedom"
    let art2 = fs::read_to_string(
        site_dir.join("travel/rivers-mountains-and-freedom/index.html"),
    )
    .expect("Failed to read article with punctuation in filename");

    assert!(
        art2.contains("moss-series-nav"),
        "Article with punctuation in filename should have series navigation.\nGot:\n{}",
        &art2[..art2.len().min(3000)]
    );
    assert!(
        art2.contains("Departure"),
        "Middle article should have 'prev' link to Departure"
    );
    assert!(
        art2.contains("The Real Journey"),
        "Middle article should have 'next' link to The Real Journey"
    );

    // Also verify first and last articles get series nav
    let art1 = fs::read_to_string(site_dir.join("travel/departure/index.html"))
        .expect("Failed to read Departure");
    assert!(
        art1.contains("moss-series-nav"),
        "First article should have series navigation"
    );

    let art3 = fs::read_to_string(
        site_dir.join("travel/the-real-journey/index.html"),
    )
    .expect("Failed to read The Real Journey");
    assert!(
        art3.contains("moss-series-nav"),
        "Last article should have series navigation"
    );
}

/// Regression test: an explicit `order:` list on a collection's folder index
/// must control the rendered child order, EVEN WHEN the children carry `date:`
/// frontmatter (which makes the inferred sort axis `Date`). Two bugs used to
/// break this for real Obsidian vaults:
///   1. The list-rendering layer re-sorted children by date-descending whenever
///      the axis was `Date`, discarding the explicit order entirely.
///   2. `order:`/`sort:` entries written as `[[Wikilinks]]` were never stripped
///      of their brackets, so they never matched the child filename stems.
///
/// The explicit order below (Gamma, Rivers, Alpha) is deliberately distinct from
/// date-descending (Rivers, Gamma, Alpha), date-ascending (Alpha, Gamma, Rivers),
/// and alphabetical (Alpha, Gamma, Rivers) — so a pass uniquely proves the
/// explicit list won, not a coincidental axis sort. One entry carries a comma to
/// exercise punctuation handling on the order/sort path (slug != filename stem).
#[test]
fn test_collection_explicit_order_wins_over_date_axis() {
    let temp_dir = std::env::temp_dir().join(format!(
        "moss_explicit_order_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    fs::write(temp_dir.join("index.md"), "---\ntitle: My Site\n---\n# My Site").unwrap();

    // Collection folder index with an explicit `order:` of `[[Wikilinks]]`.
    fs::create_dir_all(temp_dir.join("travel")).unwrap();
    fs::write(
        temp_dir.join("travel/index.md"),
        "---\ntitle: Travel\norder:\n  - \"[[Gamma]]\"\n  - \"[[Rivers, Mountains]]\"\n  - \"[[Alpha]]\"\n---\nA travel collection.",
    )
    .unwrap();

    // All children dated -> inferred axis is Date (triggers the date re-sort bug).
    fs::write(
        temp_dir.join("travel/Gamma.md"),
        "---\ntitle: Gamma\ndate: 2024-02-01\n---\nGamma.",
    )
    .unwrap();
    fs::write(
        temp_dir.join("travel/Rivers, Mountains.md"),
        "---\ntitle: Rivers, Mountains\ndate: 2024-03-01\n---\nRivers.",
    )
    .unwrap();
    fs::write(
        temp_dir.join("travel/Alpha.md"),
        "---\ntitle: Alpha\ndate: 2024-01-01\n---\nAlpha.",
    )
    .unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let collection = fs::read_to_string(
        temp_dir.join(".moss/build.nosync/staging/travel/index.html"),
    )
    .expect("Failed to read collection index page");

    // Locate each child by its (unique) URL slug in document order.
    let pos_gamma = collection
        .find("travel/gamma/")
        .expect("collection page should link to gamma");
    let pos_rivers = collection
        .find("travel/rivers-mountains/")
        .expect("collection page should link to rivers-mountains (comma -> hyphen slug)");
    let pos_alpha = collection
        .find("travel/alpha/")
        .expect("collection page should link to alpha");

    assert!(
        pos_gamma < pos_rivers && pos_rivers < pos_alpha,
        "Children must render in the explicit `order:` (Gamma, Rivers, Alpha), \
         not date-descending. Got positions gamma={}, rivers={}, alpha={}.\n\nPage:\n{}",
        pos_gamma,
        pos_rivers,
        pos_alpha,
        &collection[..collection.len().min(4000)]
    );
}

// =============================================================================
// Default Favicon Tests
// =============================================================================

/// When a site has no assets/favicon.svg, moss should write the default moss logo
/// and include a <link> tag in the generated HTML.
#[test]
fn test_default_favicon_when_no_user_favicon() {
    let temp_dir = std::env::temp_dir().join(format!("moss_favicon_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create minimal site with NO favicon
    fs::write(temp_dir.join("index.md"), "# Home\nWelcome.").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // Default favicon file should exist in output
    let favicon_path = site_dir.join("assets/favicon.svg");
    assert!(
        favicon_path.exists(),
        "Default favicon should be written to assets/favicon.svg when user has none"
    );

    // The fallback writes the mark with its viewBox tightened to the tab-only
    // ink bbox (moss's 2026-09-14 fix) — not icon.svg verbatim.
    let favicon_content = fs::read_to_string(&favicon_path).unwrap();
    assert_eq!(
        favicon_content,
        moss_build::build::site_meta::favicon::tighten_default_favicon_viewbox(MOSS_MARK),
        "Default favicon should be icons/icon.svg with the tightened viewBox"
    );

    // HTML should contain a <link> tag for the favicon
    let index_html = fs::read_to_string(site_dir.join("index.html")).unwrap();
    assert!(
        index_html.contains(r#"<link rel="icon" type="image/svg+xml" href="/assets/favicon.svg">"#),
        "Homepage HTML should contain favicon link tag"
    );

    // The raster favicons are a separate step (generate_favicons) that usvg can
    // fail on, and blocking.rs is deliberately fail-open about it -- so a build
    // that only checks the .svg above can stay green while the PNGs and their
    // <link> tags silently disappear. Assert the rasters exist too.
    for raster in ["favicon-16.png", "favicon-32.png", "favicon-180.png"] {
        assert!(
            site_dir.join("assets").join(raster).exists(),
            "Default build should also write assets/{raster}"
        );
    }
    assert!(
        index_html.contains(r#"<link rel="icon" type="image/png" sizes="32x32" href="/assets/favicon-32.png">"#),
        "Homepage HTML should link the 32x32 favicon raster"
    );
    assert!(
        index_html.contains(r#"<link rel="icon" type="image/png" sizes="16x16" href="/assets/favicon-16.png">"#),
        "Homepage HTML should link the 16x16 favicon raster"
    );
    assert!(
        index_html.contains(r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/favicon-180.png">"#),
        "Homepage HTML should link the 180x180 apple-touch-icon raster"
    );
}

/// When a site has a user-provided assets/favicon.svg, moss should use that instead
/// of the default.
#[test]
fn test_user_favicon_takes_precedence() {
    let temp_dir = std::env::temp_dir().join(format!("moss_favicon_user_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create site WITH a custom favicon
    fs::write(temp_dir.join("index.md"), "# Home\nWelcome.").unwrap();
    fs::create_dir_all(temp_dir.join("assets")).unwrap();
    fs::write(
        temp_dir.join("assets/favicon.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="10" fill="red"/></svg>"#,
    ).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // Favicon should exist
    let favicon_path = site_dir.join("assets/favicon.svg");
    assert!(favicon_path.exists(), "Favicon should exist in output");

    // Should be the USER's favicon, not the moss default
    let favicon_content = fs::read_to_string(&favicon_path).unwrap();
    assert!(
        favicon_content.contains("red"),
        "Output favicon should be the user's custom favicon, not the moss default"
    );
    assert_ne!(
        favicon_content, MOSS_MARK,
        "Output favicon should NOT be the moss default mark"
    );

    // HTML should contain the favicon link tag
    let index_html = fs::read_to_string(site_dir.join("index.html")).unwrap();
    assert!(
        index_html.contains(r#"<link rel="icon" type="image/svg+xml" href="/assets/favicon.svg">"#),
        "HTML should contain favicon link tag for user-provided favicon"
    );
}

/// Favicon link tag should use root-relative paths for all pages including nested articles.
#[test]
fn test_default_favicon_root_relative_paths() {
    let temp_dir = std::env::temp_dir().join(format!("moss_favicon_depth_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    // Create site with a nested article but NO favicon
    fs::write(temp_dir.join("index.md"), "# Home\nWelcome.").unwrap();
    fs::write(temp_dir.join("about.md"), "---\ntitle: About\n---\n# About\nAbout us.").unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // Nested article should have root-relative favicon path
    let about_html = fs::read_to_string(site_dir.join("about/index.html")).unwrap();
    assert!(
        about_html.contains(r#"<link rel="icon" type="image/svg+xml" href="/assets/favicon.svg">"#),
        "Nested article should have root-relative favicon path /assets/favicon.svg"
    );
}

// =============================================================================
// Images-are-not-pages Tests
// =============================================================================

/// Images are static assets, never pages of their own — a gallery is explicit
/// (the media-collection marker), never inferred from "folder of images".
#[test]
fn test_images_not_promoted_when_md_children_exist() {
    let temp_dir = std::env::temp_dir().join(format!("moss_no_image_pages_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).expect("Failed to create temp directory");

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(temp_dir.clone());

    fs::write(temp_dir.join("index.md"), "---\ntitle: Blog\nuid: bb000001\n---\n# Blog\n").unwrap();

    let posts_dir = temp_dir.join("posts");
    fs::create_dir_all(&posts_dir).unwrap();
    fs::write(posts_dir.join("posts.md"), "---\ntitle: Posts\nuid: bb000002\n---\n").unwrap();
    // This folder has a .md child article → images should NOT become pages
    fs::write(posts_dir.join("first-post.md"), "---\ntitle: First Post\nuid: bb000003\n---\nMy post.\n").unwrap();

    // Also add an image — it should stay as a static asset, NOT a page
    let jpeg: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xD9]; // Minimal JPEG stub
    fs::write(posts_dir.join("photo.jpg"), &jpeg).unwrap();

    let result = build_sync(&temp_dir.to_string_lossy(), false);
    assert!(result.is_ok(), "Build failed: {:?}", result);

    let site_dir = temp_dir.join(".moss/build.nosync/staging");

    // The first-post should exist as a page
    assert!(
        site_dir.join("posts/first-post/index.html").exists(),
        "Markdown child should generate a page"
    );

    // The image should NOT have generated a page
    assert!(
        !site_dir.join("posts/photo/index.html").exists(),
        "Image should NOT generate a page when folder has .md children"
    );
}
