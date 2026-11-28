//! Core's `site_pages`, read from a folder's texts and bundle, must give every
//! page the facts the build gave it, on every snapshot fixture.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_core::page::site_page::PageFacts;

/// What one build run knew once every page was read, keyed by site folder.
pub(crate) struct Captured {
    pub root_name: String,
    pub pages: BTreeMap<String, PageFacts>,
    /// Each listing host's own listing, by source path: what it asks the
    /// selector, and the addresses the build's selection gave.
    pub listings: BTreeMap<String, (Listing, Vec<String>)>,
    /// Each page's richness and listing cover by address, which core reads
    /// from the cards it draws rather than from its facts.
    pub cards: HashMap<String, (bool, bool)>,
    pub has_language_trees: bool,
    /// The kinds the build derived its terms from.
    pub kinds: Vec<crate::build::terms::TermKind>,
    /// Each page's term lines and place names, by source path.
    pub lines: BTreeMap<String, (Vec<moss_core::terms::lines::TermLine>, Option<String>)>,
}

pub(crate) struct Listing {
    pub folder: String,
    pub flatten: bool,
    pub scoped: bool,
    pub edition: String,
    /// The build's own listing as shown: its addresses, style and group.
    pub shown: Option<(Vec<String>, moss_core::Resolved<String>, String)>,
}

/// The minute both sides order upcoming events against.
const NOW: &str = "2026-06-15T12:00";

fn now() -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::parse_from_str(NOW, "%Y-%m-%dT%H:%M").unwrap()
}

static CAPTURED: Mutex<Vec<(PathBuf, Captured)>> = Mutex::new(Vec::new());

pub(crate) fn record(
    root: &crate::vault::paths::VaultRoot,
    docs: &[crate::build::types::ParsedDocument],
    terms: &crate::build::terms::TermIndex,
    project: &crate::types::content::ProjectStructure,
) {
    let pages = docs.iter().filter_map(|d| Some((d.source_path.clone()?, PageFacts::from_page(d)))).collect();
    // Core draws no term page's listing yet.
    let term_folders: std::collections::HashSet<&str> = terms.term_keys().chain(terms.roots_in_use()).collect();
    use crate::build::render::incremental::listing::{own_listing_key, Depth};
    let listings = docs.iter()
        .filter_map(|d| Some((d.source_path.clone()?, own_listing_key(d, docs)?)))
        .filter(|(_, key)| !term_folders.contains(key.folder_slug.as_str()))
        .map(|(path, key)| {
            let flatten = key.depth == Depth::All;
            let picked = crate::build::folder_embed::select_children_by_slug(&key.folder_slug, flatten, key.scope_default_tree, key.exclude_nav, &key.edition, docs, project);
            let shown = docs.iter().find(|d| d.source_path.as_deref() == Some(path.as_str()))
                .and_then(|host| crate::build::folder_embed::members::host_listing(host, docs, project, now()))
                .map(|o| (o.limited.iter().map(|d| d.url_path.clone()).collect(), o.style, o.group));
            let listing = Listing { folder: key.folder_slug, flatten, scoped: key.scope_default_tree, edition: key.edition, shown };
            (path, (listing, picked.iter().map(|d| d.url_path.clone()).collect()))
        })
        .collect();
    let cards = docs.iter()
        .map(|d| (d.url_path.clone(), (crate::build::folder_embed::is_rich_child(d, true), d.listing_cover().is_some())))
        .collect();
    let lines = docs.iter().filter_map(|d| Some((d.source_path.clone()?, (d.term_lines.clone(), d.place_names.clone())))).collect();
    let captured = Captured { root_name: root.name().to_string(), pages, listings, cards, has_language_trees: project.has_language_trees, kinds: terms.kinds().to_vec(), lines };
    if let Ok(mut all) = CAPTURED.lock() {
        all.push((root.path().to_path_buf(), captured));
    }
}

fn take(root: &Path) -> Option<Captured> {
    let mut all = CAPTURED.lock().ok()?;
    let i = all.iter().position(|(p, _)| p == root)?;
    Some(all.swap_remove(i).1)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn site_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            site_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn build(folder: &Path) {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    let path = folder.to_string_lossy().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _ = rt.block_on(run_pipeline(PipelineConfig {
        root: crate::vault_root::VaultRoot::resolve(folder),
        progress: crate::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: crate::cli::host::cli_host_ports(&path),
        trigger: BuildTrigger::Full,
        reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }));
}

/// The trail or site name and the nav bar's page items, without the toggles
/// and the whitespace between tags.
fn masthead_of(html: &str) -> Option<String> {
    let start = html.find(r#"<div class="nav-left""#)?;
    let end = start + html[start..].find("</nav>")?;
    let mut out = html[start..end].to_string();
    if let Some(at) = out.find(r#"<button class="mobile-menu-button""#) {
        let close = at + out[at..].find("</button>")? + "</button>".len();
        out.replace_range(at..close, "");
    }
    Some(regex::Regex::new(r">\s+<").unwrap().replace_all(out.trim(), "><").into_owned())
}

/// A page's folder title, or nothing when it draws none.
fn folder_title_of(html: &str) -> String {
    let Some(start) = html.find(r#"<h1 class="moss-folder-title"#) else { return String::new() };
    html[start..].find("</h1>").map_or_else(String::new, |end| html[start..start + end + "</h1>".len()].to_string())
}

/// Fixture × field mismatch counts, and the first few differing pages.
#[test]
fn core_site_pages_match_the_builds_pages_on_every_fixture() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshot-sites");
    let mut names: Vec<String> = std::fs::read_dir(&fixtures).unwrap().flatten()
        .filter(|e| e.path().join("input").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    let scratch = std::env::temp_dir().join(format!("moss_site_pages_{}", std::process::id()));
    let mut report = String::new();
    let mut failures = 0;
    let mut compared = 0;
    let mut mastheads = 0;
    let (mut no_expected_file, mut forced_trails, mut one_sided) = (0, 0, 0);
    let mut masthead_report = String::new();
    let (mut headings, mut explorer_roots, mut heading_report) = (0, 0, String::new());
    let (mut listings, mut listing_report) = (0, String::new());
    let (mut shown_rows, mut styles) = (0, std::collections::BTreeSet::new());
    let (mut term_lines, mut place_names, mut lines_report) = (0, 0, String::new());
    for name in &names {
        let folder = scratch.join(name).join("site");
        copy_dir(&fixtures.join(name).join("input"), &folder);
        build(&folder);
        let root = crate::vault_root::VaultRoot::resolve(&folder);
        let Some(captured) = take(root.path()) else {
            report.push_str(&format!("{name}: no capture\n"));
            failures += 1;
            continue;
        };
        let mut files = Vec::new();
        site_files(&folder, &folder, &mut files);
        let texts: HashMap<String, String> = files.iter()
            .filter(|f| moss_core::source_edit::policy::is_page_path(Path::new(f)))
            .filter_map(|f| Some((f.clone(), std::fs::read_to_string(folder.join(f)).ok()?)))
            .collect();
        let config_toml = std::fs::read_to_string(folder.join(".moss/config.toml")).ok();
        let places_toml = std::fs::read_to_string(folder.join(".moss/places.toml")).ok();
        let markdown: Vec<crate::types::content::FileInfo> = files.iter().filter(|f| f.ends_with(".md"))
            .map(|f| crate::types::content::FileInfo { path: f.clone(), file_type: "md".into(), size: 0, modified: None })
            .collect();
        let detected_lang = Some(crate::i18n::detect::resolve_site_lang(None, None, &markdown, &folder.to_string_lossy()).code);
        let (bundle, _) = moss_core::page::site_bundle(moss_core::page::SiteSources {
            files, texts: texts.clone(), config_toml, places_toml, detected_lang,
            detected: texts.iter().map(|(p, t)| (p.clone(), crate::i18n::detect::detected(t))).collect(),
            ..Default::default()
        });
        if bundle.terms != captured.kinds {
            report.push_str(&format!("{name}: term kinds\n  build {:?}\n  core  {:?}\n", captured.kinds, bundle.terms));
            failures += 1;
        }
        let site = moss_core::page::site_page::site_pages(&texts, &bundle, &captured.root_name);
        for (path, page) in site.paths.iter().zip(&site.pages) {
            let pretty = moss_core::resolve::output_url::to_pretty_url(&page.url_path);
            let Ok(html) = std::fs::read_to_string(fixtures.join(name).join("expected").join(pretty.trim_matches('/')).join("index.html")) else {
                no_expected_file += 1;
                continue;
            };
            // A places explorer root hides its title for the map, which the preview does not draw.
            let explorer_root = html.contains(r#"class="moss-place-map""#) && html.contains(r#"<h1 class="moss-folder-title visually-hidden""#);
            explorer_roots += usize::from(explorer_root);
            if page.is_folder_index && !explorer_root && !html.contains("moss-collection-cover") {
                let built = folder_title_of(&html).replace("[TEST_DIR]", &captured.root_name);
                let core = site.folder_heading(path, moss_core::frontmatter::body(&texts[path])).unwrap_or_default();
                headings += 1;
                if built != core {
                    heading_report.push_str(&format!("{name} {path}\n  build {built}\n  core  {core}\n"));
                }
            }
            // A page the build draws without a masthead (`header: none`, a bare page).
            let (Some(built), Some(core)) = (masthead_of(&html), site.masthead(path).as_deref().and_then(masthead_of)) else {
                one_sided += 1;
                continue;
            };
            // The fixtures spell the folder's name `[TEST_DIR]`; a trail forced
            // onto a places root, the one trail that shows the current page, is
            // not the preview's.
            let built = built.replace("[TEST_DIR]", &captured.root_name);
            if built.contains("breadcrumb-current") {
                forced_trails += 1;
                continue;
            }
            // The published page drops the editor's source annotations.
            let core = regex::Regex::new(r#" data-source-(file|fm)="[^"]*""#).unwrap().replace_all(&core, "").into_owned();
            mastheads += 1;
            if built != core {
                masthead_report.push_str(&format!("{name} {path}\n  build {built}\n  core  {core}\n"));
            }
        }
        assert_eq!(site.has_language_trees, captured.has_language_trees, "{name}: has_language_trees");
        for (path, (listing, built)) in &captured.listings {
            let Some(page) = site.paths.iter().position(|p| p == path).map(|i| &site.pages[i]) else { continue };
            let own = moss_core::page::listing::own_listing(page, &site.pages);
            let expected = moss_core::page::listing::OwnListing { folder: listing.folder.clone(), flatten: listing.flatten, scoped: listing.scoped };
            if own.as_ref() != Some(&expected) {
                listing_report.push_str(&format!("{name} {path}\n  build {expected:?}\n  core  {own:?}\n"));
                continue;
            }
            let core: Vec<String> = moss_core::page::listing::select(&expected.scope(&listing.edition), &site.pages, site.listing_site()).iter().map(|p| p.url_path.clone()).collect();
            listings += 1;
            if &core != built {
                listing_report.push_str(&format!("{name} {path}\n  build {built:?}\n  core  {core:?}\n"));
                continue;
            }
            let card = |p: &PageFacts| captured.cards.get(&p.url_path).copied().unwrap_or_default();
            let shown = moss_core::page::listing::host_listing(page, &site.pages, site.listing_site(), NOW, |p| card(p).0, |p| card(p).1)
                .map(|o| (o.limited.iter().map(|p| p.url_path.clone()).collect::<Vec<_>>(), o.style, o.group));
            if let Some((rows, style, group)) = &shown {
                shown_rows += rows.len();
                styles.insert(format!("{}/{group}", style.value));
            }
            if shown != listing.shown {
                listing_report.push_str(&format!("{name} {path} shown\n  build {:?}\n  core  {shown:?}\n", listing.shown));
            }
        }
        for (path, page) in site.paths.iter().zip(&site.pages) {
            let Some((built_lines, built_places)) = captured.lines.get(path) else { continue };
            let core = (moss_core::terms::lines::term_lines(page, &site.terms, site.site_lang), moss_core::terms::lines::place_names(page, &site.terms));
            term_lines += built_lines.len();
            place_names += usize::from(built_places.is_some());
            if (&core.0, &core.1) != (built_lines, built_places) {
                lines_report.push_str(&format!("{name} {path}\n  build {built_lines:?} {built_places:?}\n  core  {:?} {:?}\n", core.0, core.1));
            }
        }
        let core: BTreeMap<String, PageFacts> = site.paths.into_iter().zip(site.pages).collect();
        assert!(!captured.pages.is_empty() || texts.is_empty(), "{name}: the build captured no pages");
        compared += captured.pages.len();
        let mut fields: BTreeMap<String, usize> = BTreeMap::new();
        let mut examples = Vec::new();
        for (path, built) in &captured.pages {
            let built_json = serde_json::to_value(built).unwrap();
            let core_json = core.get(path).map(|c| serde_json::to_value(c).unwrap());
            let Some(core_json) = core_json else {
                *fields.entry("<missing>".into()).or_default() += 1;
                examples.push(format!("  {path}: not in core's pages"));
                continue;
            };
            for (key, value) in built_json.as_object().unwrap() {
                if core_json.get(key) != Some(value) {
                    *fields.entry(key.clone()).or_default() += 1;
                    if examples.len() < 6 {
                        examples.push(format!("  {path} {key}: build {value} core {}", core_json.get(key).unwrap_or(&serde_json::Value::Null)));
                    }
                }
            }
        }
        for path in core.keys().filter(|p| !captured.pages.contains_key(*p)) {
            *fields.entry("<extra>".into()).or_default() += 1;
            if examples.len() < 6 {
                examples.push(format!("  {path}: only in core's pages"));
            }
        }
        if !fields.is_empty() {
            failures += 1;
            report.push_str(&format!("{name}: {fields:?}\n{}\n", examples.join("\n")));
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    if let Ok(out) = std::env::var("SITE_PAGES_REPORT") {
        let _ = std::fs::write(out, &report);
    }
    if let Ok(out) = std::env::var("MASTHEAD_REPORT") {
        let _ = std::fs::write(out, &masthead_report);
    }
    eprintln!("folder headings compared {headings}, skipped: {explorer_roots} places explorer roots");
    eprintln!("mastheads compared {mastheads}, skipped: {forced_trails} forced places-root trails, {one_sided} without a masthead, {no_expected_file} pages without an expected file");
    assert!(compared > 0, "no page was compared");
    assert!(mastheads > 0, "no masthead was compared");
    assert!(headings > 0, "no folder heading was compared");
    eprintln!("listings compared {listings}, {shown_rows} rows shown, styles/groups {styles:?}");
    assert!(listings > 0, "no listing was compared");
    eprintln!("term lines compared {term_lines}, place names {place_names}");
    assert!(term_lines > 0 && place_names > 0, "no term line or place name was compared");
    assert!(lines_report.is_empty(), "core's term lines differ from the build's:\n{lines_report}");
    assert!(listing_report.is_empty(), "core's listing selection differs from the build's:\n{listing_report}");
    assert!(heading_report.is_empty(), "the preview's folder heading differs from the build's:\n{heading_report}");
    assert!(masthead_report.is_empty(), "the preview's masthead differs from the build's:\n{masthead_report}");
    assert_eq!(failures, 0, "core's pages differ from the build's in {failures} of {} fixtures:\n{report}", names.len());
}
