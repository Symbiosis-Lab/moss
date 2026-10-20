use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::build::manifest::PendingManifest;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::types::content::SiteHashes;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn build(root: &Path) -> (std::path::PathBuf, Vec<ParsedDocument>) {
    let project = crate::build::scan::scan::scan_folder(root.to_str().unwrap()).unwrap();
    let output = root.join(".moss/build.nosync/site");
    fs::create_dir_all(&output).unwrap();
    let kinds = crate::build::terms::term_kinds(&crate::config::ConfigFile::parse("").unwrap(), "en");
    let (_, _, documents, _) = super::super::blocking::generate_blocking_content(
        &crate::vault::paths::VaultRoot::resolve(root), &project, &output,
        None, None, true,
        super::super::config::SiteConfig { term_kinds: kinds, ..Default::default() },
        &mut PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(root)),
    ).unwrap();
    (output, documents)
}

fn generated(documents: &[ParsedDocument]) -> BTreeSet<&str> {
    documents.iter().filter(|d| d.source_path.is_none() && d.url_path != "index.html")
        .map(|d| d.url_path.as_str()).collect()
}

#[test]
fn generated_tree_and_output_agree_for_nested_empty_assets_terms_and_mapped_folders() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    write(root.path(), "Nested/Deep/story.md", "---\ntitle: Story\nauthor: Ada\n---\nA story.");
    write(root.path(), "Assets/notes.txt", "asset bytes");
    fs::create_dir(root.path().join("Empty")).unwrap();
    write(root.path(), "Authored/index.md", "---\ntitle: Authored\nurl: archive\n---\nIntro.");
    fs::create_dir_all(root.path().join("Authored/Empty")).unwrap();
    let (output, documents) = build(root.path());
    let generated = generated(&documents);
    for expected in ["nested/index.html", "nested/deep/index.html", "assets/index.html", "authors/index.html", "authors/ada/index.html"] {
        assert!(generated.contains(expected), "missing generated tree entry {expected}: {generated:?}");
        assert!(output.join(expected).exists(), "missing emitted index {expected}");
    }
    assert!(!generated.contains("archive/index.html"), "an authored index remains authored");
    assert!(!generated.contains("empty/index.html") && !generated.contains("archive/empty/index.html"), "empty folders publish nothing: {generated:?}");
    for doc in documents.iter().filter(|d| d.source_path.is_none()) {
        assert!(output.join(&doc.url_path).exists(), "tree entry has no output: {}", doc.url_path);
        assert!(doc.lang_tag.is_some(), "generated entry lacks content language: {}", doc.url_path);
    }
}

#[test]
fn indexless_language_tree_keeps_its_served_indexes_and_gains_typed_tree_entries() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\nlang: en\nbreadcrumb: false\n---\n# Garden");
    write(root.path(), "zh-hans/notes/story.md", "---\ntitle: 故事\nlang: zh-hans\n---\n这是故事。");
    let (output, documents) = build(root.path());
    for expected in ["zh-hans/index.html", "zh-hans/notes/index.html"] {
        let doc = documents.iter().find(|d| d.url_path == expected).expect("generated edition index belongs in the tree");
        assert_eq!(doc.source_path, None);
        assert_eq!(doc.lang_tag.as_deref(), Some("zh-Hans"));
        assert!(output.join(expected).exists(), "existing served index must survive");
        let html = fs::read_to_string(output.join(expected)).unwrap();
        assert!(html.contains(r#"lang="zh-Hans""#), "content tag must describe its edition");
    }
    let story = fs::read_to_string(output.join("zh-hans/notes/story/index.html")).unwrap();
    assert!(story.contains("<title>故事 - Garden</title>"), "a synthetic edition root cannot invent a translated site brand");
    assert!(story.contains(r#"<meta property="og:site_name" content="Garden">"#));
    assert!(story.contains(r#"class="site-name"#) && story.contains(">Garden</a>"));
    let homepage = fs::read_to_string(output.join("index.html")).unwrap();
    assert!(homepage.contains(r#"href="/zh-hans/" class="nav-lang-link" hreflang="zh-Hans""#), "a published synthesized front door is reachable from the English edition");
    assert!(story.contains(r#"href="/" class="nav-lang-link" hreflang="en""#), "the published Chinese edition can return to English");
    let notes = fs::read_to_string(output.join("zh-hans/notes/index.html")).unwrap();
    assert!(notes.contains("zh-hans/"), "breadcrumb and navigation can find the represented root");
}

#[test]
fn translated_root_has_no_phantom_generated_descendants() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\nlang: en\ntranslationKey: home\n---\n# Garden");
    write(root.path(), "edition/index.md", "---\ntitle: Jardin\nlang: fr\ntranslationKey: home\n---\n# Jardin");
    write(root.path(), "edition/notes/story.md", "---\ntitle: Histoire\nlang: fr\n---\nUne histoire.");
    let (output, documents) = build(root.path());
    assert!(output.join("edition/index.html").exists());
    assert!(output.join("edition/notes/story/index.html").exists());
    assert!(!output.join("edition/notes/index.html").exists(), "preserve translation-root suppression");
    assert!(!generated(&documents).contains("edition/notes/index.html"), "a suppressed index cannot remain in the page tree");
}

#[test]
fn slot_sources_cannot_create_virtual_content_folders() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    write(root.path(), "footer.md", "---\nurl: ghost/deep/footer\n---\nFooter copy.");
    let (output, documents) = build(root.path());
    assert!(documents.iter().any(|d| d.slot_only));
    assert!(!generated(&documents).iter().any(|url| url.starts_with("ghost/")));
    assert!(!output.join("ghost/index.html").exists());
}

#[test]
fn generated_and_authored_nested_pages_link_the_same_favicon_artifacts() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    write(root.path(), "notes/story.md", "---\ntitle: Story\n---\nA story.");
    write(root.path(), ".moss/theme/style.css", "body { color: red; }");
    write(root.path(), ".moss/theme/script.js", "window.themeLoaded = true;");
    let (output, _) = build(root.path());
    let generated = fs::read_to_string(output.join("notes/index.html")).unwrap();
    let authored = fs::read_to_string(output.join("notes/story/index.html")).unwrap();
    for tag in [
        r#"<link rel="icon" type="image/png" sizes="32x32" href="/assets/favicon-32.png">"#,
        r#"<link rel="icon" type="image/png" sizes="16x16" href="/assets/favicon-16.png">"#,
        r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/favicon-180.png">"#,
    ] {
        assert!(generated.contains(tag), "generated chrome omitted {tag}");
        assert!(authored.contains(tag), "authored chrome omitted {tag}");
    }
    for html in [&generated, &authored] {
        assert!(html.contains("/_moss/theme/style."));
        assert!(html.contains("/_moss/theme/script."));
        assert!(html.contains("/_moss/js/theme."));
        assert!(html.contains("Garden"));
    }
    assert!(!generated.contains(r#"<link rel="canonical""#), "generated folder metadata policy remains unchanged");
    assert!(generated.contains("\n    <link rel=\"stylesheet\" href=\"/_moss/theme/style."), "preserve generated theme-link whitespace");
}

#[test]
fn pending_page_sources_suppress_only_their_generated_folder_indexes() {
    let root = tempfile::tempdir().unwrap();
    for folder in ["empty", "pending", "mixed"] {
        fs::create_dir(root.path().join(folder)).unwrap();
    }
    write(root.path(), "index.md", "# Garden");
    write(root.path(), "mixed/story.md", "A ready story.");
    let project = crate::build::scan::scan::scan_folder(root.path().to_str().unwrap()).unwrap();
    let evidence = crate::build::cloud_ledger::InputEvidence::new(root.path());
    for source in ["pending/story.md", "mixed/index.md"] {
        evidence.require(&root.path().join(source), crate::build::cloud_ledger::InputRole::PageContent);
    }
    let mut documents = vec![ParsedDocument {
        source_path: Some("mixed/story.md".to_string()),
        url_path: "mixed/story/index.html".to_string(),
        ..Default::default()
    }];
    let plan = super::FolderIndexPlan::resolve(
        &mut documents, &project, &Default::default(), &Default::default(), &Default::default(), &evidence,
        |folder| ParsedDocument { url_path: format!("{folder}/index.html"), ..Default::default() },
    );
    let folders: BTreeSet<&str> = plan.entries.iter().map(|entry| entry.folder.as_str()).collect();
    assert!(!folders.contains("empty"), "a genuinely empty directory publishes no index");
    assert!(!folders.contains("pending"), "a pending-only directory cannot masquerade as finished");
    assert!(!folders.contains("mixed"), "a pending authored index cannot be replaced by a synthetic one");
}


#[test]
fn draft_translation_pair_cannot_publish_a_generated_language_edition() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Home\n---\n");
    write(root.path(), "notes/story.md", "---\ntitle: Story\n---\n这是一篇讨论文化与生态之间关系的中文文章，以及这种关系如何影响我们对世界的理解和思考方式。");
    write(root.path(), "about.md", "---\ntitle: About\nlang: en\ndraft: true\n---\nA draft introduction.");
    write(root.path(), "about.zh-hans.md", "---\ntitle: 关于\nlang: zh-hans\ndraft: true\n---\n这是介绍页面的草稿。");
    let (output, documents) = build(root.path());
    let with_switcher: BTreeSet<&str> = documents.iter().filter(|doc| {
        fs::read_to_string(output.join(&doc.url_path)).unwrap().contains("nav-lang-toggle")
    }).map(|doc| doc.url_path.as_str()).collect();
    assert_eq!(with_switcher, BTreeSet::from(["about/index.html", "zh-hans/about/index.html"]),
        "authored draft translations keep their links; no generated root publishes their edition");
    assert!(output.join("zh-hans/index.html").exists(), "the existing served index survives");
}

#[test]
fn draft_only_language_tree_cannot_publish_a_generated_language_edition() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\nlang: en\n---\nAn English homepage.");
    write(root.path(), "zh-hans/notes/story.md", "---\ntitle: 故事\nlang: zh-hans\ndraft: true\n---\n这是故事草稿。");
    let (output, documents) = build(root.path());
    assert!(output.join("zh-hans/index.html").exists(), "draft exclusion must not delete served paths");
    for doc in &documents {
        let html = fs::read_to_string(output.join(&doc.url_path)).unwrap();
        assert!(!html.contains("nav-lang-toggle"), "a draft-only edition cannot add a root fallback on {}", doc.url_path);
    }
}

#[test]
fn folder_with_no_file_beneath_it_publishes_no_page() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    fs::create_dir(root.path().join("empty")).unwrap();
    fs::create_dir_all(root.path().join("nested/deeper")).unwrap();
    write(root.path(), "keep/photo.png", "x");
    let (output, documents) = build(root.path());
    let generated = generated(&documents);
    for gone in ["empty/index.html", "nested/index.html", "nested/deeper/index.html"] {
        assert!(!generated.contains(gone), "an empty folder must not get {gone}: {generated:?}");
        assert!(!output.join(gone).exists(), "an empty folder must not emit {gone}");
    }
    assert!(generated.contains("keep/index.html"), "an image-only folder keeps its page: {generated:?}");
    assert!(output.join("keep/index.html").exists());
}

#[test]
fn a_build_names_each_empty_folder_it_left_out() {
    use crate::build::ports::reporter::BuildReporter;
    use crate::build::progress::PipelineEvent;
    #[derive(Default)]
    struct Recording(std::sync::Mutex<Vec<PipelineEvent>>);
    impl BuildReporter for Recording {
        fn report(&self, event: &PipelineEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    fs::create_dir_all(root.path().join("drafts/later")).unwrap();
    let project = crate::build::scan::scan::scan_folder(root.path().to_str().unwrap()).unwrap();
    let output = root.path().join(".moss/build.nosync/site");
    fs::create_dir_all(&output).unwrap();
    let kinds = crate::build::terms::term_kinds(&crate::config::ConfigFile::parse("").unwrap(), "en");
    let recording = std::sync::Arc::new(Recording::default());
    let mut services = crate::types::services::BuildServices::headless();
    services.reporter = recording.clone();
    super::super::blocking::generate_blocking_content(
        &crate::vault::paths::VaultRoot::resolve(root.path()), &project, &output,
        Some(&services), None, true,
        super::super::config::SiteConfig { term_kinds: kinds, ..Default::default() },
        &mut PendingManifest::for_build(SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(root.path())),
    ).unwrap();
    let named: Vec<String> = recording.0.lock().unwrap().iter()
        .filter_map(|e| match e { PipelineEvent::BackgroundProgress { advisories, .. } => Some(advisories.clone()), _ => None })
        .flatten()
        .filter_map(|a| a.item)
        .collect();
    assert_eq!(named, vec!["drafts".to_string()], "the build names the empty folder once, topmost");
}


#[test]
fn passthrough_folder_and_its_subfolders_get_no_listing_but_their_pages_render() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "index.md", "---\ntitle: Garden\n---\n# Garden");
    write(root.path(), ".moss/config.toml", "[build]\npassthrough = [\"scene\"]\n");
    write(root.path(), "scene/app.js", "var a = 1;");
    write(root.path(), "scene/credits.md", "---\ntitle: Credits\n---\nThanks.");
    write(root.path(), "scene/sketch/README.md", "---\ntitle: Sketch\n---\nNotes.");
    write(root.path(), "scene/deep/more/page.md", "---\ntitle: Page\n---\nText.");
    write(root.path(), "notes/entry.md", "---\ntitle: Entry\n---\nText.");
    let (output, documents) = build(root.path());
    let generated = generated(&documents);
    for gone in ["scene/index.html", "scene/deep/index.html", "scene/deep/more/index.html"] {
        assert!(!generated.contains(gone), "passthrough folder got a listing {gone}: {generated:?}");
        let written = std::fs::read_to_string(output.join(gone)).unwrap_or_default();
        assert!(written.is_empty() || written.contains("http-equiv=\"refresh\""), "listing written for {gone}");
    }
    assert!(generated.contains("notes/index.html"), "an ordinary folder keeps its listing: {generated:?}");
    for page in ["scene/credits/index.html", "scene/sketch/index.html", "scene/deep/more/page/index.html"] {
        assert!(output.join(page).exists(), "markdown inside a passthrough folder must render: {page}");
    }
}
