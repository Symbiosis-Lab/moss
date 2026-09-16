use super::*;
use proptest::prelude::*;
use std::collections::HashSet;

fn tmp_dir() -> tempfile::TempDir {
    let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    tempfile::TempDir::new_in(&test_tmp).unwrap()
}

fn write(dir: &Path, rel: &str, content: &str) {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

#[test]
fn orphaned_webp_with_no_reference_anywhere_is_removed() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html><body>no images here</body></html>");
    write(tmp.path(), "assets/orphan.webp", "fake webp bytes");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    outputs.insert("assets/orphan.webp".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 1);
    assert!(removed.contains("assets/orphan.webp"));
    assert!(!tmp.path().join("assets/orphan.webp").exists());
}

/// A genuine orphan whose basename happens to collide with a variant *rung*
/// name (`w800.webp`, as if the directory/base component were stripped) must
/// still be pruned — widening the token class or SCAN_EXTENSIONS must not
/// turn "this looks like it could be a rung" into a free pass.
#[test]
fn file_literally_named_after_a_rung_with_no_reference_is_still_pruned() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html><body>no images here</body></html>");
    write(tmp.path(), "w800.webp", "fake webp bytes");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    outputs.insert("w800.webp".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 1);
    assert!(removed.contains("w800.webp"));
    assert!(!tmp.path().join("w800.webp").exists());
}

/// Invariant 6 (moss#976): an image reference must survive pruning no
/// matter which scannable text format carries it — `<source srcset>`,
/// CSS `url()`, an RSS/Atom enclosure, or a plugin's bare `data-*`
/// attribute all put the same shape of token in the output. Table-driven
/// per docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md
/// P3 — collapses what were three near-identical tests into one.
#[test]
fn image_referenced_only_from_non_html_wrapper_survives() {
    struct Case {
        name: &'static str,
        file: &'static str,
        wrapper: fn(&str) -> String,
    }
    let cases = [
        Case {
            name: "css url()",
            file: "assets/theme.css",
            wrapper: |rel| format!(r#".hero {{ background-image: url("{rel}"); }}"#),
        },
        Case {
            name: "feed.xml enclosure",
            file: "feed.xml",
            wrapper: |rel| {
                format!(r#"<rss><channel><item><enclosure url="{rel}" type="image/webp"/></item></channel></rss>"#)
            },
        },
        Case {
            name: "plugin data attribute",
            file: "gallery/index.html",
            wrapper: |rel| format!(r#"<div data-lightbox-full="{rel}" data-caption="hi"></div>"#),
        },
        Case {
            name: "svg image href",
            file: "icon.svg",
            wrapper: |rel| {
                format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="{rel}"/></svg>"#)
            },
        },
        Case {
            name: "notebook cell output",
            file: "notebook.ipynb",
            wrapper: |rel| {
                format!(r#"{{"cells":[{{"cell_type":"markdown","source":["![cover]({rel})"]}}]}}"#)
            },
        },
    ];

    for case in cases {
        let tmp = tmp_dir();
        let asset_key = "assets/wrapped.webp";
        write(tmp.path(), case.file, &(case.wrapper)("../assets/wrapped.webp"));
        write(tmp.path(), asset_key, "fake webp bytes");

        let referenced = extract_referenced_tails(tmp.path()).tails;
        let mut outputs = HashSet::new();
        outputs.insert(asset_key.to_string());

        let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

        assert_eq!(result.files_removed, 0, "case {:?} must survive pruning", case.name);
        assert!(removed.is_empty(), "case {:?} must survive pruning", case.name);
    }
}

#[test]
fn raster_fallback_tier_is_never_pruned_even_when_unreferenced() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html></html>");
    write(tmp.path(), "assets/orphan.png", "fake png bytes");
    write(tmp.path(), "assets/orphan.jpg", "fake jpg bytes");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    outputs.insert("assets/orphan.png".to_string());
    outputs.insert("assets/orphan.jpg".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0);
    assert!(removed.is_empty());
    assert!(tmp.path().join("assets/orphan.png").exists());
    assert!(tmp.path().join("assets/orphan.jpg").exists());
}

#[test]
fn reference_at_a_different_relative_depth_still_matches() {
    let tmp = tmp_dir();
    // Deep page references the asset via a longer ../ chain than the
    // manifest's site-root-relative key.
    write(
        tmp.path(),
        "a/b/c/index.html",
        r#"<img src="../../../assets/deep.webp">"#,
    );
    write(tmp.path(), "assets/deep.webp", "fake webp bytes");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    outputs.insert("assets/deep.webp".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0);
    assert!(removed.is_empty());
}

#[test]
fn absolute_and_data_uri_references_are_ignored_not_matched() {
    let tmp = tmp_dir();
    write(
        tmp.path(),
        "index.html",
        r#"<img src="https://example.com/assets/unrelated.webp"><img src="data:image/webp;base64,AAA=">"#,
    );
    write(tmp.path(), "assets/unrelated.webp", "fake webp bytes");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    // The absolute URL must not falsely "protect" the local file of the same
    // name via a suffix collision — same basename, but the local file has no
    // real reference and should be pruned.
    let mut outputs = HashSet::new();
    outputs.insert("assets/unrelated.webp".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 1);
    assert!(removed.contains("assets/unrelated.webp"));
}

#[test]
fn path_suffixes_yields_every_trailing_slice() {
    let got: Vec<String> = path_suffixes("a/b/c.webp").collect();
    assert_eq!(got, vec!["a/b/c.webp", "b/c.webp", "c.webp"]);
}

#[test]
fn decoding_strips_query_and_fragment_but_keeps_the_relative_prefix() {
    // The prefix is the only thing that says where the reference points from,
    // so it survives decoding and is stripped later, per reading.
    assert_eq!(
        decode_reference("../../assets/x.webp?v=2#frag"),
        Some("../../assets/x.webp".to_string())
    );
    assert_eq!(decode_reference("https://cdn.example.com/x.webp"), None);
    assert_eq!(decode_reference("data:image/webp;base64,AAA"), None);
    assert_eq!(strip_relative_prefixes("../../assets/x.webp"), "assets/x.webp");
    assert_eq!(strip_relative_prefixes("/assets/x.webp"), "assets/x.webp");
}

/// A reference written SHALLOWER than the key, which path suffixes
/// structurally cannot reach: suffixes of a token only ever get shorter, so
/// `assets/x.webp` can never equal `gallery/assets/x.webp`.
///
/// This is the ordinary shape for every non-HTML type in `SCAN_EXTENSIONS` —
/// a stylesheet's `url()` is relative to the stylesheet. Before the token was
/// also resolved against the file it was found in, this stylesheet's live
/// reference read as an orphan and the variant was deleted; with moss#1085's
/// verdict persisted, `suppressed_variants` would then have kept it deleted,
/// leaving a CSS rule pointing at nothing for good.
#[test]
fn a_stylesheet_below_the_root_protects_the_asset_beside_it() {
    let tmp = tmp_dir();
    write(
        tmp.path(),
        "gallery/style.css",
        r#".hero { background-image: url("assets/hero.webp"); }"#,
    );
    write(tmp.path(), "gallery/assets/hero.webp", "fake webp bytes");
    let outputs: HashSet<String> = ["gallery/assets/hero.webp".to_string()].into();

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0, "removed {removed:?}");
    assert!(
        suppressed_variants(tmp.path(), &outputs).is_empty(),
        "and a stale verdict for it must lift, or the producers stop re-making it"
    );
}

/// The other direction, and the reason both readings are kept rather than
/// resolution replacing suffixes: a data file below the root listing
/// site-root-relative keys WITHOUT a leading `/`. Resolving that against the
/// file's own directory yields `data/assets/hero.webp`, which is not the key —
/// only the suffix reading matches it. Delete either half of the union and one
/// of these two tests goes red.
#[test]
fn a_data_file_below_the_root_listing_root_relative_keys_still_protects_them() {
    let tmp = tmp_dir();
    write(
        tmp.path(),
        "data/gallery.json",
        r#"{"images": ["assets/hero.webp"]}"#,
    );
    write(tmp.path(), "assets/hero.webp", "fake webp bytes");
    let outputs: HashSet<String> = ["assets/hero.webp".to_string()].into();

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0, "removed {removed:?}");
}

/// Explicit rows for the non-ASCII scripts and shapes named in
/// docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md
/// P3/Part 3 — CJK, Cyrillic, Arabic, Devanagari, emoji, and a comma in the
/// filename — each using the REAL variant shape (`.wNNN.webp`, a rung under
/// an `assets/` directory), since no fixture before this used a rung, which
/// is part of why Bug A shipped.
#[test]
fn non_ascii_and_comma_filenames_with_a_real_variant_rung_survive_pruning() {
    let rows: &[(&str, &str)] = &[
        ("cjk", "寫作獎海報"),
        ("cyrillic", "Фотография"),
        ("arabic", "صورة-الغلاف"),
        ("devanagari", "आवरण-चित्र"),
        ("emoji", "cover-\u{1F4F8}"),
        ("comma", "Yu,ChinMei"),
    ];

    for (label, stem) in rows {
        let tmp = tmp_dir();
        let key = format!("assets/{stem}.w800.webp");
        write(
            tmp.path(),
            "index.html",
            &format!(r#"<img src="{key}">"#),
        );

        let referenced = extract_referenced_tails(tmp.path()).tails;
        let mut outputs = HashSet::new();
        outputs.insert(key.clone());

        let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

        assert_eq!(result.files_removed, 0, "row {:?} ({:?}) must survive pruning", label, key);
        assert!(removed.is_empty(), "row {:?} ({:?}) must survive pruning", label, key);
    }
}

/// The shape moss actually emits, which the raw-filename rows above do NOT
/// cover: a percent-ENCODED URL whose very first segment is encoded. A cover
/// under a CJK directory ships as
/// `/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.w800.webp` — every byte before the
/// extension is inside an escape, so unless `%` may START a token the earliest
/// legal match begins at the `E` of `%E7`, the tail decodes to garbage, no
/// suffix equals the manifest key, and a live reference reads as an orphan.
///
/// This is not hypothetical: it is Bug A's exact failure mode surviving the
/// first fix, found by `tests/staged_html_manifest_parity.rs` before commit.
/// Both spellings must survive, because moss emits the raw form from one path
/// and the encoded form from another.
#[test]
fn a_fully_percent_encoded_reference_survives_pruning() {
    let rows: &[(&str, &str, &str)] = &[
        (
            "encoded cjk dir and leaf",
            "獎項/封面.w800.webp",
            "/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.w800.webp",
        ),
        (
            "encoded leaf only",
            "assets/封面.w1600.webp",
            "assets/%E5%B0%81%E9%9D%A2.w1600.webp",
        ),
        (
            "encoded space",
            "news/Winter Song.w400.webp",
            "news/Winter%20Song.w400.webp",
        ),
        (
            "encoded comma (srcset form)",
            "about/Yu,ChinMei.w800.webp",
            "about/Yu%2CChinMei.w800.webp",
        ),
    ];

    for (label, key, emitted) in rows {
        let tmp = tmp_dir();
        write(
            tmp.path(),
            "index.html",
            &format!(r#"<source srcset="{emitted}">"#),
        );

        let referenced = extract_referenced_tails(tmp.path()).tails;
        let mut outputs = HashSet::new();
        outputs.insert(key.to_string());

        let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

        assert_eq!(
            result.files_removed, 0,
            "row {:?}: {:?} emitted as {:?} must survive pruning",
            label, key, emitted
        );
        assert!(
            removed.is_empty(),
            "row {:?}: {:?} emitted as {:?} must survive pruning",
            label, key, emitted
        );
    }
}

/// Two properties the negated class must keep that no other test now pins,
/// both restored after the widening deleted their originals:
///
/// - an apostrophe in a filename survives **through its HTML entity**. `'` is
///   excluded from the class on purpose (see the long comment on the regex),
///   so the token has to span `&#39;` whole and `html_entities::decode` puts it
///   back. This is the only route by which an apostrophe filename is safe, so
///   it needs a test of its own.
/// - the extension alternation is case-insensitive. `(?i:)` on the alternation
///   is a different mechanism from the `to_ascii_lowercase()` applied to
///   `SCAN_EXTENSIONS`, and nothing else exercises it.
#[test]
fn apostrophe_entity_and_uppercase_extension_are_still_recognized() {
    let tmp = tmp_dir();
    write(
        tmp.path(),
        "index.html",
        r#"<img src="assets/Grandma&#39;s-House.webp"><img src="assets/Poster.WEBP">"#,
    );

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    outputs.insert("assets/Grandma's-House.webp".to_string());
    outputs.insert("assets/Poster.WEBP".to_string());

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0, "removed: {:?}", removed);
    assert!(removed.is_empty(), "removed: {:?}", removed);
}

/// The scan runs two token spellings and unions them, because each one alone
/// deletes a live file the other keeps. Both halves, in the non-HTML files
/// where a literal `'` survives (here `.js`, where nothing escapes it):
///
/// - a quoted LIST needs `'` to end a token, or `'a.webp','b.webp'` matches as
///   one glued token and both files read as unreferenced;
/// - a filename CONTAINING `'` needs `'` to continue a token, or
///   `grandmas'-house.webp` truncates to `-house.webp` and reads as
///   unreferenced.
///
/// Neither spelling satisfies both. The union does, and stays on the safe side
/// of invariant 6 because a union of over-approximations is one.
#[test]
fn both_a_quoted_list_and_an_apostrophe_filename_survive_in_the_same_file() {
    let tmp = tmp_dir();
    // A single-quoted list — needs `'` to END a token.
    write(
        tmp.path(),
        "bundle.js",
        "const covers = ['assets/a.webp','assets/b.webp'];\n",
    );
    // A double-quoted JSON string carrying a raw apostrophe in the filename —
    // `serde_json` does not escape `'`, so this is the shape moss's own
    // `previews.json` would take. Needs `'` to CONTINUE a token.
    write(
        tmp.path(),
        "previews.json",
        "{\"cover\":\"assets/grandmas'-house.webp\"}\n",
    );

    let referenced = extract_referenced_tails(tmp.path()).tails;
    let mut outputs = HashSet::new();
    for key in [
        "assets/a.webp",
        "assets/b.webp",
        "assets/grandmas'-house.webp",
    ] {
        outputs.insert(key.to_string());
    }

    let (removed, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);

    assert_eq!(result.files_removed, 0, "removed: {:?}", removed);
    assert!(removed.is_empty(), "removed: {:?}", removed);
}

/// The negated-class widening must not make an ASCII-punctuation byte able
/// to START a token on its own — only ASCII alnum/underscore or non-ASCII
/// (CJK/Cyrillic/Arabic/Devanagari/emoji) may. `=` stays a legal
/// *continuation* byte (a real unescaped filename can contain one), but an
/// isolated `=` not glued to a preceding identifier run must never itself
/// become the start of a match: that would prepend a spurious leading byte
/// to the extracted tail and break the suffix comparison against the real
/// manifest key ("assets/glued.webp" would never appear in
/// `path_suffixes("=assets/glued.webp")`, only "glued.webp" would).
#[test]
fn an_isolated_equals_sign_does_not_become_the_start_of_a_token() {
    let tmp = tmp_dir();
    write(tmp.path(), "notes.txt", "path =assets/glued.webp end");

    let referenced = extract_referenced_tails(tmp.path()).tails;
    assert!(referenced.contains("assets/glued.webp"));
    assert!(!referenced.contains("=assets/glued.webp"));
}

/// Strategy for a single character that may legally START a token: ASCII
/// alnum/underscore, or any non-ASCII scalar value drawn from the scripts
/// named in the module doc (CJK, Cyrillic, Arabic, Devanagari, emoji).
fn start_char() -> impl Strategy<Value = char> {
    prop_oneof![
        3 => proptest::char::range('a', 'z'),
        3 => proptest::char::range('A', 'Z'),
        3 => proptest::char::range('0', '9'),
        1 => Just('_'),
        2 => proptest::char::range('\u{4e00}', '\u{9fff}'),   // CJK unified ideographs
        2 => proptest::char::range('\u{0400}', '\u{04FF}'),   // Cyrillic
        2 => proptest::char::range('\u{0600}', '\u{06FF}'),   // Arabic
        2 => proptest::char::range('\u{0900}', '\u{097F}'),   // Devanagari
        1 => proptest::char::range('\u{1F600}', '\u{1F64F}'), // emoji
    ]
}

/// The ASCII punctuation bytes moss's canonical URL encoder leaves LITERAL,
/// **derived by probing the encoder** rather than transcribed from it.
///
/// This is the coupling the whole module rests on: the token class in
/// `extract_referenced_tails` is safe precisely because every byte it treats
/// as a delimiter (whitespace, `"`, `<`, `>`, `(`, `)`, `:`) is a byte
/// `percent_encode_url` escapes — so that byte can never appear raw inside a
/// moss-emitted URL, and excluding it cannot truncate a live reference. The
/// one exception is `'`, which the encoder DOES leave literal and which the
/// second `TOKEN_PATTERNS` spelling exists to cover.
///
/// Nothing enforced that coupling before: the allowlist lived in
/// `moss_core::resolve::fuzzy_path::push_encoded_segment`, the delimiter set
/// lived in `orphan_prune.rs`, and this strategy held a third hand-written
/// copy. Adding one byte to the encoder's allowlist — `(` would do it — makes
/// the pruner start truncating real references again while all three lists
/// still look internally consistent. That is Bug A's exact shape, and it is
/// the only route back to it that survived the 2026-08-06 fixes.
///
/// Probing closes the loop: widen the encoder and this set widens with it, so
/// `every_byte_the_encoder_leaves_literal_survives_pruning` fails on the same
/// commit instead of on a user's site.
///
/// `/` is the path separator, and `?`/`#` are reserved by `percent_encode_url`
/// itself (`split_url_path` treats what follows as query/fragment), so a
/// filename containing them cannot round-trip through *any* URL — an
/// encoder-level limit, not a pruner one, and out of scope here.
fn encoder_literal_ascii() -> &'static [char] {
    static LITERAL: std::sync::OnceLock<Vec<char>> = std::sync::OnceLock::new();
    LITERAL.get_or_init(|| {
        (0x21u8..=0x7E)
            .map(char::from)
            .filter(|c| !matches!(c, '/' | '?' | '#' | '%'))
            .filter(|c| {
                let probe = format!("assets/a{c}b.w800.webp");
                moss_core::resolve::fuzzy_path::percent_encode_url(&probe).contains(*c)
            })
            .collect()
    })
}

/// Every byte the canonical encoder leaves literal must survive the prune when
/// it appears in a real filename — emitted the way moss emits it (encoded,
/// then `html_escape`d into an `src` attribute).
///
/// Falsifier for [`encoder_literal_ascii`]'s docs: this is what fires if the
/// encoder's allowlist and the pruner's delimiter set ever drift apart.
#[test]
fn every_byte_the_encoder_leaves_literal_survives_pruning() {
    let mut deleted = Vec::new();
    for &c in encoder_literal_ascii() {
        let tmp = tmp_dir();
        let key = format!("assets/a{c}b.w800.webp");
        let url = moss_core::resolve::fuzzy_path::percent_encode_url(&key);
        write(
            tmp.path(),
            "index.html",
            &format!(r#"<img src="{}">"#, moss_core::media::html_escape(&url)),
        );
        write(tmp.path(), &key, "fake webp bytes");

        let referenced = extract_referenced_tails(tmp.path()).tails;
        let outputs: HashSet<String> = std::iter::once(key.clone()).collect();
        let (_, result) = prune_orphaned_webp(tmp.path(), &outputs, &referenced);
        if result.files_removed != 0 {
            deleted.push(format!("{c:?} (emitted as {url})"));
        }
    }

    assert!(
        !deleted.is_empty() || !encoder_literal_ascii().is_empty(),
        "probe found no literal bytes at all — the encoder or this test broke, \
         and the loop below is vacuous"
    );
    assert!(
        deleted.is_empty(),
        "the pruner DELETED a live reference for {} byte(s) the URL encoder \
         leaves literal: {}\n\nEach one is a 404 on the live site. Either add \
         the byte to the token class in `extract_referenced_tails`, or stop \
         leaving it literal in `push_encoded_segment`.",
        deleted.len(),
        deleted.join(", "),
    );
}

/// Strategy for a character that may CONTINUE a token: everything
/// `start_char` allows, plus combining marks and the ASCII punctuation bytes
/// moss's URL encoder leaves unescaped — drawn from [`encoder_literal_ascii`]
/// so this strategy cannot drift from the encoder the way a hand-written list
/// did. (`html_escape` further encodes `&` and `'` before this reaches a
/// scanned file, exercising the entity-span path too.)
fn continue_char() -> impl Strategy<Value = char> {
    prop_oneof![
        6 => start_char(),
        1 => proptest::char::range('\u{0300}', '\u{036F}'), // combining marks
        6 => proptest::sample::select(encoder_literal_ascii()),
    ]
}

fn filename_strategy() -> impl Strategy<Value = String> {
    (start_char(), proptest::collection::vec(continue_char(), 0..24)).prop_map(
        |(first, rest)| {
            let mut s = String::new();
            s.push(first);
            s.extend(rest);
            s
        },
    )
}

proptest::proptest! {
    /// Round-trip falsifier for the whole widening: any arbitrary UTF-8
    /// filename, emitted the way real (possibly un-encoded, per Bug A) HTML
    /// embeds it — `html_escape`d but NOT percent-encoded — must be
    /// recognized as referenced. This is the general case the explicit CJK/
    /// Cyrillic/Arabic/Devanagari/emoji/comma rows above sample from by hand.
    #[test]
    fn any_utf8_filename_emitted_raw_into_html_is_recognized_as_referenced(
        stem in filename_strategy(),
    ) {
        let tmp = tmp_dir();
        let key = format!("assets/{stem}.w800.webp");
        let escaped = moss_core::media::html_escape(&key);
        write(
            tmp.path(),
            "index.html",
            &format!(r#"<img src="{escaped}">"#),
        );

        let referenced = extract_referenced_tails(tmp.path()).tails;
        proptest::prop_assert!(
            referenced.contains(&key),
            "stem {:?} -> key {:?} not found in {:?}",
            stem,
            key,
            referenced,
        );
    }
}

// ---------------------------------------------------------------------------
// The scan's own blindness — an incomplete reference set authorizes deletion
// ---------------------------------------------------------------------------

/// A page the scan cannot READ must be reported, because every reference it
/// holds is missing from `tails` and a missing reference is a deletion.
#[cfg(unix)]
#[test]
fn a_page_that_cannot_be_read_is_reported_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", r#"<img src="assets/kept.webp">"#);
    write(tmp.path(), "secret/page.html", r#"<img src="assets/hidden.webp">"#);
    let locked = tmp.path().join("secret/page.html");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(
        std::fs::read(&locked).is_err(),
        "mode 0o000 must genuinely block the read — as root it would not, and \
         this test would prove nothing"
    );

    let scan = extract_referenced_tails(tmp.path());

    assert!(
        scan.unreadable.iter().any(|p| p == &locked),
        "unreadable page missing from the report: {:?}",
        scan.unreadable
    );
    // And this is why it matters: the reference inside it is simply absent.
    assert!(scan.tails.contains("assets/kept.webp"));
    assert!(!scan.tails.contains("assets/hidden.webp"));
}

/// A directory the walk cannot open hides a whole subtree, so it counts the
/// same way — the `read_dir` arm has its own fail-open history.
#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_opened_is_reported_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html></html>");
    write(tmp.path(), "locked/page.html", r#"<img src="assets/hidden.webp">"#);
    let locked = tmp.path().join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(std::fs::read_dir(&locked).is_err(), "mode 0o000 must block read_dir");

    let scan = extract_referenced_tails(tmp.path());
    let reported = scan.unreadable.iter().any(|p| p == &locked);

    // Restore before the TempDir drops, or the tree cannot be cleaned up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(reported, "unopenable dir missing from the report: {:?}", scan.unreadable);
}

/// The distinction the read site must keep: bytes that are not UTF-8 are NOT
/// an I/O fault. Nothing moss emits into a scannable extension is binary, so
/// there is no reference to miss — reporting it would make an ordinary stray
/// file permanently disable pruning for the site.
#[test]
fn non_utf8_bytes_in_a_scannable_extension_are_skipped_not_reported() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", r#"<img src="assets/kept.webp">"#);
    std::fs::write(tmp.path().join("stray.json"), [0xff, 0xfe, 0x00, 0x01]).unwrap();

    let scan = extract_referenced_tails(tmp.path());

    assert!(scan.unreadable.is_empty(), "reported: {:?}", scan.unreadable);
    assert!(scan.tails.contains("assets/kept.webp"));
}

// ---------------------------------------------------------------------------
// suppressed_variants — the carried verdict the producers read (moss#1085)
// ---------------------------------------------------------------------------

/// The steady state the whole mechanism exists to reach: a variant a complete
/// scan already judged unreferenced stays suppressed, so the producers stop
/// re-making what the prune keeps deleting.
#[test]
fn a_variant_the_last_scan_pruned_stays_suppressed() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html><body>no images here</body></html>");
    let previously_pruned: HashSet<String> = ["assets/orphan.webp".to_string()].into();

    let suppressed = suppressed_variants(tmp.path(), &previously_pruned);

    assert!(suppressed.contains("assets/orphan.webp"));
}

/// The escape hatch, and the reason suppression is not a one-way door: an
/// author points a page at an image nobody used before, and the suppression
/// lifts on that same build — the page HTML is already staged when the
/// producers run.
#[test]
fn a_variant_this_build_references_again_is_not_suppressed() {
    let tmp = tmp_dir();
    write(
        tmp.path(),
        "index.html",
        r#"<picture><source srcset="assets/orphan.webp" type="image/webp"></picture>"#,
    );
    let previously_pruned: HashSet<String> = ["assets/orphan.webp".to_string()].into();

    let suppressed = suppressed_variants(tmp.path(), &previously_pruned);

    assert!(
        suppressed.is_empty(),
        "a re-referenced variant must be produced again, got {suppressed:?}"
    );
}

/// A cold vault has no verdict to carry, so nothing is suppressed and the
/// first build behaves exactly as it did before moss#1085 — encode everything,
/// let the ship-time prune decide with complete information. This is also the
/// early return that keeps the staging scan off the first build's critical
/// path.
#[test]
fn nothing_is_suppressed_before_a_prune_has_ever_run() {
    let tmp = tmp_dir();
    write(tmp.path(), "index.html", "<html><body>no images here</body></html>");

    assert!(suppressed_variants(tmp.path(), &HashSet::new()).is_empty());
}
