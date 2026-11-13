//! The font copy against a mock font host.

use super::*;
use crate::vault::import::scrape::fetch::test_server;

fn face(family: &str, source: &str) -> FontFace {
    FontFace { family: family.to_string(), descriptors: Vec::new(), source: source.to_string(), format: Some("woff2".to_string()) }
}

const FONT: &[u8] = b"wOF2 sixty-four bytes of a font file, padded out to the length....";

#[tokio::test]
async fn a_family_that_would_pass_the_byte_limit_is_not_copied_and_says_so() {
    let mut server = test_server().await;
    let _a = server.mock("GET", "/a.woff2").with_body(FONT).create_async().await;
    let _b = server.mock("GET", "/b.woff2").with_body(FONT).create_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let faces = [face("Field Sans", &format!("{}/a.woff2", server.url())), face("Field Serif", &format!("{}/b.woff2", server.url()))];
    let families = ["Field Sans".to_string(), "Field Serif".to_string()];
    let mut notes = Vec::new();

    let (css, copied) = copy_fonts(&faces, &families, tmp.path(), "test", &mut HostPacer::default(), FONT.len() + 10, &mut notes).await;

    assert_eq!(copied, vec!["Field Sans".to_string()]);
    assert!(css.contains("url(fonts/a.woff2)") && !css.contains("b.woff2"), "{css}");
    assert!(!tmp.path().join("fonts/b.woff2").exists());
    assert_eq!(notes, vec!["font Field Serif (0 KB) would pass the 0 MB of fonts one import copies; not copied".to_string()]);
}

#[tokio::test]
async fn a_font_file_the_folder_already_has_is_used_and_never_overwritten() {
    let mut server = test_server().await;
    let fetch = server.mock("GET", "/a.woff2").with_body(FONT).expect(0).create_async().await;
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("fonts")).unwrap();
    std::fs::write(tmp.path().join("fonts/a.woff2"), b"mine").unwrap();
    let faces = [face("Field Sans", &format!("{}/a.woff2", server.url()))];

    let (css, copied) = copy_fonts(&faces, &["Field Sans".to_string()], tmp.path(), "test", &mut HostPacer::default(), MAX_FONT_BYTES, &mut Vec::new()).await;

    assert_eq!(std::fs::read(tmp.path().join("fonts/a.woff2")).unwrap(), b"mine");
    assert!(css.contains("url(fonts/a.woff2)") && copied == ["Field Sans"], "{css}");
    fetch.assert_async().await;
}

#[tokio::test]
async fn a_font_on_a_local_or_private_address_is_not_requested() {
    let tmp = tempfile::tempdir().unwrap();
    // A port of this machine no test registered.
    let faces = [face("Field Sans", "http://127.0.0.1:9/a.woff2"), face("Field Sans", "http://169.254.169.254/b.woff2"), face("Field Sans", "http://[fe80::1]/c.woff2")];
    let mut notes = Vec::new();

    let (css, copied) = copy_fonts(&faces, &["Field Sans".to_string()], tmp.path(), "test", &mut HostPacer::default(), MAX_FONT_BYTES, &mut notes).await;

    assert!(css.is_empty() && copied.is_empty());
    let refused: Vec<String> = faces.iter().map(|f| format!("font Field Sans at {}: address not allowed", f.source)).collect();
    assert_eq!(notes, refused);
}

#[tokio::test]
async fn a_font_that_fails_or_is_no_font_is_reported() {
    let mut server = test_server().await;
    let _gone = server.mock("GET", "/gone.woff2").with_status(404).create_async().await;
    let _page = server.mock("GET", "/page.woff2").with_body("<html>not found</html>").create_async().await;
    let tmp = tempfile::tempdir().unwrap();
    let faces = [face("Field Sans", &format!("{}/gone.woff2", server.url())), face("Field Sans", &format!("{}/page.woff2", server.url()))];
    let mut notes = Vec::new();

    copy_fonts(&faces, &["Field Sans".to_string()], tmp.path(), "test", &mut HostPacer::default(), MAX_FONT_BYTES, &mut notes).await;

    assert!(notes[0].starts_with("font Field Sans at") && notes[0].contains("could not be fetched (404 "), "{notes:?}");
    assert!(notes[1].contains("is not a font file"), "{notes:?}");
}
