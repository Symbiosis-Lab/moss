use super::*;
use std::path::Path;

fn segment(html: &str) -> String {
    // The shared dictionary, not a fresh `Jieba::new()` per call — that parses
    // ~5 MB of embedded dictionary, once per test in this file.
    segment_html(html, &JIEBA).expect("rewrite should succeed")
}

/// Latin text must survive byte-for-byte: the segmentation pass is a no-op
/// for every language Pagefind already handles.
#[test]
fn latin_html_is_unchanged() {
    let html = "<html lang=\"en\"><body><h1 class=\"t\">Hello</h1>\
                    <p>The quick brown fox &amp; the lazy dog.</p>\
                    <a href=\"/posts/a-b/\">link</a></body></html>";
    assert_eq!(segment(html), html);
}

/// Han runs become space-joined words; the source has no spaces at all.
#[test]
fn chinese_prose_is_split_into_words() {
    let out = segment("<p>这是一段简单的测试文本</p>");
    assert!(
        out.starts_with("<p>") && out.ends_with("</p>"),
        "markup changed: {}",
        out
    );
    let text = out.trim_start_matches("<p>").trim_end_matches("</p>");
    let words: Vec<&str> = text.split(' ').collect();
    assert!(words.len() > 3, "expected several words, got {:?}", words);
    assert_eq!(
        words.concat(),
        "这是一段简单的测试文本",
        "characters were lost"
    );
}

/// Attributes are not text nodes and must never be segmented — a slug or a
/// query string with Han in it has to survive intact.
#[test]
fn attributes_are_never_segmented() {
    let out = segment("<a href=\"/文章/标题/\" data-id=\"文章\">文章</a>");
    assert!(
        out.contains("href=\"/文章/标题/\""),
        "href was rewritten: {}",
        out
    );
    assert!(
        out.contains("data-id=\"文章\""),
        "data attr was rewritten: {}",
        out
    );
}

/// Code and script bodies are markup-adjacent: spaces inserted there would
/// corrupt what the reader copies out.
#[test]
fn code_script_style_and_pre_are_left_alone() {
    let html = "<pre><code>let 变量 = 值;</code></pre>\
                    <script>var 名字 = \"值\";</script>\
                    <style>.类名{color:red}/* 注释 */</style>\
                    <p>这是文本</p>";
    let out = segment(html);
    assert!(
        out.contains("let 变量 = 值;"),
        "code was segmented: {}",
        out
    );
    assert!(
        out.contains("var 名字 = \"值\";"),
        "script was segmented: {}",
        out
    );
    assert!(out.contains("/* 注释 */"), "style was segmented: {}", out);
    assert!(
        !out.contains("<p>这是文本</p>"),
        "prose was not segmented: {}",
        out
    );
}

/// Text after a skipped element must be segmented again — the skip is
/// scoped to the element, not sticky.
#[test]
fn skipping_is_scoped_to_the_element() {
    let out = segment("<p><code>代码</code>这是一段文本</p>");
    assert!(
        out.contains("<code>代码</code>"),
        "code was segmented: {}",
        out
    );
    assert!(
        out.contains(' '),
        "prose after </code> was not segmented: {}",
        out
    );
}

/// Han glued to Latin has to be split at the boundary too, or Pagefind
/// indexes "moss是" as a single token that no query reaches.
#[test]
fn latin_han_boundaries_get_a_space() {
    let out = segment("<p>moss是一个工具v2</p>");
    assert!(out.contains("moss "), "no space after latin: {}", out);
    assert!(out.contains(" v2"), "no space before latin: {}", out);
}

/// Kana and Hangul are out of jieba's scope and must pass through
/// untouched rather than be mis-split by a Chinese dictionary.
#[test]
fn kana_and_hangul_pass_through() {
    let html = "<p>ひらがな カタカナ 한국어</p>";
    assert_eq!(segment(html), html);
}

/// A small bilingual site: Chinese pages (two scripts, nested, one with a code
/// sample), English pages, a page with no `lang`, and a non-HTML file. Each
/// language also has a directory of several sibling pages, written in name
/// order, so a walk that followed directory order rather than name order would
/// almost surely hand them over in a different order.
const SITE_PAGES: usize = 14;

fn write_bilingual_site(root: &Path) {
    let mut pages = vec![
        ("index.html".to_string(), "<html lang=\"zh-Hant\"><head><title>首頁</title></head><body><h1>步道</h1><p>這是一段關於山區步道的文字，moss是一個工具。</p></body></html>".to_string()),
        ("posts/hello/index.html".into(), "<html lang=\"zh-Hant\"><body><h1 id=\"t\">春季筆記</h1><p>春天的步道已經重新開放。</p><pre><code>let 變量 = 值;</code></pre></body></html>".into()),
        ("posts/second/index.html".into(), "<html lang=\"zh-Hant\"><body><h1>路线说明</h1><p>这是一段简单的测试文本，包含简体字。</p><a href=\"/文章/\">文章</a></body></html>".into()),
        ("en/index.html".into(), "<html lang=\"en\"><head><title>Home</title></head><body><h1>Trail guide</h1><p>The quick brown fox &amp; the lazy dog.</p></body></html>".into()),
        ("en/posts/notes/index.html".into(), "<html lang=\"en\"><body><h1>Spring notes</h1><p>The trail reopens in spring, running and jumping.</p></body></html>".into()),
        ("about.html".into(), "<html><body><h1>About</h1><p>No language attribute on this page.</p></body></html>".into()),
    ];
    for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
        pages.push((
            format!("notes/{name}.html"),
            format!("<html lang=\"zh-Hant\"><body><h1>筆記{i}</h1><p>第{i}段步道在山谷裡。</p></body></html>"),
        ));
        pages.push((
            format!("en/notes/{name}.html"),
            format!("<html lang=\"en\"><body><h1>Note {i}</h1><p>Trail section {i} follows the valley.</p></body></html>"),
        ));
    }
    assert_eq!(pages.len(), SITE_PAGES);
    for (rel, html) in pages {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, html).unwrap();
    }
    std::fs::write(root.join("style.css"), "body{color:red}").unwrap();
}

/// A bundle as a sorted list of `(path, bytes)`, so two bundles compare
/// regardless of the order Pagefind returned the files in.
fn sorted_files(index: &SearchIndex) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = index.files.iter().map(|f| (f.rel_path.clone(), f.bytes.clone())).collect();
    files.sort();
    files
}

/// Assert two bundles hold the same files with the same bytes.
///
/// `pagefind-entry.json` is compared as JSON instead: Pagefind lists the
/// languages in it in hash-map order, which differs between two runs over the
/// same pages.
fn assert_same_bundle(got: &SearchIndex, expected: &SearchIndex, reference: &str) {
    let (got, expected) = (sorted_files(got), sorted_files(expected));
    assert_eq!(
        got.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        expected.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "bundle file set differs from the {reference}"
    );
    let parse = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).unwrap();
    for ((path, got), (_, expected)) in got.iter().zip(&expected) {
        if path == "pagefind-entry.json" {
            assert_eq!(parse(got), parse(expected), "{path} differs from the {reference}");
        } else {
            assert!(got == expected, "{path} differs from the {reference}");
        }
    }
}

/// The bundle as it was built before pages were handed to Pagefind in memory:
/// a segmented copy of the tree in a scratch directory, read back from disk by
/// Pagefind's own `add_directory`. One difference is deliberate: that build
/// walked the copy in directory order, which is not reproducible, so here each
/// page is added by its own exact-path glob in name order. What remains to
/// differ is only how a page reaches Pagefind — its URL, its parsed content,
/// its page number.
fn bundle_via_scratch_copy(site_dir: &Path) -> SearchIndex {
    let scratch = tempfile::tempdir().unwrap();
    let mut rels = Vec::new();
    for entry in walkdir::WalkDir::new(site_dir).sort_by_file_name().into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|x| x.to_str()) != Some("html")
        {
            continue;
        }
        let rel = entry.path().strip_prefix(site_dir).unwrap().to_string_lossy().replace('\\', "/");
        let html = std::fs::read_to_string(entry.path()).unwrap();
        let dest = scratch.path().join(&rel);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, segment(&html)).unwrap();
        rels.push(rel);
    }

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let files = rt.block_on(async {
        let mut index = pagefind::api::PagefindIndex::new(None).unwrap();
        for rel in &rels {
            let added = index
                .add_directory(scratch.path().to_string_lossy().into_owned(), Some(rel.clone()))
                .await
                .unwrap();
            assert_eq!(added, 1, "the glob {rel} selects exactly its own page");
        }
        index.get_files().await.unwrap()
    });
    SearchIndex {
        files: files
            .into_iter()
            .map(|f| SearchIndexFile {
                rel_path: f.filename.to_string_lossy().replace('\\', "/"),
                bytes: f.contents,
            })
            .collect(),
        pages: rels.len(),
        skipped: 0,
    }
}

/// Handing Pagefind the pages in memory must not change the bundle: same
/// files, same bytes, same census as the scratch-copy build it replaced — for
/// Chinese and English pages alike. The byte comparison covers the page URLs
/// Pagefind derives, the segmented text in every fragment, and the page
/// numbering in the index files, which follows the order pages arrive in —
/// so it also fails if the walk stops being in name order.
#[test]
fn in_memory_bundle_matches_the_scratch_copy_bundle() {
    let dir = tempfile::tempdir().unwrap();
    write_bilingual_site(dir.path());
    let before = std::fs::read(dir.path().join("index.html")).unwrap();

    let _serialize = lock_index_counter();
    let expected = bundle_via_scratch_copy(dir.path());
    let got = build_search_index(dir.path()).expect("index should build");

    assert_eq!((got.pages, got.skipped), (expected.pages, expected.skipped));
    assert_eq!(got.pages, SITE_PAGES, "every page is indexed");
    assert_same_bundle(&got, &expected, "scratch-copy build");
    for lang in ["zh-hant_", "en_"] {
        assert!(
            got.files.iter().any(|f| f.rel_path.starts_with(&format!("fragment/{lang}"))),
            "no {lang} pages indexed"
        );
    }
    assert_eq!(
        std::fs::read(dir.path().join("index.html")).unwrap(),
        before,
        "the deployed build output must never be mutated"
    );
}

/// A newer request arriving while pages are being handed to Pagefind stops the
/// pass before the next page, rather than after the whole corpus is parsed.
/// Pagefind parses one page at a time, so that phase is long enough to matter.
#[test]
fn a_cancellation_between_pages_stops_before_the_next_page() {
    let dir = tempfile::tempdir().unwrap();
    write_bilingual_site(dir.path());
    let polls = std::sync::atomic::AtomicUsize::new(0);
    // Poll 1 follows segmentation; polls 2 and 3 precede the first two pages.
    let cancel_on = 3;
    let cancelled = || polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 >= cancel_on;

    let _serialize = lock_index_counter();
    let outcome = build_search_index_cancellable(dir.path(), &cancelled).expect("no error");
    assert!(outcome.is_none(), "a cancelled pass produces nothing");
    assert_eq!(
        polls.into_inner(),
        cancel_on,
        "the pass stopped at the first poll that said cancelled"
    );
}

/// End-to-end proof that the pre-pass reaches Pagefind: a Chinese page
/// with no spaces in its source must come out of the indexer as several
/// words, not one. The fragment Pagefind writes is gzipped JSON carrying
/// the indexed text and Pagefind's own `word_count`, so it can be read
/// back without depending on the binary index format.
#[test]
fn chinese_page_is_indexed_as_multiple_words() {
    use std::io::Read;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<html lang=\"zh\"><body><h1>测试</h1>\
             <p>这是一段简单的测试文本</p></body></html>",
    )
    .unwrap();

    let _serialize = lock_index_counter();
    let index = build_search_index(dir.path()).expect("index should build");
    let fragment = index
        .files
        .iter()
        .find(|f| f.rel_path.ends_with(".pf_fragment"))
        .expect("expected a page fragment");

    let mut json = String::new();
    flate2::read::GzDecoder::new(&fragment.bytes[..])
        .read_to_string(&mut json)
        .expect("fragments are gzipped");

    assert!(
        json.contains("这是 一段"),
        "fragment text is not segmented: {}",
        json
    );

    // Without segmentation Pagefind counts the whole run as one word.
    let word_count: usize = json
        .split("\"word_count\":")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no word_count in fragment: {}", json));
    assert!(
        word_count >= 5,
        "expected several indexed words, got {}",
        word_count
    );
}

/// An output directory with no indexable HTML must not be an error — a
/// brand-new empty folder is a legitimate build.
#[test]
fn empty_directory_yields_no_files_without_erroring() {
    let dir = tempfile::tempdir().unwrap();
    let _serialize = lock_index_counter();
    let index = build_search_index(dir.path()).expect("empty dir must not error");
    assert!(index.files.is_empty());
    assert_eq!((index.pages, index.skipped), (0, 0));
}
