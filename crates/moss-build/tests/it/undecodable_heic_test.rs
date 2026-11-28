//! A HEIC the build cannot read still has a webp variant named when its page
//! renders; none is ever made, so what a reader is sent names the original.

use crate::site::Site;
use std::fs;

const ORIGINAL: &str = r#"src="/notes/IMG_1.HEIC""#;

/// A dated article embedding `notes/IMG_1.HEIC` holding `bytes`.
fn article_with_heic(bytes: &[u8]) -> Site {
    let site = Site::in_language("en", "url = \"https://example.org\"\n", &[("index.md", "# Home\n")]);
    fs::create_dir_all(site.root.join("notes")).unwrap();
    fs::write(site.root.join("notes/IMG_1.HEIC"), bytes).unwrap();
    site.write("notes/spring.md", "---\ntitle: Spring\ndate: 2026-03-01\n---\n\n![](IMG_1.HEIC)\n");
    site.rebuild();
    site
}

/// Every `<img>` tag of `html`, so a failure reads without the chrome.
fn imgs(html: &str) -> Vec<&str> {
    html.split("<img").skip(1).map(|tag| tag.split('>').next().unwrap()).filter(|tag| !tag.contains("lightbox")).collect()
}

fn assert_names_only_the_original(html: &str, original: &str) {
    let imgs = imgs(html);
    assert!(imgs.iter().any(|tag| tag.contains(original)), "no <img> names {original}: {imgs:#?}");
    assert!(!imgs.iter().any(|tag| tag.contains("IMG_1.HEIC.")), "an <img> names a variant never made: {imgs:#?}");
}

// Only ImageIO hands the encoder a HEIC it then fails to decode; elsewhere it is skipped before render.
#[cfg(target_os = "macos")]
#[test]
fn a_heic_that_fails_to_decode_is_shown_as_its_original() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/images/heic/tiled-1100x700.heic");
    let mut bytes = fs::read(path).unwrap();
    // The primary item becomes a kind ImageIO does not know; its size still reads.
    let grid = bytes.windows(4).position(|w| w == b"grid").expect("a grid item");
    bytes[grid..grid + 4].copy_from_slice(b"xxxx");
    let site = article_with_heic(&bytes);

    assert_names_only_the_original(&site.html("notes/spring/index.html"), ORIGINAL);
}

#[test]
fn a_heic_that_is_not_a_picture_is_shown_and_fed_as_its_original() {
    let site = article_with_heic(&[7u8; 4000]);

    assert_names_only_the_original(&site.html("notes/spring/index.html"), ORIGINAL);
    assert_names_only_the_original(&site.html("rss.xml"), r#"src="https://example.org/notes/IMG_1.HEIC""#);
}

#[test]
fn a_heic_cover_never_made_leaves_no_share_picture_naming_its_webp() {
    let site = Site::in_language("en", "url = \"https://example.org\"\n", &[("index.md", "# Home\n")]);
    fs::create_dir_all(site.root.join("notes")).unwrap();
    fs::write(site.root.join("notes/IMG_1.HEIC"), [7u8; 4000]).unwrap();
    site.write("notes/spring.md", "---\ntitle: Spring\ndate: 2026-03-01\ncover: IMG_1.HEIC\n---\n\nWords.\n");
    site.rebuild();

    let html = site.html("notes/spring/index.html");
    let share: Vec<&str> = html.split('<').filter(|tag| tag.contains("og:image") || tag.contains("twitter:image") || tag.contains("data-share-cover")).collect();
    assert!(!share.iter().any(|tag| tag.contains("IMG_1.HEIC.webp")), "a share tag names a variant never made: {share:#?}");
    assert!(!share.iter().any(|tag| tag.contains("og:image:alt") || tag.contains("twitter:image:alt")), "an image's alt outlived it: {share:#?}");
}
