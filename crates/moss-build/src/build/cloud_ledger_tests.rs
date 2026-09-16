use super::*;

/// A unique root per test — the store is process-global and the test
/// harness runs these in parallel.
fn root(tag: &str) -> PathBuf {
    // A temp directory, not an asset URL. (This file predates the lint: it
    // was written before the revert that deleted it, and restored after.)
    PathBuf::from(format!("/tmp/moss-cloud-ledger-test/{tag}")) // allow:served-path-url-construct
}

#[test]
fn notes_are_scoped_to_their_root() {
    let a = root("scoped-a");
    let b = root("scoped-b");
    note_unavailable(&a.join("one.jpg"));
    note_unavailable(&b.join("two.jpg"));

    assert_eq!(noted_under(&a), vec![a.join("one.jpg")]);
    assert_eq!(noted_under(&b), vec![b.join("two.jpg")]);
}

#[test]
fn begin_build_clears_only_its_own_root() {
    let a = root("clear-a");
    let b = root("clear-b");
    note_unavailable(&a.join("one.jpg"));
    note_unavailable(&b.join("two.jpg"));

    begin_build(&a);

    assert!(noted_under(&a).is_empty(), "the rebuilt root starts fresh");
    assert_eq!(
        noted_under(&b),
        vec![b.join("two.jpg")],
        "a concurrently-open folder's record must survive"
    );
}

#[test]
fn noting_the_same_path_twice_counts_once() {
    let r = root("idempotent");
    note_unavailable(&r.join("one.jpg"));
    note_unavailable(&r.join("one.jpg"));
    assert_eq!(noted_under(&r).len(), 1);
}

/// `outstanding` filters on `is_still_in_the_cloud`, which is `false` off
/// macOS and `false` for any path that does not exist. So on CI this is
/// zero for a recorded-but-absent path — which is the honest answer: the
/// gate must not hold over a file nothing is waiting for.
#[test]
fn outstanding_excludes_paths_that_are_no_longer_in_the_cloud() {
    let r = root("outstanding");
    note_unavailable(&r.join("gone.jpg"));
    assert_eq!(outstanding(&r), 0);
    assert_eq!(noted_under(&r).len(), 1, "still recorded, just not awaited");
}

/// The publish split. Media degrades to a placeholder and the site is still
/// the user's site; a page source does not, so its absence is what must
/// hold a publish back.
#[test]
fn page_sources_and_config_are_structural_media_is_not() {
    for structural in [
        "index.md",
        "index.markdown",
        "post.docx",
        "post.PAGES",
        "page.html",
        "config.toml",
        ".moss/theme/style.css",
        "notebook.ipynb",
    ] {
        assert!(
            is_structural_source(Path::new(structural)),
            "{structural} decides the site"
        );
    }
    for decorative in [
        "a.jpg", "a.JPEG", "a.png", "a.webp", "a.svg", "a.mp4", "a.MOV",
    ] {
        assert!(
            !is_structural_source(Path::new(decorative)),
            "{decorative} is decoration"
        );
    }
}

/// The property that keeps the publish gate clearable. A vault holds files
/// moss never reads, the provider evicts them like anything else, and it
/// will not download one nothing opens — so if an unknown extension counted
/// as structure, a single `.zip` would withhold every publish forever and
/// leave the user with no site at all.
#[test]
fn a_file_moss_never_reads_is_not_structural() {
    for inert in [
        "archive.zip",
        "art.psd",
        "deck.key",
        "notes.txt",
        "Makefile",
        "README",
    ] {
        assert!(
            !is_structural_source(Path::new(inert)),
            "{inert} must not hold a publish"
        );
    }
}

/// Either half alone is enough — they see different windows in time. The scan
/// predates the build; the ledger only holds what a read site reached. The
/// scan-only case is the blueprint-grid shape moss#1042 reported: every page
/// source was dataless when the walk ran, so no document reached the render
/// pass, no `read_page_source` ever ran, and the ledger is empty.
///
/// And the COUNT is the decision's own, not a later re-query. moss#1061: the
/// log explaining a withheld publish re-read `structural_outstanding()` while
/// the decision had been made from the scan's list, so a build withheld because
/// every page source was dataless printed "0 structural source(s) are still
/// downloading" — a sentence contradicting itself, in the one line an operator
/// reads to find out why the preview did not move.
#[test]
fn either_half_alone_reports_a_structural_gap_and_says_how_many() {
    let scan_only = [std::path::PathBuf::from("/v/index.md")];
    assert_eq!(structural_missing_count(&[], 1), 1, "ledger only");
    assert_eq!(
        structural_missing_count(&scan_only, 0),
        1,
        "scan only — the half a re-query of the ledger reports as zero"
    );
    assert_eq!(
        structural_missing_count(&[], 0),
        0,
        "neither — the fully-local case"
    );

    // Media never counts, however much of it is still arriving (ADR-013).
    let mixed = [
        std::path::PathBuf::from("/v/index.md"),
        std::path::PathBuf::from("/v/about.md"),
        std::path::PathBuf::from("/v/photo.jpg"),
    ];
    assert_eq!(structural_missing_count(&mixed, 0), 2);

    // Two overlapping views of one set, combined the way `cloud_outstanding`
    // combines its own two: the larger, never the sum, so a source both halves
    // saw is not counted twice.
    assert_eq!(structural_missing_count(&mixed, 1), 2);
}

/// `structural_outstanding` is `outstanding` restricted to that list. Both
/// apply the `is_still_in_the_cloud` filter, which is `false` on CI for a
/// path that does not exist — so this asserts the *classification*, via
/// `noted_under`, rather than a count that is zero everywhere off macOS.
#[test]
fn structural_outstanding_ignores_media() {
    let r = root("structural");
    note_unavailable(&r.join("cover.jpg"));
    note_unavailable(&r.join("index.md"));

    assert_eq!(noted_under(&r).len(), 2, "both were recorded");
    assert_eq!(
        noted_under(&r)
            .iter()
            .filter(|p| is_structural_source(p))
            .count(),
        1,
        "only the page source is structural"
    );
    assert_eq!(
        structural_outstanding(&r),
        0,
        "neither path exists, so neither is awaited"
    );
}
