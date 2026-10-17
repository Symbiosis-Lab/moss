//! A page that names no `cover:` is listed with its own first picture.

use std::fs;
use std::path::{Path, PathBuf};

fn build(folder: &Path, trigger: moss_build::build::BuildTrigger) -> Result<String, String> {
    use moss_build::build::{run_pipeline, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder.to_string_lossy()),
        trigger,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn write_picture(root: &Path, rel: &str, shade: u8) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([shade, (x % 200) as u8, (y % 200) as u8]))
        .save(path)
        .unwrap();
}

struct Site(PathBuf);
impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn page(title: &str, front: &str, body: &str) -> String {
    format!("---\ntitle: {title}\n{front}---\n\n{body}\n")
}

/// Two folders with the same pages: `Harbor Notes` lists with the default
/// style, `Quay Notes` with `children_style: grid`.
fn site() -> Site {
    let root = std::env::temp_dir().join(format!("moss_first_picture_{}", uuid::Uuid::new_v4()));
    write(&root, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&root, "index.md", &page("Harbor", "", "Welcome."));
    for (folder, style) in [("Harbor Notes", ""), ("Quay Notes", "children_style: grid\n")] {
        write(&root, &format!("{folder}/index.md"), &page(folder, style, "Notes."));
        let pages: [(&str, &str, &str); 13] = [
            ("lighthouse", "", "![Lighthouse at dusk](Photos/Light%20house.jpg)\n\nA tall light."),
            ("declared", "cover: Photos/declared.jpg\n", "![Body picture](Photos/body.jpg)\n\nWritten cover wins."),
            ("quiet", "cover: false\n", "![Body picture](Photos/quiet.jpg)\n\nNo picture for this one."),
            ("linked", "", "[![Thumb](Photos/linked.jpg)](lighthouse.md)\n\nA picture that belongs to another page."),
            ("icon", "", "Press ![gear](Photos/gear.png) to save."),
            ("plain", "", "Words only."),
            ("album", "", ":::gallery\n![One](Photos/album-one.jpg)\n![Two](Photos/album-two.jpg)\n:::\n"),
            ("relative", "", "![Dock](../Shared/dock.jpg)\n\nA shared picture."),
            ("remote", "", "![Far](https://example.com/far.jpg)\n\nA picture on another site."),
            ("inline", "", "![Dot](data:image/png;base64,iVBORw0KGgo=)\n\nA picture inside the page."),
            ("drawing", "", "![Plan](Photos/plan.svg)\n\nA vector drawing."),
            ("contents", "", ":::grid 2\n[[lighthouse]]\n+++\n[[plain]]\n:::\n"),
            ("opening", "", ":::hero {image=Photos/hero.jpg}\n:::\n\n![Later](Photos/later.jpg)\n"),
        ];
        for (stem, front, body) in pages {
            write(&root, &format!("{folder}/{stem}.md"), &page(stem, front, body));
        }
        for (i, name) in ["Light house", "declared", "body", "quiet", "linked", "gear", "album-one", "album-two", "hero", "later"].iter().enumerate() {
            write_picture(&root, &format!("{folder}/Photos/{name}.{}", if *name == "gear" { "png" } else { "jpg" }), 20 + i as u8 * 20);
        }
        write(&root, &format!("{folder}/Photos/plan.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#);
    }
    write_picture(&root, "Shared/dock.jpg", 200);
    write(&root, "Studio/index.md", &page("Studio", "", "![Studio floor](pics/floor.jpg)\n\nWhere it happens."));
    write_picture(&root, "Studio/pics/floor.jpg", 90);
    Site(root)
}

/// Each listed page's card picture, by the page's last URL segment.
fn card_pictures(root: &Path, folder_slug: &str) -> Vec<(String, Option<String>)> {
    let html = fs::read_to_string(root.join(format!(".moss/build.nosync/staging/{folder_slug}/index.html"))).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    let img = scraper::Selector::parse(".moss-card-cover img").unwrap();
    doc.select(&card)
        .map(|c| {
            let href = c.value().attr("href").unwrap_or("").trim_end_matches('/');
            let slug = href.rsplit('/').next().unwrap_or("").to_string();
            let src = c.select(&img).next().and_then(|i| i.value().attr("src")).map(str::to_string);
            (slug, src)
        })
        .collect()
}

fn picture_of(cards: &[(String, Option<String>)], slug: &str) -> Option<String> {
    cards.iter().find(|(s, _)| s == slug).unwrap_or_else(|| panic!("no card for {slug}: {cards:?}")).1.clone()
}

/// The problem counter is process-wide, so builds run one at a time.
static ONE_BUILD_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Built(Site, #[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
impl Built {
    fn root(&self) -> &Path {
        &(self.0).0
    }
}

fn built() -> Built {
    let guard = ONE_BUILD_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let _ = moss_build::build::cli_output::take_cli_problems();
    let s = site();
    let r = build(&s.0, moss_build::build::BuildTrigger::Full);
    assert!(r.is_ok(), "{r:?}");
    Built(s, guard)
}


const FOLDERS: [(&str, &str); 2] = [("harbor-notes", "folder listing"), ("quay-notes", "grid listing")];

#[test]
fn a_page_with_no_cover_is_listed_with_its_first_picture() {
    let s = built();
    for (slug, what) in FOLDERS {
        let cards = card_pictures(s.root(), slug);
        let in_folder = |file: &str| Some(format!("/{slug}/photos/{file}"));
        assert_eq!(picture_of(&cards, "lighthouse"), in_folder("Light%20house.jpg"), "{what}: a figure");
        assert_eq!(picture_of(&cards, "album"), in_folder("album-one.jpg"), "{what}: a gallery's first picture");
        assert_eq!(picture_of(&cards, "relative"), Some("/shared/dock.jpg".into()), "{what}: a ../ path");
        assert_eq!(picture_of(&cards, "opening"), in_folder("hero.jpg"), "{what}: the opening hero's picture");
    }
}

#[test]
fn the_card_names_the_file_the_page_body_shows() {
    let s = built();
    let page = fs::read_to_string(s.root().join(".moss/build.nosync/staging/harbor-notes/lighthouse/index.html")).unwrap();
    let doc = scraper::Html::parse_document(&page);
    let body_src = doc
        .select(&scraper::Selector::parse("article img").unwrap())
        .next()
        .and_then(|i| i.value().attr("src"))
        .map(str::to_string);
    let card = picture_of(&card_pictures(s.root(), "harbor-notes"), "lighthouse");
    assert!(body_src.is_some() && card == body_src, "card {card:?} vs body {body_src:?}");
    let served = s.root().join(".moss/build.nosync/staging").join(urlencoding::decode(card.unwrap().trim_start_matches('/')).unwrap().as_ref());
    assert!(served.exists(), "{} is not in the output", served.display());
}

#[test]
fn a_written_cover_wins() {
    let s = built();
    for (slug, what) in FOLDERS {
        let cards = card_pictures(s.root(), slug);
        assert_eq!(picture_of(&cards, "declared"), Some(format!("/{slug}/photos/declared.jpg")), "{what}");
    }
}

#[test]
fn cover_false_lists_the_page_without_a_picture() {
    let s = built();
    for (slug, what) in FOLDERS {
        assert_eq!(picture_of(&card_pictures(s.root(), slug), "quiet"), None, "{what}");
    }
    assert_eq!(moss_build::build::cli_output::take_cli_problems(), 0, "`cover: false` must not be a warning");
}

#[test]
fn a_linked_picture_or_an_inline_image_is_not_the_pages_picture() {
    let s = built();
    for (slug, what) in FOLDERS {
        let cards = card_pictures(s.root(), slug);
        assert_eq!(picture_of(&cards, "linked"), None, "{what}: a picture inside a link to another page");
        assert_eq!(picture_of(&cards, "icon"), None, "{what}: an image set in a sentence");
    }
}

#[test]
fn a_remote_inline_or_vector_picture_is_not_taken() {
    let s = built();
    for (slug, what) in FOLDERS {
        let cards = card_pictures(s.root(), slug);
        assert_eq!(picture_of(&cards, "remote"), None, "{what}: a remote address");
        assert_eq!(picture_of(&cards, "inline"), None, "{what}: a data: address");
        assert_eq!(picture_of(&cards, "drawing"), None, "{what}: a vector");
    }
}

#[test]
fn a_grid_card_shows_the_pages_first_picture_or_none() {
    let s = built();
    let html = fs::read_to_string(s.root().join(".moss/build.nosync/staging/harbor-notes/contents/index.html")).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let img = scraper::Selector::parse("img").unwrap();
    let cards: Vec<(String, bool)> = doc
        .select(&scraper::Selector::parse("a.moss-card").unwrap())
        .map(|c| (c.value().attr("href").unwrap_or("").to_string(), c.select(&img).next().is_some()))
        .collect();
    let has_img = |slug: &str| cards.iter().find(|(h, _)| h.contains(slug)).unwrap_or_else(|| panic!("{slug}: {cards:?}")).1;
    assert!(has_img("lighthouse"), "{cards:?}");
    assert!(!has_img("plain"), "{cards:?}");
}

fn share_image(root: &Path, page: &str) -> Option<String> {
    let html = fs::read_to_string(root.join(format!(".moss/build.nosync/staging/harbor-notes/{page}/index.html"))).unwrap();
    let doc = scraper::Html::parse_document(&html);
    doc.select(&scraper::Selector::parse(r#"meta[property="og:image"]"#).unwrap())
        .next()
        .and_then(|m| m.value().attr("content"))
        .map(str::to_string)
}

#[test]
fn the_share_image_follows_the_card_rule() {
    let s = built();
    let shares = |page: &str, file: &str| share_image(s.root(), page).is_some_and(|u| u.contains(file));
    assert!(shares("album", "album-one.jpg"), "a gallery's first picture is shared: {:?}", share_image(s.root(), "album"));
    assert!(shares("lighthouse", "Light%20house.jpg") || shares("lighthouse", "Light house.jpg"));
    assert!(!shares("drawing", "plan.svg"), "a vector is not shared: {:?}", share_image(s.root(), "drawing"));
    assert!(!shares("linked", "linked.jpg"), "a picture inside a link is not shared");
}

#[test]
fn a_page_with_no_picture_is_listed_as_before() {
    let s = built();
    for (slug, what) in FOLDERS {
        assert_eq!(picture_of(&card_pictures(s.root(), slug), "plain"), None, "{what}");
    }
}

#[test]
fn a_folder_page_does_not_draw_its_own_first_picture_twice() {
    let s = built();
    let html = fs::read_to_string(s.root().join(".moss/build.nosync/staging/studio/index.html")).unwrap();
    assert_eq!(html.matches(r#"src="/studio/pics/floor.jpg""#).count(), 1, "{html}");
    assert!(!html.contains("moss-collection-cover"), "an inferred picture must not open the cover row");
}

#[test]
fn changing_a_pages_first_picture_re_renders_its_listing() {
    let s = built();
    let before = picture_of(&card_pictures(s.root(), "harbor-notes"), "lighthouse");
    assert_eq!(before, Some("/harbor-notes/photos/Light%20house.jpg".into()));

    let changed = s.root().join("Harbor Notes/lighthouse.md");
    fs::write(&changed, page("lighthouse", "", "![Lighthouse at dawn](Photos/body.jpg)\n\nA tall light.")).unwrap();
    let r = build(s.root(), moss_build::build::BuildTrigger::ContentOnly(vec![changed]));
    assert!(r.is_ok(), "{r:?}");

    let after = picture_of(&card_pictures(s.root(), "harbor-notes"), "lighthouse");
    assert_eq!(after, Some("/harbor-notes/photos/body.jpg".into()));
}
