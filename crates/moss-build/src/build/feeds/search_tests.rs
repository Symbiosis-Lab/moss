use super::*;

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

/// The pre-pass writes a mirror tree of segmented HTML and leaves the real
/// build output — which is what gets deployed — byte-for-byte alone.
#[test]
fn segmented_copy_mirrors_html_without_touching_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<html lang=\"zh\"><body><p>这是一段简单的测试文本</p></body></html>";
    std::fs::create_dir_all(dir.path().join("posts/hello")).unwrap();
    std::fs::write(dir.path().join("posts/hello/index.html"), source).unwrap();
    std::fs::write(dir.path().join("style.css"), "body{color:red}").unwrap();

    let (temp, copied_n, skipped_n) = segmented_copy(dir.path()).expect("copy should build");
    assert_eq!((copied_n, skipped_n), (1, 0), "census must count the one page it copied");

    let copied = std::fs::read_to_string(temp.path().join("posts/hello/index.html")).unwrap();
    assert!(copied.contains(' '), "copy is not segmented: {}", copied);
    assert_ne!(copied, source);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("posts/hello/index.html")).unwrap(),
        source,
        "the deployed build output must never be mutated"
    );
    assert!(
        !temp.path().join("style.css").exists(),
        "non-html should not be copied"
    );

    let path = temp.path().to_path_buf();
    drop(temp);
    assert!(!path.exists(), "scratch dir must be deleted on drop");
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
