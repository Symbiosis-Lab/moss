use super::*;
use crate::build::manifest::PendingManifest;
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

/// Build a sealed manifest from `(source, output, source_hash, output_bytes)`
/// rows plus any unmapped outputs, registering the source hash beside the
/// mapping the way the render phase does.
fn sealed(
    pages: &[(&str, &str, &str, &[u8])],
    unmapped: &[(&str, &[u8])],
) -> crate::build::manifest::SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, hash, bytes) in pages {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
        pending.register_page_source_hash(
            src.to_string(),
            SourceMetadata { hash: hash.to_string(), size: 1, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
        );
    }
    for (out, bytes) in unmapped {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
    }
    pending.seal()
}

fn snapshot(m: &crate::build::manifest::SealedManifest) -> PublishedSnapshot {
    PublishedSnapshot::from_sealed(m, "moss:site-1", "2026-08-05T00:00:00Z".into())
}

fn verb_of(set: &ChangeSet, src: &str) -> Option<PageVerb> {
    set.pages.iter().find(|p| p.source_path == src).map(|p| p.verb)
}

#[test]
fn added_when_source_is_new() {
    let prev = snapshot(&sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[]));
    let cur = sealed(
        &[("a.md", "a/index.html", "h-a", b"A"), ("b.md", "b/index.html", "h-b", b"B")],
        &[],
    );

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.added, 1);
    assert_eq!(verb_of(&set, "b.md"), Some(PageVerb::Added));
    assert_eq!((set.edited, set.deleted, set.restyled), (0, 0, 0));
}

#[test]
fn deleted_when_source_gone_from_source_to_output() {
    let prev = snapshot(&sealed(
        &[("a.md", "a/index.html", "h-a", b"A"), ("b.md", "b/index.html", "h-b", b"B")],
        &[],
    ));
    let cur = sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[]);

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.deleted, 1);
    assert_eq!(verb_of(&set, "b.md"), Some(PageVerb::Deleted));
    assert_eq!((set.added, set.edited, set.restyled), (0, 0, 0));
}

#[test]
fn edited_when_source_hash_moved() {
    let prev = snapshot(&sealed(&[("a.md", "a/index.html", "old", b"A")], &[]));
    let cur = sealed(&[("a.md", "a/index.html", "new", b"A2")], &[]);

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.edited, 1);
    assert_eq!(verb_of(&set, "a.md"), Some(PageVerb::Edited));
    assert_eq!(set.restyled, 0, "an edit is not also a restyle");
}

#[test]
fn restyled_when_source_hash_stable_but_output_moved() {
    let prev = snapshot(&sealed(&[("a.md", "a/index.html", "h-a", b"<p>old theme</p>")], &[]));
    let cur = sealed(&[("a.md", "a/index.html", "h-a", b"<p>new theme</p>")], &[]);

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.restyled, 1);
    assert_eq!(verb_of(&set, "a.md"), Some(PageVerb::Restyled));
    assert_eq!((set.added, set.edited, set.deleted), (0, 0, 0));
}

#[test]
fn unchanged_page_appears_in_no_verb() {
    let prev = snapshot(&sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[]));
    let cur = sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[]);

    let set = classify(Some(&prev), &cur);

    assert!(set.classified);
    assert!(set.pages.is_empty(), "an unchanged page is absent, not zero-weighted");
    assert!(set.is_empty());
}

/// No local record means no honest classification is possible — every verb is
/// zero and `classified` says so. This is the state EVERY site starts in, and
/// the state after publishing from another machine.
#[test]
fn absent_previous_snapshot_yields_classified_false_and_zero_verbs() {
    let cur = sealed(
        &[("a.md", "a/index.html", "h-a", b"A"), ("b.md", "b/index.html", "h-b", b"B")],
        &[("feed.xml", b"<rss/>")],
    );

    let set = classify(None, &cur);

    assert!(!set.classified);
    assert_eq!((set.added, set.edited, set.deleted, set.restyled, set.assets), (0, 0, 0, 0, 0));
    assert!(set.pages.is_empty());

    // The degraded surface's numbers come from the server instead.
    let flat_set = flat(47, 3);
    assert!(!flat_set.classified);
    assert_eq!((flat_set.flat_upload, flat_set.flat_remove), (47, 3));
    assert_eq!(flat_set.added, 0, "degraded mode never invents a verb");
}

/// The second degraded source (moss#993 #2): a published FILE SET, recovered
/// locally by `backfill` from the last deployed generation. Same arithmetic the
/// server's `need`/`remove` would produce, and the same refusal to name a verb.
#[test]
fn a_published_file_set_yields_flat_counts_and_still_no_verbs() {
    let previous = sealed(
        &[
            ("i.md", "index.html", "h-i", b"OLD"),
            ("s.md", "stays/index.html", "h-s", b"S"),
            ("g.md", "gone/index.html", "h-g", b"G"),
        ],
        &[],
    );
    let previous = previous.files().clone();
    let current = sealed(
        &[("i.md", "index.html", "h-i", b"changed"), ("s.md", "stays/index.html", "h-s", b"S")],
        &[("feed.xml", b"<rss/>")],
    );

    let set = flat_against(&previous, &current);

    assert!(!set.classified, "a file set carries no authorship information");
    assert_eq!(set.flat_upload, 2, "the edited page and the never-published feed");
    assert_eq!(set.flat_remove, 1);
    assert_eq!((set.added, set.edited, set.deleted, set.restyled), (0, 0, 0, 0));
}

/// Outputs with no page behind them are counted, not enumerated — and only
/// when they changed. Counting every unmapped output would report "1,847
/// assets" on a publish that changes nothing.
#[test]
fn unmapped_output_files_count_as_assets_not_pages() {
    let prev = snapshot(&sealed(
        &[("a.md", "a/index.html", "h-a", b"A")],
        &[("feed.xml", b"<rss>1</rss>"), ("img/cover.webp", b"OLD"), ("sitemap.xml", b"<url/>")],
    ));
    let cur = sealed(
        &[("a.md", "a/index.html", "h-a", b"A")],
        &[
            ("feed.xml", b"<rss>1</rss>"),
            ("img/cover.webp", b"NEW"),
            ("sitemap.xml", b"<url/>"),
            ("img/new.webp", b"NEW2"),
        ],
    );

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.assets, 2, "one changed variant + one new variant, not all four outputs");
    assert!(set.pages.is_empty(), "assets are never pages");
}

/// The R1 safety valve. A page carried through a build without a source hash
/// (Stage-5b skip, parse-cache hit, iCloud defer, or a manifest predating page
/// hashing) must not be guessed at: on a watch-loop rebuild most of a large
/// site is skipped, so guessing "edited" or "deleted" would report most of the
/// site. It is classified by output alone.
#[test]
fn carried_page_missing_source_hash_is_never_edited_or_deleted() {
    let prev = snapshot(&sealed(
        &[
            ("stable.md", "stable/index.html", "h-s", b"S"),
            ("moved.md", "moved/index.html", "h-m", b"M"),
        ],
        &[],
    ));

    // Rebuild the manifest with the mappings but WITHOUT the source hashes —
    // exactly what a missing carry-forward leaves behind.
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, bytes) in [
        ("stable.md", "stable/index.html", &b"S"[..]),
        ("moved.md", "moved/index.html", &b"M-restyled"[..]),
    ] {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
    }
    let cur = pending.seal();
    assert!(cur.sources().is_empty(), "the gap this test is about");

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.edited, 0, "a hash gap must never read as an author edit");
    assert_eq!(set.deleted, 0, "a hash gap must never read as a deletion");
    assert_eq!(set.added, 0, "the page was published before; it is not new");
    assert_eq!(verb_of(&set, "stable.md"), None, "same output ⇒ no verb at all");
    assert_eq!(
        verb_of(&set, "moved.md"),
        Some(PageVerb::Restyled),
        "output moved ⇒ the honest verb is restyled"
    );
}

/// The reason the fourth verb exists. A one-key `config.toml` edit re-renders
/// every page while the author changed nothing; classified through the render
/// hash (`FacadeCache`), that reports "413 edited". Source hashes are stable
/// across it, so it must read as a restyle no matter how many pages move.
#[test]
fn config_change_is_restyled_not_edited() {
    let pages_before: Vec<(&str, &str, &str, &[u8])> = vec![
        ("a.md", "a/index.html", "h-a", b"<article class=\"narrow\">A</article>"),
        ("b.md", "b/index.html", "h-b", b"<article class=\"narrow\">B</article>"),
        ("c.md", "c/index.html", "h-c", b"<article class=\"narrow\">C</article>"),
    ];
    // Same source hashes; every output moved because the template did.
    let pages_after: Vec<(&str, &str, &str, &[u8])> = vec![
        ("a.md", "a/index.html", "h-a", b"<article class=\"wide\">A</article>"),
        ("b.md", "b/index.html", "h-b", b"<article class=\"wide\">B</article>"),
        // ...and one page the author really did edit, which must stay visible.
        ("c.md", "c/index.html", "h-c-EDITED", b"<article class=\"wide\">C rewritten</article>"),
    ];

    let prev = snapshot(&sealed(&pages_before, &[]));
    let set = classify(Some(&prev), &sealed(&pages_after, &[]));

    assert_eq!(set.edited, 1, "only the page whose SOURCE moved is an edit");
    assert_eq!(verb_of(&set, "c.md"), Some(PageVerb::Edited));
    assert_eq!(set.restyled, 2, "the rest are restyles, however many there are");
    assert_eq!((set.added, set.deleted), (0, 0));
}

/// A page that was mapped WITHOUT a source hash at the last publish, then
/// deleted. `from_sealed` drops such a page from `sources`, so a deletion loop
/// walking `sources` cannot see it: the publish removes the page from the live
/// site while the change set reports nothing being deleted. Under-reporting is
/// the direction the author cannot correct, so this is the one that matters.
#[test]
fn deletion_is_reported_even_when_the_page_was_never_hashed() {
    // Previous publish: two pages mapped, NEITHER hashed — a manifest written
    // before page hashing existed, or a rebuild whose carry-forward found
    // nothing.
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, bytes) in
        [("keep.md", "keep/index.html", &b"K"[..]), ("gone.md", "gone/index.html", &b"G"[..])]
    {
        let sp = ServedPath::from_source(out).unwrap();
        pending.register(&sp, bytes, crate::build::manifest::HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
    }
    let prev = snapshot(&pending.seal());
    assert!(prev.sources.is_empty(), "the precondition this test is about");
    assert_eq!(prev.source_to_output.len(), 2, "but both pages ARE mapped");

    // The author deletes gone.md.
    let cur = sealed(&[("keep.md", "keep/index.html", "h-k", b"K")], &[]);

    let set = classify(Some(&prev), &cur);

    assert_eq!(set.deleted, 1, "the removed page must be reported");
    assert_eq!(verb_of(&set, "gone.md"), Some(PageVerb::Deleted));
    assert!(!set.is_empty(), "a publish that removes a page is not empty");
}

/// An output that was live and is gone, with no page behind it — a dropped
/// image variant, a removed rendition, an orphaned card. `assets` walks the
/// CURRENT files and is structurally blind to it, so without a separate count
/// a publish whose only effect is removal reported itself as empty.
#[test]
fn removed_assets_are_counted_so_a_removal_only_publish_is_not_empty() {
    let prev = snapshot(&sealed(
        &[("a.md", "a/index.html", "h-a", b"A")],
        &[("img/old.webp", &b"OLD"[..]), ("feed.xml", &b"F"[..])],
    ));
    // Same page, byte-identical; the variant is gone.
    let cur = sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[("feed.xml", &b"F"[..])]);

    let set = classify(Some(&prev), &cur);

    assert_eq!((set.added, set.edited, set.deleted, set.restyled), (0, 0, 0, 0));
    assert_eq!(set.assets, 0, "nothing in the current output changed");
    assert_eq!(set.removed_assets, 1, "but one output will be removed");
    assert!(!set.is_empty(), "a publish that removes a file is not 'nothing'");
}

/// Degraded mode knows nothing, which is not the same as knowing nothing will
/// change. `is_empty()` must not turn an absence of data into a claim.
#[test]
fn an_unclassified_change_set_is_unknown_not_empty() {
    let set = classify(None, &sealed(&[("a.md", "a/index.html", "h-a", b"A")], &[]));

    assert!(!set.classified);
    assert!(!set.is_empty(), "unclassified is unknown, never 'nothing will change'");
}

// ── diff_hashes: the plain-map core classify and deploy::history share ────

#[test]
fn diff_hashes_reports_added_edited_and_deleted_over_plain_maps() {
    let previous = HashMap::from([
        ("stable.md".to_string(), "h-stable".to_string()),
        ("old.md".to_string(), "h-old".to_string()),
    ]);
    let current = HashMap::from([
        ("stable.md".to_string(), "h-stable".to_string()),
        ("new.md".to_string(), "h-new".to_string()),
    ]);

    let mut changed = diff_hashes(&previous, &current);
    changed.sort_by(|a, b| a.source_path.cmp(&b.source_path));

    assert_eq!(
        changed,
        vec![
            ChangedPage { source_path: "new.md".to_string(), verb: PageVerb::Added },
            ChangedPage { source_path: "old.md".to_string(), verb: PageVerb::Deleted },
        ]
    );
}

#[test]
fn diff_hashes_reports_edited_when_the_hash_moves() {
    let previous = HashMap::from([("a.md".to_string(), "old".to_string())]);
    let current = HashMap::from([("a.md".to_string(), "new".to_string())]);

    let changed = diff_hashes(&previous, &current);

    assert_eq!(changed, vec![ChangedPage { source_path: "a.md".to_string(), verb: PageVerb::Edited }]);
}

#[test]
fn diff_hashes_is_silent_about_an_unchanged_path() {
    let previous = HashMap::from([("a.md".to_string(), "h".to_string())]);
    let current = HashMap::from([("a.md".to_string(), "h".to_string())]);

    assert!(diff_hashes(&previous, &current).is_empty());
}
