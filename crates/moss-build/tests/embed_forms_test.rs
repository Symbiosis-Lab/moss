//! `![alt](path)` and `![[name|text]]` are two spellings of one embed. For a
//! site file of a typed kind (video, audio, PDF, HTML, 3D model) the standard
//! spelling must render exactly what the wiki spelling renders, and a
//! percent-encoded target must find the same file the link forms find. These
//! tests drive the real page-body pipeline, so a regression in either the
//! parser's figure gate, the dispatcher, or URL resolution shows up here.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use moss_build::build::markdown::{process_markdown_file, PageContext, SiteMarkdown};
use moss_build::build::scan::scan::{build_content_graph, scan_folder};
use moss_core::content_graph::ContentGraph;
use moss_core::resolve::DiagnosticKind;

const PAGE: &str = "index.md";

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

struct Site {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    graph: ContentGraph,
}

fn site(files: &[&str]) -> Site {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp");
    fs::create_dir_all(&base).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("embed_forms_")
        .tempdir_in(&base)
        .unwrap();
    let root = dir.path().to_path_buf();
    touch(&root, PAGE);
    for f in files {
        touch(&root, f);
    }
    let ps = scan_folder(&root.to_string_lossy()).expect("scan");
    let graph = build_content_graph(&ps);
    Site {
        _dir: dir,
        root,
        graph,
    }
}

fn render(s: &Site, md: &str) -> String {
    process(s, md).html_content
}

fn process(s: &Site, md: &str) -> moss_build::build::types::ParsedDocument {
    let map = HashMap::new();
    let handlers = moss_build::build::embed_handlers::builtin_marker_handlers(
        s.root.clone(),
        moss_build::i18n::Language::En,
    );
    let snap = moss_core::asset_snapshot::AssetSnapshot::new();
    let root = s.root.clone();
    let resolved = moss_core::resolve::resolve_content_with_handlers_and_snapshot(
        PAGE,
        md,
        &s.graph,
        &|p| std::fs::read_to_string(root.join(p)).ok(),
        &handlers,
        &snap,
    );
    process_markdown_file(
        PAGE,
        &resolved.content_markdown,
        "site",
        &map,
        false,
        moss_build::i18n::Language::En,
        None,
        SiteMarkdown {
            implicit_figure: true,
            ..Default::default()
        },
        None,
        None,
        Some(&s.graph),
        PageContext::default(),
    )
    .expect("parse")
}

/// Diagnostics the dispatcher alone raises for `md` (before URL resolution
/// gets a chance to report the same missing file).
fn dispatch_diagnostics(s: &Site, md: &str) -> Vec<DiagnosticKind> {
    let mut doc = moss_core::ast::parse(md);
    let snap = moss_core::asset_snapshot::AssetSnapshot::new();
    let d = moss_core::ast::dispatch_wikilink_embeds(&mut doc, &snap, &s.graph, PAGE);
    d.diagnostics.iter().map(|x| x.kind.clone()).collect()
}

/// Diagnostic kinds the build raises for `md`, in production order
/// (parse -> dispatch -> resolve_urls).
fn diagnostics(s: &Site, md: &str) -> Vec<DiagnosticKind> {
    let mut doc = moss_core::ast::parse(md);
    let snap = moss_core::asset_snapshot::AssetSnapshot::new();
    let d = moss_core::ast::dispatch_wikilink_embeds(&mut doc, &snap, &s.graph, PAGE);
    let u = moss_core::ast::resolve_urls(&mut doc, &s.graph, PAGE);
    d.diagnostics
        .iter()
        .chain(u.diagnostics.iter())
        .map(|x| x.kind.clone())
        .collect()
}

const TYPED: [(&str, &str, &str, &str); 5] = [
    // (site path, wiki name, tag the typed renderer emits, label)
    ("media/clip.mp4", "clip.mp4", "<video", "video"),
    ("media/song.mp3", "song.mp3", "<audio", "audio"),
    ("docs/paper.pdf", "paper.pdf", "<object", "pdf"),
    ("widgets/page.html", "page.html", "<iframe", "html"),
    ("models/thing.glb", "thing.glb", "<model-viewer", "model"),
];

fn typed_site() -> Site {
    site(&[
        "media/clip.mp4",
        "media/song.mp3",
        "docs/paper.pdf",
        "widgets/page.html",
        "models/thing.glb",
        "photos/photo.jpg",
    ])
}

#[test]
fn standard_embed_of_a_typed_file_renders_what_the_wiki_form_renders() {
    let s = typed_site();
    for (path, wiki, tag, label) in TYPED {
        for (std_md, wiki_md) in [
            (format!("![]({path})\n"), format!("![[{wiki}]]\n")),
            (
                format!("Before ![]({path}) after.\n"),
                format!("Before ![[{wiki}]] after.\n"),
            ),
        ] {
            let w = render(&s, &wiki_md);
            assert!(w.contains(tag), "{label}: wiki form should emit {tag}: {w}");
            assert_eq!(render(&s, &std_md), w, "{label}: {std_md:?} vs {wiki_md:?}");
        }
    }
}

#[test]
fn standard_alt_text_plays_the_role_of_the_wiki_text_after_the_pipe() {
    let s = typed_site();
    for (std_md, wiki_md) in [
        ("![400](media/clip.mp4)\n", "![[clip.mp4|400]]\n"),
        ("![some alt](media/clip.mp4)\n", "![[clip.mp4|some alt]]\n"),
        ("x ![400](media/clip.mp4) y\n", "x ![[clip.mp4|400]] y\n"),
        ("![wide](docs/paper.pdf)\n", "![[paper.pdf|wide]]\n"),
    ] {
        assert_eq!(render(&s, std_md), render(&s, wiki_md), "{std_md:?}");
    }
}

#[test]
fn an_image_whose_name_matches_only_another_format_is_missing() {
    // `chart.md` and `logo.svg` share a name with the images but are not them.
    let s = site(&["notes/chart.md", "img/logo.svg"]);
    for md in ["![](chart.png)\n", "![](logo.png)\n", "![[chart.png]]\n", "![[logo.png]]\n"] {
        assert!(
            diagnostics(&s, md).contains(&DiagnosticKind::MissingAsset),
            "{md:?} must raise a blocking MissingAsset, got {:?}",
            diagnostics(&s, md)
        );
    }
}

#[test]
fn missing_typed_file_behaves_as_the_wiki_form_and_still_blocks() {
    let s = typed_site();
    // Only the lone form is pinned: mid-sentence, a missing file ends as a
    // plain `<img>` and the URL pass reports it for both spellings, with or
    // without the dispatcher involved, so a case there would prove nothing.
    for (std_md, wiki_md) in [("![](gone.mp4)\n", "![[gone.mp4]]\n")] {
        assert_eq!(render(&s, std_md), render(&s, wiki_md), "{std_md:?}");
        assert!(
            diagnostics(&s, std_md).contains(&DiagnosticKind::MissingAsset),
            "{std_md:?} must still raise a blocking MissingAsset"
        );
        // The dispatcher itself must have claimed the embed; the later URL
        // pass would report the missing file for a plain image too.
        assert_eq!(
            dispatch_diagnostics(&s, std_md),
            dispatch_diagnostics(&s, wiki_md),
            "{std_md:?}"
        );
        assert!(!dispatch_diagnostics(&s, std_md).is_empty(), "{std_md:?}");
    }
}

#[test]
fn standard_embeds_outside_the_typed_site_file_rule_are_unchanged() {
    let s = typed_site();
    // Images of image kind keep the plain image path, in any position.
    let alone = render(&s, "![](photos/photo.jpg)\n");
    assert!(
        alone.starts_with("<p><img"),
        "empty-alt image stays <p><img>: {alone}"
    );
    assert!(!alone.contains("moss-embed"), "{alone}");
    let mid = render(&s, "Before ![](photos/photo.jpg) after.\n");
    assert!(mid.contains("<img") && !mid.contains("moss-embed"), "{mid}");
    let captioned = render(&s, "![cap|400](photos/photo.jpg)\n");
    assert!(captioned.contains("<figure"), "{captioned}");
    assert!(render(&s, "![](photos/photo.jpg?v=2)\n").contains("<img"));
    // External URLs of any extension, data URIs and unknown extensions are
    // not site files of a typed kind.
    for md in [
        "![](https://example.com/clip.mp4)\n",
        "![](http://example.com/song.mp3)\n",
        "![](//example.com/paper.pdf)\n",
        "![](data:video/mp4;base64,AAAA)\n",
        "![](media/odd.xyz)\n",
    ] {
        let h = render(&s, md);
        assert!(h.contains("<img"), "{md:?} stays an image: {h}");
        assert!(
            !h.contains("<video") && !h.contains("<audio") && !h.contains("<object"),
            "{md:?}: {h}"
        );
    }
}

/// Files the build resolves `md` to (outgoing edges), in production order.
fn targets(s: &Site, md: &str) -> Vec<String> {
    let mut doc = moss_core::ast::parse(md);
    let snap = moss_core::asset_snapshot::AssetSnapshot::new();
    let d = moss_core::ast::dispatch_wikilink_embeds(&mut doc, &snap, &s.graph, PAGE);
    let u = moss_core::ast::resolve_urls(&mut doc, &s.graph, PAGE);
    d.outgoing_links
        .iter()
        .chain(u.outgoing.iter())
        .map(|o| o.target_path.clone())
        .collect()
}

#[test]
fn percent_encoded_embed_targets_resolve_to_the_file_the_link_forms_find() {
    let s = site(&["my photo.jpg", "my clip.mp4", "照片.jpg"]);
    for (encoded, real) in [
        ("my%20photo.jpg", "my photo.jpg"),
        ("my%20clip.mp4", "my clip.mp4"),
        ("%E7%85%A7%E7%89%87.jpg", "照片.jpg"),
    ] {
        for md in [
            format!("![]({encoded})\n"),
            format!("![[{encoded}]]\n"),
            format!("[x]({encoded})\n"),
        ] {
            assert_eq!(targets(&s, &md), vec![real.to_string()], "{md:?}");
            assert!(
                diagnostics(&s, &md).is_empty(),
                "{md:?}: {:?}",
                diagnostics(&s, &md)
            );
        }
    }
    // Same page as the unambiguous spelling of the same file.
    assert_eq!(
        render(&s, "![](my%20clip.mp4)\n"),
        render(&s, "![[my clip.mp4]]\n")
    );
    assert_eq!(
        render(&s, "![](my%20photo.jpg)\n"),
        render(&s, "![](<my photo.jpg>)\n")
    );
}

#[test]
fn a_file_literally_named_with_percent_20_wins_over_the_decoded_reading() {
    let s = site(&["a%20b.jpg", "a b.jpg"]);
    for md in ["![](a%20b.jpg)\n", "![[a%20b.jpg]]\n"] {
        assert_eq!(targets(&s, md), vec!["a%20b.jpg".to_string()], "{md:?}");
    }
}

/// (site path, wiki name, attribute that carries the author's label)
const LABEL_ATTR: [(&str, &str, &str); 5] = [
    ("media/clip.mp4", "clip.mp4", "aria-label"),
    ("media/song.mp3", "song.mp3", "aria-label"),
    ("docs/paper.pdf", "paper.pdf", "aria-label"),
    ("widgets/page.html", "page.html", "title"),
    ("models/thing.glb", "thing.glb", "alt"),
];

#[test]
fn a_plain_alt_becomes_the_players_accessible_label_in_both_forms() {
    let s = typed_site();
    for (path, wiki, attr) in LABEL_ATTR {
        let want = format!(r#" {attr}="a descriptive alt""#);
        let std_html = render(&s, &format!("![a descriptive alt]({path})\n"));
        let wiki_html = render(&s, &format!("![[{wiki}|a descriptive alt]]\n"));
        assert!(std_html.contains(&want), "{path}: {std_html}");
        assert_eq!(std_html, wiki_html, "{path}");
        // Mid-sentence takes the same label.
        let mid = render(&s, &format!("a ![a descriptive alt]({path}) b\n"));
        assert!(mid.contains(&want), "{path}: {mid}");
        // No visible caption is added.
        assert!(
            !std_html.contains("<figure") && !std_html.contains("figcaption"),
            "{path}: {std_html}"
        );
    }
}

#[test]
fn a_size_or_key_value_text_is_not_a_label_and_a_label_is_escaped() {
    let s = typed_site();
    for (path, wiki, attr) in LABEL_ATTR {
        for text in ["400", "640x360", "wide", "align=right"] {
            for html in [
                render(&s, &format!("![{text}]({path})\n")),
                render(&s, &format!("![[{wiki}|{text}]]\n")),
            ] {
                assert!(
                    !html.contains(&format!(" {attr}=\"{text}\"")),
                    "{path} {text}: {html}"
                );
                assert!(!html.contains("aria-label"), "{path} {text}: {html}");
            }
        }
        let want = format!(r#" {attr}="Tom &amp; &quot;Jerry&quot;""#);
        for md in [
            format!("![[{wiki}|Tom & \"Jerry\"]]\n"),
            format!("![Tom & \"Jerry\"]({path})\n"),
        ] {
            let esc = render(&s, &md);
            assert!(esc.contains(&want), "{path} {md:?}: {esc}");
        }
    }
    // A size is kept as a size, in both forms.
    for md in ["![400](media/clip.mp4)\n", "![[clip.mp4|400]]\n"] {
        let v = render(&s, md);
        assert!(v.contains(r#" width="400px""#) && !v.contains("aria-label"), "{md:?}: {v}");
    }
    // `loop` is a flag, not label text: it leaves the size or the label beside it alone.
    let v = render(&s, "![[clip.mp4|640x360 loop]]\n");
    assert!(
        v.contains(r#" width="640px" height="360px""#) && v.contains("data-loop") && !v.contains("aria-label"),
        "{v}"
    );
    let v = render(&s, "![[clip.mp4|a label loop]]\n");
    assert!(v.contains(r#"aria-label="a label""#) && v.contains("data-loop"), "{v}");
    // A size written together with words is one piece of text: all label, no size.
    let v = render(&s, "![[clip.mp4|640x360 a label]]\n");
    assert!(v.contains(r#"aria-label="640x360 a label""#) && !v.contains(" width="), "{v}");
}

#[test]
fn display_keywords_are_not_an_accessible_label_in_either_form() {
    let s = typed_site();
    for (path, wiki, attr) in LABEL_ATTR.iter().filter(|(_, _, a)| *a != "title") {
        for text in ["cover", "contain", "top left", "cover top"] {
            for html in [
                render(&s, &format!("![{text}]({path})\n")),
                render(&s, &format!("![[{wiki}|{text}]]\n")),
            ] {
                assert!(!html.contains(&format!(" {attr}=")), "{path} {text}: {html}");
            }
        }
        // Words that merely contain a keyword are still a label.
        let html = render(&s, &format!("![[{wiki}|cover story]]\n"));
        assert!(html.contains(&format!(r#" {attr}="cover story""#)), "{path}: {html}");
    }
}

#[test]
fn an_image_title_labels_a_standard_embed_with_no_alt() {
    let s = typed_site();
    for (path, wiki, attr) in LABEL_ATTR {
        let titled = render(&s, &format!("![]({path} \"A title\")\n"));
        assert!(
            titled.contains(&format!(r#" {attr}="A title""#)),
            "{path}: {titled}"
        );
        assert_eq!(
            titled,
            render(&s, &format!("![[{wiki}|A title]]\n")),
            "{path}"
        );
        // Alt wins over the title.
        let both = render(&s, &format!("![alt text]({path} \"A title\")\n"));
        assert!(
            both.contains(r#""alt text""#) && !both.contains("A title"),
            "{path}: {both}"
        );
    }
}

#[test]
fn spellings_of_the_target_that_the_standard_form_may_take() {
    let s = typed_site();
    // Each spelling renders exactly what the wiki form renders for that target:
    // an uppercase extension, a query, a fragment, and a reference-style
    // definition (its destination decides).
    for (std_md, wiki_md) in [
        ("![](media/CLIP.MP4)\n", "![[CLIP.MP4]]\n"),
        ("![](media/clip.mp4?t=1)\n", "![[clip.mp4?t=1]]\n"),
        ("![](media/clip.mp4#t=5)\n", "![[clip.mp4#t=5]]\n"),
        ("![alt][ref]\n\n[ref]: media/clip.mp4\n", "![[clip.mp4|alt]]\n"),
    ] {
        let std_html = render(&s, std_md);
        assert!(std_html.contains("<video") && !std_html.contains("<img"), "{std_md:?}: {std_html}");
        assert_eq!(std_html, render(&s, wiki_md), "{std_md:?} vs {wiki_md:?}");
    }
    // The emitted URL drops the query and the fragment in both forms, so a
    // `?t=1` or `#t=5` on a site video never reaches the player's `src`.
    for md in ["![](media/clip.mp4?t=1)\n", "![[clip.mp4?t=1]]\n", "![](media/clip.mp4#t=5)\n"] {
        let html = render(&s, md);
        assert!(html.contains(r#" src="/media/clip.mp4""#), "{md:?}: {html}");
        assert!(!html.contains("t=1") && !html.contains("t=5"), "{md:?}: {html}");
    }
}

#[test]
fn a_video_embed_is_never_the_pages_cover_image() {
    let s = typed_site();
    for md in [
        "![](media/clip.mp4)\n",
        "![[clip.mp4]]\n",
        "x ![](media/clip.mp4) y\n",
    ] {
        assert_eq!(process(&s, md).body_cover_path, None, "{md:?}");
    }
    // A later image is still the cover: the video is skipped, not the search.
    let doc = process(&s, "![](media/clip.mp4)\n\n![](photos/photo.jpg)\n");
    assert_eq!(doc.body_cover_path.as_deref(), Some("/photos/photo.jpg"));
}

#[test]
fn a_linked_image_of_a_media_file_stays_an_image_inside_the_link() {
    // Not covered by the unified rendering: the image sits inside a link, so
    // neither the dispatcher nor the figure gate sees a lone embed.
    let s = typed_site();
    let h = render(&s, "[![](media/clip.mp4)](https://example.com)\n");
    assert!(
        h.contains(r#"href="https://example.com"><img src="/media/clip.mp4""#),
        "{h}"
    );
    assert!(!h.contains("<video"), "{h}");
}
