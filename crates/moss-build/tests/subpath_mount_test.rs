//! A site served under a path. One site is built at `/`, `/~alice/` and
//! `/a/b/`, and what a publish would upload (the sealed generation) is served
//! the way a plain static file server does it: under the prefix only, with no
//! fallback to the origin's root. Every URL the HTML and CSS load or link to
//! must answer.

#[path = "support/site.rs"]
mod site;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use site::Site;

const ORIGIN: &str = "http://example.edu";

fn png(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(1600, 900, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 120]))
        .save(path)
        .unwrap();
}

/// One of everything that writes a site-local URL: pages and folders, links
/// and wikilinks, a picture with responsive variants, the site's stylesheet,
/// a footer slot, a copied HTML page, a redirect and the 404 page.
fn site_at(url: &str) -> Site {
    let config = format!(
        "url = \"{url}\"\n\n[redirects]\n\"/old/\" = \"/about/\"\n\n[services.forms]\nendpoint = \"https://forms.example.com/submit\"\n"
    );
    let files = [
        ("index.md", "---\ntitle: Home\n---\n\nSee [About](/about/), [[First]] and the [notes](notes/).\n\n![A photo](images/photo.png)\n"),
        ("about.md", "---\ntitle: About\n---\n\nBack [home](/).\n"),
        ("notes/index.md", "---\ntitle: Notes\n---\n\nThe notes.\n"),
        ("notes/first.md", "---\ntitle: First\n---\n\nUp to [the notes](/notes/) and [about](../about/).\n"),
        ("footer.md", "[Home](/) · [About](/about/)\n"),
        ("ask.md", "---\ntitle: Ask\n---\n\n:::form {#ask next=/about/}\n- Your name *\n:::\n"),
        (".moss/theme/style.css", "body { background-image: url(\"/images/bg.png\"); }\n.hero { background: url(/images/photo.png) no-repeat; }\n"),
        (
            "raw/page.html",
            "<!doctype html><html><head><title>Raw</title><style>body { background: url(/images/bg.png); }</style></head>\
             <body><a href=\"/about/\">About</a><img src=\"/images/photo.png\" srcset=\"/images/photo.png 1x\"></body></html>",
        ),
    ];
    let site = Site::with_config(&config, &files);
    // Real pictures, so the rebuild publishes their responsive variants.
    png(&site.root.join("images/photo.png"));
    png(&site.root.join("images/bg.png"));
    site.rebuild();
    site
}

/// The generation a publish uploads, as a static file server under `prefix`
/// would answer for it.
struct Served {
    root: PathBuf,
    prefix: String,
}

impl Served {
    fn of(site: &Site, prefix: &str) -> Served {
        Served { root: site.root.join(".moss/build.nosync/current"), prefix: prefix.to_string() }
    }

    fn file(&self, path: &str) -> Option<PathBuf> {
        let rel = path.strip_prefix(&self.prefix)?;
        let rel = percent_encoding::percent_decode_str(rel).decode_utf8().ok()?;
        let mut full = self.root.join(rel.as_ref());
        if rel.is_empty() || rel.ends_with('/') || full.is_dir() {
            full = full.join("index.html");
        }
        full.is_file().then_some(full)
    }

    fn url_of(&self, file: &Path) -> String {
        let rel = file.strip_prefix(&self.root).unwrap().to_string_lossy().replace('\\', "/");
        format!("{}{}", self.prefix, rel)
    }

    fn files(&self, ext: &str) -> Vec<PathBuf> {
        walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == ext))
            .map(|e| e.into_path())
            .collect()
    }

    /// Every same-origin URL that does not answer, with the page that named it.
    fn crawl(&self) -> Vec<String> {
        let mut queue: Vec<(String, String)> = vec![(self.prefix.clone(), "(home)".into())];
        // Every HTML file is a way in too: copied pages, the 404 page and the
        // redirect stubs are reached by no link.
        queue.extend(self.files("html").iter().map(|f| (self.url_of(f), "(every page)".into())));
        let mut seen = BTreeSet::new();
        let mut missing = Vec::new();
        while let Some((path, from)) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let Some(file) = self.file(&path) else {
                missing.push(format!("{path} <- {from}"));
                continue;
            };
            let refs = match file.extension().and_then(|e| e.to_str()) {
                Some("html") => html_refs(&fs::read_to_string(&file).unwrap()),
                Some("css") => css_refs(&fs::read_to_string(&file).unwrap()),
                _ => continue,
            };
            let base = url::Url::parse(ORIGIN).unwrap().join(&path).unwrap();
            for r in refs {
                let Ok(u) = base.join(&r) else { continue };
                if u.scheme().starts_with("http") && u.host_str() == Some("example.edu") {
                    queue.push((u.path().to_string(), path.clone()));
                }
            }
        }
        missing
    }
}

/// A page's identity, not a URL to load: kept as the site wrote it.
const IDENTITY_ATTRS: [&str; 2] = ["data-path", "data-page-key"];

/// Every URL a page names: any attribute whose value is a URL path (so a new
/// URL attribute is crawled without being listed here), each `srcset`
/// candidate, a meta refresh target, and the `url()`s of inline styles.
fn html_refs(html: &str) -> Vec<String> {
    let doc = scraper::Html::parse_document(html);
    let mut out = Vec::new();
    let all = scraper::Selector::parse("*").unwrap();
    for el in doc.select(&all) {
        let e = el.value();
        for (name, value) in e.attrs() {
            let value = value.trim();
            match name {
                _ if IDENTITY_ATTRS.contains(&name) => {}
                "srcset" => out.extend(value.split(',').filter_map(|c| c.split_whitespace().next()).map(str::to_string)),
                "style" => out.extend(css_refs(value)),
                "content" if e.attr("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh")) => {
                    if let Some(at) = value.to_ascii_lowercase().find("url=") {
                        out.push(value[at + 4..].trim_matches(['\'', '"', ' ']).to_string());
                    }
                }
                "href" | "src" | "poster" | "action" | "data" => out.push(value.to_string()),
                _ if value.starts_with('/') && !value.starts_with("//") => out.push(value.to_string()),
                _ => {}
            }
        }
        if e.name() == "style" {
            out.extend(css_refs(&el.text().collect::<String>()));
        }
    }
    out.retain(|u| !u.is_empty() && !u.starts_with(['#']) && !u.starts_with("data:") && !u.starts_with("mailto:"));
    out
}

/// Under a mount, every root-relative URL a page names starts with it.
fn unmounted_urls(served: &Served) -> Vec<String> {
    let mut out = Vec::new();
    for file in served.files("html") {
        let html = fs::read_to_string(&file).unwrap();
        for u in html_refs(&html) {
            if u.starts_with('/') && !u.starts_with("//") && !u.starts_with(&served.prefix) {
                out.push(format!("{u} in {}", served.url_of(&file)));
            }
        }
    }
    out
}

fn css_refs(css: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"url\(\s*['"]?([^'")\s]+)|@import\s+['"]([^'"]+)"#).unwrap();
    re.captures_iter(css)
        .filter_map(|c| c.get(1).or(c.get(2)).map(|m| m.as_str().to_string()))
        .filter(|u| !u.starts_with("data:") && !u.starts_with('#'))
        .collect()
}

/// Every manifest entry for a file names the bytes the generation holds: a
/// publish uploads those bytes and checks each against its entry.
fn manifest_describes_the_generation(site: &Site) -> Vec<String> {
    let hashes: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(site.root.join(".moss/build.nosync/hashes.json")).unwrap()).unwrap();
    let files: HashMap<String, String> = serde_json::from_value(hashes["files"].clone()).unwrap();
    let current = site.root.join(".moss/build.nosync/current");
    files
        .iter()
        .filter_map(|(path, entry)| {
            let hash = entry.strip_prefix("100644:")?;
            let bytes = fs::read(current.join(path)).ok()?;
            let actual = format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&bytes));
            (actual != hash).then(|| format!("{path}: manifest {hash}, generation {actual}"))
        })
        .collect()
}

fn every_url_answers_under(url: &str, prefix: &str) -> Site {
    let site = site_at(url);
    let served = Served::of(&site, prefix);
    assert!(served.file(prefix).is_some(), "no home page at {prefix}");
    let missing = served.crawl();
    assert!(missing.is_empty(), "under {prefix} these do not answer:\n{}", missing.join("\n"));
    let wrong = manifest_describes_the_generation(&site);
    assert!(wrong.is_empty(), "the manifest does not describe what ships under {prefix}:\n{}", wrong.join("\n"));
    let unmounted = unmounted_urls(&served);
    assert!(unmounted.is_empty(), "under {prefix} these URLs are not under it:\n{}", unmounted.join("\n"));
    site
}

#[test]
fn every_url_answers_at_the_root() {
    every_url_answers_under("https://example.edu/", "/");
}

#[test]
fn every_url_answers_under_a_user_path() {
    let site = every_url_answers_under("https://example.edu/~alice/", "/~alice/");
    let home = fs::read_to_string(site.root.join(".moss/build.nosync/current/index.html")).unwrap();
    assert!(home.contains(r#"<meta name="moss:base" content="/~alice/">"#), "{home}");
    // Absolute addresses already carry the path, once.
    assert!(home.contains("https://example.edu/~alice/about/") || home.contains(r#"href="/~alice/about/""#));
    assert!(!home.contains("/~alice/~alice/"));
}

#[test]
fn every_url_answers_under_a_nested_path() {
    every_url_answers_under("https://example.edu/a/b/", "/a/b/");
}

/// The preview's stage never carries the mount, so a rebuild that re-reads a
/// staged page cannot prefix it twice.
#[test]
fn a_rebuild_never_prefixes_twice() {
    let site = site_at("https://example.edu/~alice/");
    site.rebuild();
    let served = Served::of(&site, "/~alice/");
    let missing = served.crawl();
    assert!(missing.is_empty(), "after a rebuild these do not answer:\n{}", missing.join("\n"));
    // A rebuild re-registers unchanged redirect stubs without writing them.
    let wrong = manifest_describes_the_generation(&site);
    assert!(wrong.is_empty(), "after a rebuild the manifest does not describe what ships:\n{}", wrong.join("\n"));
    for file in served.files("html").into_iter().chain(served.files("css")) {
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("/~alice/~alice/"), "{} is prefixed twice", file.display());
    }
    let staged = fs::read_to_string(site.output("index.html")).unwrap();
    // Absolute addresses carry the path in the stage too; no root-relative URL does.
    assert!(!staged.contains("=\"/~alice/") && !staged.contains("moss:base"), "the stage carries the mount");
}

/// At the root nothing is added and no URL moves: every page and stylesheet
/// ships with exactly the URLs its staged copy has.
#[test]
fn a_site_at_the_root_ships_its_urls_unchanged() {
    let site = site_at("https://example.edu/");
    let served = Served::of(&site, "/");
    let pages = served.files("html");
    assert!(pages.len() > 5);
    for file in pages.into_iter().chain(served.files("css")) {
        let shipped = fs::read_to_string(&file).unwrap();
        assert!(!shipped.contains("moss:base"), "{} gained a base meta", file.display());
        let rel = file.strip_prefix(&served.root).unwrap();
        let Ok(staged) = fs::read_to_string(site.output(&rel.to_string_lossy())) else { continue };
        let refs = |text: &str, css: bool| -> Vec<String> { if css { css_refs(text) } else { html_refs(text) } };
        let css = file.extension().is_some_and(|e| e == "css");
        assert_eq!(refs(&shipped, css), refs(&staged, css), "{} ships different URLs", rel.display());
        if css {
            assert_eq!(shipped, staged, "{} changed", rel.display());
        }
    }
}
