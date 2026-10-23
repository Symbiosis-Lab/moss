use super::*;

/// A 1×1 transparent PNG, base64.
const PNG_1X1_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// End-to-end local import: an MHTML douban-note archive becomes a markdown
/// note whose body is the note (not douban chrome), whose embedded image is
/// written to disk and referenced locally, and whose frontmatter carries an
/// `origin` link to the archived page — no network access.
#[tokio::test]
async fn import_local_mhtml_writes_origin_note_with_embedded_asset() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    let mhtml = format!(
        "From: <Saved by Blink>\r\n\
Snapshot-Content-Location: https://www.douban.com/note/1/?_i=track\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
Content-Location: https://www.douban.com/note/1/?_i=track\r\n\
\r\n\
<html><head><title>=E4=B9=8C=E7=BE=BD=E7=8E=89</title></head><body>\r\n\
<div id=3D\"db-nav\"><ul class=3D\"nav\"><li><a href=3D\"https://movie.douban.com\">movie-nav</a></li></ul></div>\r\n\
<div id=3D\"link-report\"><div class=3D\"note\">\r\n\
<p>peyote body text.</p>\r\n\
<img src=3D\"https://img.douban.com/a.png\">\r\n\
</div></div></body></html>\r\n\
--B\r\n\
Content-Type: image/png\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: https://img.douban.com/a.png\r\n\
\r\n\
{PNG_1X1_B64}\r\n\
--B--\r\n"
    );
    let mhtml_path = out.join("saved.mhtml");
    std::fs::write(&mhtml_path, mhtml).unwrap();

    let res = import_local_file(&mhtml_path, out).await.unwrap();
    assert_eq!(res.total_pages, 1, "one note imported");
    assert_eq!(res.failed_pages, 0);

    // Locate the written markdown (ignore the source .mhtml).
    let md_path = std::fs::read_dir(out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .expect("a .md note should be written");
    let content = std::fs::read_to_string(&md_path).unwrap();

    // origin points at the archived page, with the tracking query stripped.
    assert!(
        content.contains("origin: \"https://www.douban.com/note/1/\"\n"),
        "frontmatter should record the clean source URL as origin; got:\n{content}"
    );
    assert!(!content.contains("external_url"), "got:\n{content}");
    // Body is the note, not douban nav chrome.
    assert!(content.contains("peyote body text"), "got:\n{content}");
    assert!(!content.contains("movie-nav"), "nav leaked; got:\n{content}");
    // Embedded image written to disk and referenced locally.
    assert!(
        content.contains("./assets/imported/"),
        "image should be rewritten to a local path; got:\n{content}"
    );
    let asset_count = std::fs::read_dir(out.join(ASSETS_SUBDIR))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(asset_count, 1, "exactly one embedded asset should be written");
}

/// A saved WeChat article: the body container is shipped hidden and its
/// images carry only `data-src`; the body and the embedded image must land,
/// the account card must not.
#[tokio::test]
async fn import_local_mhtml_wechat_article_keeps_body_and_image() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    let mhtml = format!(
        "From: <Saved by Blink>\r\n\
Snapshot-Content-Location: https://mp.weixin.qq.com/s/invented\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
Content-Location: https://mp.weixin.qq.com/s/invented\r\n\
\r\n\
<html><head><title>Invented Post</title></head><body>\r\n\
<script>var ct =3D \"1757394245\"; var nickname =3D htmlDecode(\"Invented Lanterns\");\r\n\
var author =3D \"\";</script>\r\n\
<div id=3D\"js_content\" style=3D\"visibility: hidden; opacity: 0;\">\r\n\
<mp-common-profile><div>Invented Account Card</div></mp-common-profile>\r\n\
<p>Lantern harvest begins at dusk.</p>\r\n\
<p><img data-src=3D\"https://mmbiz.qpic.cn/invented/640?wx_fmt=3Djpeg\"></p>\r\n\
</div></body></html>\r\n\
--B\r\n\
Content-Type: image/png\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: https://mmbiz.qpic.cn/invented/640?wx_fmt=jpeg\r\n\
\r\n\
{PNG_1X1_B64}\r\n\
--B--\r\n"
    );
    let mhtml_path = out.join("saved.mhtml");
    std::fs::write(&mhtml_path, mhtml).unwrap();

    let res = import_local_file(&mhtml_path, out).await.unwrap();
    assert_eq!(res.total_pages, 1);
    let md_path = std::fs::read_dir(out)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
        .expect("a .md note should be written");
    let content = std::fs::read_to_string(&md_path).unwrap();
    assert!(content.contains("Lantern harvest begins at dusk."), "got:\n{content}");
    assert!(!content.contains("Invented Account Card"), "got:\n{content}");
    assert!(content.contains("date: \"2025-09-09T13:04:05+08:00\""), "got:\n{content}");
    assert!(content.contains("publisher: \"Invented Lanterns\""), "got:\n{content}");
    assert!(!content.contains("author:"), "got:\n{content}");
    assert!(content.contains("./assets/imported/"), "got:\n{content}");
    let asset_count = std::fs::read_dir(out.join(ASSETS_SUBDIR)).map(|d| d.count()).unwrap_or(0);
    assert_eq!(asset_count, 1, "the embedded image should be written");
}

#[tokio::test]
async fn import_local_plain_html_no_source_omits_origin() {
    // The plain-.html arm: no source URL, so no `origin`; body extracted.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    let html = "<html><head><title>My Essay</title></head><body><article>\
        <p>First paragraph with enough words to be extracted as the content.</p>\
        <p>A second paragraph, also with a few words, to satisfy scoring.</p>\
        </article></body></html>";
    let p = out.join("essay.html");
    std::fs::write(&p, html).unwrap();

    let res = import_local_file(&p, out).await.unwrap();
    assert_eq!(res.total_pages, 1);
    let md = out.join("My Essay.md");
    assert!(md.exists(), "note named from <title> should exist");
    let content = std::fs::read_to_string(&md).unwrap();
    assert!(content.contains("First paragraph"), "body missing: {content}");
    assert!(!content.contains("origin"), "no source → no origin: {content}");
    assert!(!content.contains("syndicated"), "{content}");
    assert!(!content.contains("external_url"), "{content}");
}

#[tokio::test]
async fn import_local_empty_body_errors_instead_of_empty_note() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path();
    let p = out.join("empty.html");
    std::fs::write(&p, "<html><head><title>Nothing</title></head><body></body></html>").unwrap();
    let res = import_local_file(&p, out).await;
    assert!(res.is_err(), "empty extraction should error, not write an empty note");
    assert!(
        std::fs::read_dir(out).unwrap().filter_map(|e| e.ok())
            .all(|e| e.path().extension().and_then(|x| x.to_str()) != Some("md")),
        "no .md should be written on empty extraction"
    );
}
