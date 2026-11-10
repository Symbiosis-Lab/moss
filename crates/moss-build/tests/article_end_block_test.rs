//! A `slot: before-article-end` partial closes the articles of its folder and
//! the folders beneath it, as real builds draw it.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&PathBuf::from(folder_path)),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
}

const CONFIG: &str = r#"schema_version = 6

[site]
name = "Example Notes"
lang = "en"
url = "https://example.test"
search = true
comments = true

[services.comments]

[fields.next]
type = "page"
place = "end"
"#;

const PLAIN_CONFIG: &str = "schema_version = 6\n\n[site]\nname = \"Example Notes\"\nlang = \"en\"\n";

const END: &str = "---\npartial: true\nslot: before-article-end\n---\nThe next letter arrives on Wednesday.\n";
const END_OPEN: &str = "<aside class=\"moss-article-end\"";

struct Site(PathBuf);

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Site {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("moss_article_end_{}", uuid::Uuid::new_v4()));
        let site = Site(root);
        site.write(".moss/config.toml", CONFIG);
        site.write("index.md", "---\ntitle: Home\n---\n\nWelcome to the notes.\n");
        for (rel, body) in files {
            site.write(rel, body);
        }
        site
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build(&self) {
        let result = build_sync(&self.0.to_string_lossy());
        assert!(result.is_ok(), "build failed: {result:?}");
    }

    fn output(&self, rel: &str) -> PathBuf {
        self.0.join(".moss/build.nosync/staging").join(rel)
    }

    fn html(&self, rel: &str) -> String {
        fs::read_to_string(self.output(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }
}

fn letter(title: &str) -> String {
    format!("---\ntitle: {title}\ndate: 2026-10-01\n---\n\nA letter about {title}.\n")
}

/// Every Pagefind fragment of a built site, decoded.
fn search_fragments(site: &Site) -> String {
    let mut text = String::new();
    for entry in fs::read_dir(site.output("_moss/pagefind/fragment")).unwrap() {
        let bytes = fs::read(entry.unwrap().path()).unwrap();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut text).expect("fragments are gzipped");
    }
    text
}

fn has_block(html: &str) -> bool {
    html.contains(END_OPEN)
}

#[test]
fn a_block_closes_its_folder_and_below_and_no_other_page() {
    let site = Site::new(&[
        ("letters/index.md", "---\ntitle: Letters\n---\n\nThe archive.\n"),
        ("letters/end.md", END),
        ("letters/one.md", &letter("One")),
        ("letters/2025/two.md", &letter("Two")),
        ("notes/three.md", &letter("Three")),
    ]);
    site.build();

    for page in ["letters/one/index.html", "letters/2025/two/index.html"] {
        let html = site.html(page);
        assert!(has_block(&html), "{page} must end with the block");
        assert!(html.contains("<p>The next letter arrives on Wednesday.</p>"), "{page}");
    }
    assert!(!has_block(&site.html("notes/three/index.html")), "a page in another folder");
    assert!(!has_block(&site.html("letters/index.html")), "the folder's own index");
    assert!(!has_block(&site.html("index.html")), "the home page");
    assert!(!site.output("letters/end/index.html").exists(), "the block is not a page");
}

#[test]
fn a_link_in_a_block_is_root_absolute() {
    let site = Site::new(&[
        ("letters/index.md", "---\ntitle: Letters\n---\n\nThe archive.\n"),
        ("letters/end.md", "---\npartial: true\nslot: before-article-end\n---\n[All letters](index.md)\n"),
        ("letters/one.md", &letter("One")),
        ("letters/2025/two.md", &letter("Two")),
    ]);
    site.build();

    for page in ["letters/one/index.html", "letters/2025/two/index.html"] {
        let html = site.html(page);
        assert!(html.contains("<a href=\"/letters/\">All letters</a>"), "{page}: {html}");
    }
}

#[test]
fn the_nearest_file_wins() {
    let site = Site::new(&[
        ("letters/end.md", END),
        ("letters/2025/end.md", "---\npartial: true\nslot: before-article-end\n---\nFrom the 2025 shelf.\n"),
        ("letters/one.md", &letter("One")),
        ("letters/2025/two.md", &letter("Two")),
    ]);
    site.build();

    let outer = site.html("letters/one/index.html");
    assert!(outer.contains("arrives on Wednesday") && !outer.contains("2025 shelf"), "{outer}");
    let nested = site.html("letters/2025/two/index.html");
    assert!(nested.contains("2025 shelf") && !nested.contains("arrives on Wednesday"), "{nested}");
}

#[test]
fn the_aside_follows_the_page_end_sections_and_precedes_comments() {
    let site = Site::new(&[
        ("letters/end.md", END),
        ("letters/one.md", "---\ntitle: One\ndate: 2026-10-01\nnext: \"[[two]]\"\n---\n\nThe body of letter one.\n"),
        ("letters/two.md", &letter("Two")),
    ]);
    site.build();

    let html = site.html("letters/one/index.html");
    let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("no {needle}: {html}"));
    assert!(at("The body of letter one.") < at("moss-field-end"), "the body leads");
    assert!(at("moss-field-end") < at(END_OPEN), "the block follows the page's own end sections");
    assert!(at(END_OPEN) < at("</article>"), "the block is inside the article");
    assert!(at("</article>") < at("id=\"moss-comments\""), "comments follow the article");
}

#[test]
fn the_feed_and_the_search_index_leave_the_block_out() {
    let site = Site::new(&[
        ("letters/end.md", END),
        ("letters/one.md", "---\ntitle: One\ndate: 2026-10-01\n---\n\nThe harbour pavilion letter.\n"),
    ]);
    site.build();

    assert!(has_block(&site.html("letters/one/index.html")), "the page itself carries the block");

    let feed = site.html("rss.xml");
    assert!(feed.contains("harbour pavilion"), "the feed carries the letter: {feed}");
    assert!(!feed.contains("arrives on Wednesday"), "the feed must not carry the block: {feed}");

    let search = search_fragments(&site);
    assert!(search.contains("harbour"), "the index carries the letter: {search}");
    assert!(!search.contains("Wednesday"), "the index must not carry the block: {search}");
}

/// The article map records each page's own markdown and body HTML, which are
/// what a copy of it is shaped from.
#[test]
fn the_article_map_records_the_page_without_the_block() {
    let site = Site::new(&[
        ("letters/end.md", END),
        ("letters/one.md", "---\ntitle: One\ndate: 2026-10-01\n---\n\nThe harbour pavilion letter.\n"),
    ]);
    site.build();

    assert!(has_block(&site.html("letters/one/index.html")), "the page itself carries the block");
    let map: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(site.0.join(".moss/build.nosync/article-map.json")).unwrap()).unwrap();
    let article = map["articles"]
        .as_object()
        .and_then(|articles| articles.values().find(|a| a["source_path"] == "letters/one.md"))
        .unwrap_or_else(|| panic!("no entry for the letter: {map}"));
    for field in ["content", "html_content"] {
        let text = article[field].as_str().unwrap_or_else(|| panic!("no {field}: {article}"));
        assert!(text.contains("harbour pavilion"), "{field}: {text}");
        assert!(!text.contains("arrives on Wednesday"), "{field}: {text}");
    }
}

#[test]
fn a_language_tree_block_closes_only_its_own_folder_of_that_tree() {
    let site = Site::new(&[
        ("end.md", "---\npartial: true\nslot: before-article-end\n---\nThe default language's block.\n"),
        ("letters/one.md", &letter("One")),
        ("de/index.md", "---\ntitle: Start\nlang: de\n---\n\nWillkommen.\n"),
        ("de/letters/end.md", "---\npartial: true\nslot: before-article-end\n---\nDer nächste Brief.\n\n:::subscribe {button=\"Abonnieren\"}\n:::\n"),
        ("de/letters/eins.md", "---\ntitle: Eins\ndate: 2026-10-01\nlang: de\n---\n\nEin Brief.\n"),
        ("de/notes/drei.md", "---\ntitle: Drei\ndate: 2026-10-01\nlang: de\n---\n\nEine Notiz.\n"),
    ]);
    site.build();

    let german = site.html("de/letters/eins/index.html");
    assert!(german.contains("Der nächste Brief") && !german.contains("default language's block"), "{german}");
    assert!(german.contains("name=\"scope\" value=\"de\""), "the form signs up the tree's own subscribers: {german}");
    assert!(!has_block(&site.html("de/notes/drei/index.html")), "a tree has no fallback to the site's root block");
    let english = site.html("letters/one/index.html");
    assert!(english.contains("default language's block") && !english.contains("Der nächste Brief"), "{english}");
}

#[test]
fn a_translation_key_section_is_its_own_root_for_end_blocks() {
    let home = |title: &str, lang: &str| format!("---\ntitle: {title}\nlang: {lang}\ntranslationKey: home\n---\n\nHello.\n");
    let page = |title: &str, lang: &str| format!("---\ntitle: {title}\ndate: 2026-10-01\nlang: {lang}\n---\n\nA letter.\n");
    let site = Site::new(&[
        ("index.md", &home("Home", "en")),
        ("end.md", "---\npartial: true\nslot: before-article-end\n---\nThe root block.\n\n:::subscribe\n:::\n"),
        ("letters/one.md", &page("One", "en")),
        ("journal/index.md", &home("Journal", "fr")),
        ("journal/letters/un.md", &page("Un", "fr")),
        ("revue/index.md", &home("Revue", "fr")),
        ("revue/end.md", "---\npartial: true\nslot: before-article-end\n---\nLe bloc de la revue.\n\n:::subscribe\n:::\n"),
        ("revue/lettres/deux.md", &page("Deux", "fr")),
    ]);
    site.build();

    assert!(!has_block(&site.html("journal/letters/un/index.html")), "a section has no fallback to the site's root block");
    let local = site.html("revue/lettres/deux/index.html");
    assert!(local.contains("Le bloc de la revue") && !local.contains("The root block"), "{local}");
    assert!(local.contains("name=\"scope\" value=\"revue\""), "the form signs up the section's own subscribers: {local}");
    let ordinary = site.html("letters/one/index.html");
    assert!(ordinary.contains("The root block"), "an ordinary folder still inherits the root block: {ordinary}");
}

#[test]
fn a_block_keeps_its_headings_and_rules_as_written() {
    let site = Site::new(&[
        ("letters/end.md", "---\npartial: true\nslot: before-article-end\n---\n## Keep reading\n\nMore letters.\n\n---\n\nSmall print.\n"),
        ("letters/one.md", &letter("One")),
    ]);
    site.build();

    let html = site.html("letters/one/index.html");
    let block = &html[html.find(END_OPEN).unwrap()..html.find("</aside>").unwrap()];
    assert!(block.contains("Keep reading") && block.contains("<hr"), "{block}");
    assert!(!block.contains("<section>") && !block.contains("footer-bottom"), "not footer columns: {block}");
}

#[test]
fn a_hero_ending_a_block_stays_in_the_block() {
    let site = Site::new(&[
        ("letters/end.md", "---\npartial: true\nslot: before-article-end\n---\nOne more thing.\n\n:::hero {align=top}\n## The shelf is open.\n:::\n"),
        ("letters/one.md", &letter("One")),
    ]);
    site.build();

    let html = site.html("letters/one/index.html");
    assert!(html.contains("The shelf is open."), "{html}");
    assert!(!html.contains("moss-hero-closing"), "only a footer's hero closes the page: {html}");
}

/// On a site whose config starts no service, where slot resolution would
/// otherwise be skipped. The problem count is process-wide, so this is the one
/// test here that builds a file with a problem.
#[test]
fn a_file_without_partial_true_is_unused_stays_a_page_and_is_reported() {
    let site = Site::new(&[
        ("letters/note.md", "---\ntitle: Note\nslot: before-article-end\n---\nA page aimed at the slot.\n"),
        ("letters/one.md", &letter("One")),
    ]);
    site.write(".moss/config.toml", PLAIN_CONFIG);
    let _ = moss_build::build::cli_output::take_cli_problems();
    site.build();

    assert!(moss_build::build::cli_output::take_cli_problems() >= 1, "counts toward --strict");
    assert!(!has_block(&site.html("letters/one/index.html")));
    assert!(site.output("letters/note/index.html").exists(), "the file stays an ordinary page");
}

#[test]
fn a_deleted_block_does_not_come_back_on_the_next_build() {
    let site = Site::new(&[("letters/end.md", END), ("letters/one.md", &letter("One"))]);
    site.build();
    assert!(has_block(&site.html("letters/one/index.html")), "sanity: the first build draws it");

    fs::remove_file(site.0.join("letters/end.md")).unwrap();
    site.build();
    assert!(!has_block(&site.html("letters/one/index.html")), "a deleted block must not return");
}

/// A pre-Sonoma iCloud eviction: the file's real name leaves the walk and a
/// placeholder takes its place.
#[cfg(target_os = "macos")]
#[test]
fn a_block_still_downloading_keeps_its_content() {
    let site = Site::new(&[("letters/end.md", END), ("letters/one.md", &letter("One"))]);
    // No service that would run slot resolution on its own.
    site.write(".moss/config.toml", PLAIN_CONFIG);
    site.build();

    let evict = |dir: &Path| {
        fs::remove_file(dir.join("end.md")).unwrap();
        fs::write(dir.join(".end.md.icloud"), "").unwrap();
    };
    evict(&site.0.join("letters"));
    site.write("letters/one.md", "---\ntitle: One\ndate: 2026-10-01\n---\n\nAn edited letter.\n");
    site.build();

    let html = site.html("letters/one/index.html");
    assert!(html.contains("An edited letter"), "the page itself rebuilt");
    assert!(has_block(&html), "a block mid-download must not vanish: {html}");
}
