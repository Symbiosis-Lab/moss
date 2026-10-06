//! The wiki spelling (`![[x|attrs]]`, `[[x]]`) and the standard spelling
//! (`![attrs](x)`, `[text](x)`) are interchangeable: wherever the standard
//! form can express a reference, both must render the same bytes. One table
//! drives every kind of target through the real page-body pipeline and
//! reports every cell that differs, so a gap shows up as a row, not as a
//! surprise in one author's page.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use moss_build::build::markdown::{process_markdown_file, PageContext, SiteMarkdown};
use moss_build::build::scan::scan::{build_content_graph, scan_folder};
use moss_core::content_graph::ContentGraph;

const PAGE: &str = "index.md";

const NOTEBOOK: &str = r#"{"cells":[{"cell_type":"markdown","metadata":{},"source":["Notebook prose."]},{"cell_type":"code","metadata":{},"execution_count":1,"outputs":[],"source":["print(1)"]}],"metadata":{"kernelspec":{"name":"python3","display_name":"Python 3","language":"python"}},"nbformat":4,"nbformat_minor":5}"#;

/// (path, contents) of every file the cases below point at.
const FILES: &[(&str, &str)] = &[
    ("index.md", "x"),
    ("photos/photo.jpg", "x"),
    ("media/clip.mp4", "x"),
    ("media/song.mp3", "x"),
    ("docs/paper.pdf", "x"),
    ("widgets/page.html", "x"),
    ("models/thing.glb", "x"),
    ("notes/essay.md", "# Essay\n\nOpening words.\n\n## Methods\n\nStep text.\n"),
    ("data/ledger.csv", "name,count\napples,3\npears,5\n"),
    ("lab/analysis.ipynb", NOTEBOOK),
    ("journal/one.md", "---\ntitle: First entry\n---\n\nOne body.\n"),
    ("journal/two.md", "---\ntitle: Second entry\n---\n\nTwo body.\n"),
    // Pages that embed themselves or each other, once per spelling.
    ("loop/self-std.md", "Self.\n\n![](/loop/self-std.md)\n"),
    ("loop/self-wiki.md", "Self.\n\n![[/loop/self-wiki.md]]\n"),
    ("loop/a-std.md", "A.\n\n![](/loop/b-std.md)\n"),
    ("loop/b-std.md", "B.\n\n![](/loop/a-std.md)\n"),
    ("loop/a-wiki.md", "A.\n\n![[/loop/b-wiki.md]]\n"),
    ("loop/b-wiki.md", "B.\n\n![[/loop/a-wiki.md]]\n"),
];

struct Site {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    graph: ContentGraph,
}

fn site() -> Site {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    fs::create_dir_all(&base).unwrap();
    let dir = tempfile::Builder::new().prefix("interchangeable_").tempdir_in(&base).unwrap();
    let root = dir.path().to_path_buf();
    for (rel, body) in FILES {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    let ps = scan_folder(&root.to_string_lossy()).expect("scan");
    let graph = build_content_graph(&ps);
    Site { _dir: dir, root, graph }
}

fn render(s: &Site, md: &str) -> String {
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
        SiteMarkdown { implicit_figure: true, ..Default::default() },
        None,
        None,
        Some(&s.graph),
        PageContext::default(),
    )
    .expect("parse")
    .html_content
}

const YT: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const REMOTE_IMG: &str = "https://example.com/pics/a.jpg";

/// One cell is left out on purpose: an image whose bracket text is a display
/// keyword (`![[a.jpg|cover]]` crops the picture, `![cover](a.jpg)` captions
/// it "cover"). The word is both a valid caption and a valid keyword, so
/// neither reading is chosen for the author.
///
/// (label, wiki spelling, standard spelling). Standard destinations are the
/// site-relative path; the wiki side names the file the way an author would.
fn embed_cases() -> Vec<(String, String, String)> {
    let mut v: Vec<(String, String, String)> = Vec::new();
    // kind, wiki name, std path, attribute texts that apply to the kind
    let kinds: [(&str, &str, &str, &[&str]); 12] = [
        ("image", "photo.jpg", "photos/photo.jpg", &["", "A caption", "400", "align-right 50%", "A caption|400", "align-right 50%|A caption"]),
        ("video", "clip.mp4", "media/clip.mp4", &["", "640x360 loop", "400", "a label", "align-right 50%|A caption", "a label loop"]),
        ("audio", "song.mp3", "media/song.mp3", &["", "a label", "loop", "align-right 50%|A caption"]),
        ("pdf", "paper.pdf", "docs/paper.pdf", &["", "wide", "a label", "align-right 50%|A caption"]),
        ("html", "page.html", "widgets/page.html", &["", "600x400", "My Widget", "align-right 50%|A caption"]),
        ("model", "thing.glb", "models/thing.glb", &["", "a label", "400"]),
        ("page", "essay", "notes/essay.md", &["", "ignored words", "align-right 50%", "style:map|align-right 50%"]),
        ("page-ext", "essay.md", "notes/essay.md", &[""]),
        ("page-heading", "essay#Methods", "notes/essay.md#methods", &[""]),
        ("table", "ledger.csv", "data/ledger.csv", &["", "some words"]),
        ("notebook", "analysis.ipynb", "lab/analysis.ipynb", &["", "some words"]),
        ("folder", "journal/", "journal/", &["", "limit:1", "style:grid|wide|A caption", "sort:date,limit:2"]),
    ];
    for (kind, wiki, path, attrs) in kinds {
        for a in attrs {
            let w = if a.is_empty() { format!("![[{wiki}]]\n") } else { format!("![[{wiki}|{a}]]\n") };
            let s = format!("![{a}]({path})\n");
            v.push((format!("{kind} [{a}]"), w, s));
        }
    }
    for (kind, url, attrs) in [
        ("remote player", YT, &["", "640x360", "A title", "align-right 50%"][..]),
        ("remote image", REMOTE_IMG, &["", "A caption", "400"][..]),
    ] {
        for a in attrs {
            let w = if a.is_empty() { format!("![[{url}]]\n") } else { format!("![[{url}|{a}]]\n") };
            v.push((format!("{kind} [{a}]"), w, format!("![{a}]({url})\n")));
        }
    }
    v
}

fn link_cases() -> Vec<(String, String, String)> {
    let c = |l: &str, w: &str, s: &str| (l.to_string(), format!("{w}\n"), format!("{s}\n"));
    vec![
        c("link page", "[[essay|the essay]]", "[the essay](notes/essay.md)"),
        c("link page bare", "[[essay]]", "[essay](notes/essay.md)"),
        c("link heading", "[[essay#Methods|the methods]]", "[the methods](notes/essay.md#methods)"),
        c("link heading bare", "[[essay#Methods]]", "[essay#Methods](notes/essay.md#methods)"),
        c("link asset", "[[paper.pdf|the paper]]", "[the paper](docs/paper.pdf)"),
        c("link folder", "[[journal/|the journal]]", "[the journal](journal/)"),
    ]
}

#[test]
fn the_two_spellings_render_the_same_bytes_for_every_kind() {
    let s = site();
    let mut differ = Vec::new();
    let mut total = 0;
    for (label, wiki, std) in embed_cases().into_iter().chain(link_cases()) {
        total += 1;
        // A wikilink carries `class="wikilink"` so a theme can tell how it was
        // written; where it points and what it says must not differ.
        let (w, t) = (render(&s, &wiki).replace(" class=\"wikilink\"", ""), render(&s, &std));
        if w != t {
            differ.push(format!("--- {label}\n wiki {wiki:?}\n  std {std:?}\n  wiki => {w}\n  std  => {t}"));
        }
    }
    assert!(differ.is_empty(), "{} of {total} cells differ:\n{}", differ.len(), differ.join("\n"));
}

#[test]
fn every_spelling_of_a_standard_target_reaches_the_same_transclusion() {
    let s = site();
    for (wiki, std) in [
        ("![[essay]]", "![](/notes/essay.md)"),
        ("![[essay]]", "![](<notes/essay.md>)"),
        ("![[essay]]", "![](notes/essay.md \"a title\")"),
        ("![[essay]]", "![](notes/essay.md)"),
        ("![[essay]]", "![](notes/%65ssay.md)"),
        ("![[/journal/]]", "![](/journal/)"),
        ("Before ![[ledger.csv]] after", "Before ![](data/ledger.csv) after"),
        ("- ![[analysis.ipynb]]", "- ![](lab/analysis.ipynb)"),
        ("> ![[essay#Methods]]", "> ![](notes/essay.md#Methods)"),
    ] {
        let (w, t) = (render(&s, &format!("{wiki}\n")), render(&s, &format!("{std}\n")));
        assert_eq!(w, t, "{wiki} vs {std}");
    }
}

#[test]
fn a_linked_or_unresolved_standard_image_of_a_page_is_not_a_transclusion() {
    let s = site();
    let linked = render(&s, "[![](notes/essay.md)](https://example.com)\n");
    assert!(linked.contains("<img") && !linked.contains("Opening words"), "{linked}");
    // A page that is not there degrades the way the wiki spelling does.
    let missing = render(&s, "![](notes/gone.md)\n");
    assert!(!missing.contains("<img") && !missing.contains("Opening words"), "{missing}");
    assert_eq!(missing, render(&s, "![[notes/gone.md]]\n").replace(" class=\"wikilink\"", ""));
}

#[test]
fn a_page_that_embeds_itself_or_its_embedder_ends_the_same_way_in_both_spellings() {
    let s = site();
    for (std, wiki) in [("self-std", "self-wiki"), ("a-std", "a-wiki")] {
        let t = render(&s, &format!("![](/loop/{std}.md)\n"));
        let w = render(&s, &format!("![[/loop/{wiki}.md]]\n"));
        // The spelling is part of each page's file name; compare after
        // naming them alike. The embedded page's own embed is not expanded in
        // either spelling, which is also why neither loops.
        let norm = |h: &str| h.replace("-std", "-x").replace("-wiki", "-x");
        assert_eq!(norm(&w), norm(&t), "{std}");
        assert!(t.contains("Self.") || t.contains("A."), "{t}");
        assert!(!t.contains("<img"), "{t}");
    }
}

#[test]
fn a_provider_url_renders_the_same_in_a_list_a_quote_a_gallery_and_a_hero() {
    let s = site();
    let (wiki, std) = (format!("![[{YT}]]"), format!("![]({YT})"));
    for wrap in ["- {}\n", "> {}\n", "{} trailing words\n", ":::gallery\na.jpg\n{}\n:::\n", ":::hero\n{}\n:::\n"] {
        let (w, t) = (render(&s, &wrap.replace("{}", &wiki)), render(&s, &wrap.replace("{}", &std)));
        assert_eq!(w, t, "{wrap:?}");
    }
    let list = render(&s, &format!("- {std}\n"));
    assert!(list.contains("<iframe") && !list.contains("<img"), "{list}");
}
