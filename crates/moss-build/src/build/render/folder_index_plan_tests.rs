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
    let kinds = crate::build::terms::term_kinds(&crate::config::ConfigFile::parse("").unwrap(), Language::En);
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
    for expected in ["nested/index.html", "nested/deep/index.html", "assets/index.html", "empty/index.html", "archive/empty/index.html", "authors/index.html", "authors/ada/index.html"] {
        assert!(generated.contains(expected), "missing generated tree entry {expected}: {generated:?}");
        assert!(output.join(expected).exists(), "missing emitted index {expected}");
    }
    assert!(!generated.contains("archive/index.html"), "an authored index remains authored");
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
        &mut documents, &project, &Default::default(), &Default::default(), &evidence,
        |folder| ParsedDocument { url_path: format!("{folder}/index.html"), ..Default::default() },
    );
    let folders: BTreeSet<&str> = plan.entries.iter().map(|entry| entry.folder.as_str()).collect();
    assert!(folders.contains("empty"), "a genuinely empty directory still gets an index");
    assert!(!folders.contains("pending"), "a pending-only directory cannot masquerade as finished");
    assert!(!folders.contains("mixed"), "a pending authored index cannot be replaced by a synthetic one");
}
