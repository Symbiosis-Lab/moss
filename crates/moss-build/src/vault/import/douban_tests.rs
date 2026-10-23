//! Fixtures are invented reductions of the backup shape: an invented user id,
//! invented titles and text, and reserved `.example` addresses.

use super::*;
use serde_json::json;
use std::io::Write;
use zip::write::SimpleFileOptions;

fn backup(dir: &Path, user_tables: Value) -> std::path::PathBuf {
    let path = dir.join("backup.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = SimpleFileOptions::default();
    let files = [
        (
            "database.json",
            json!({"global": {"version": 2}, "local": {"version": 5, "files": ["tofu[1234]"]}}),
        ),
        ("tofu.json", json!({"account": [], "job": []})),
        ("tofu[1234].json", user_tables),
    ];
    for (name, value) in files {
        z.start_file(name, opts).unwrap();
        z.write_all(value.to_string().as_bytes()).unwrap();
    }
    z.finish().unwrap();
    path
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

async fn import(tables: Value) -> (tempfile::TempDir, TofuReport) {
    let tmp = tempfile::tempdir().unwrap();
    let zip = backup(tmp.path(), tables);
    let out = tmp.path().join("site");
    let (_, report) = import_tofu_backup(&zip, &out).await.unwrap();
    (tmp, report)
}

#[test]
fn only_a_backup_naming_its_user_files_is_claimed() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(is_tofu_backup(&backup(tmp.path(), json!({}))));

    let other = tmp.path().join("other.zip");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    z.start_file("database.json", SimpleFileOptions::default())
        .unwrap();
    z.write_all(br#"{"unrelated": true}"#).unwrap();
    z.start_file("index.html", SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"<p>x</p>").unwrap();
    z.finish().unwrap();
    assert!(!is_tofu_backup(&other));

    let plain = tmp.path().join("plain.zip");
    std::fs::write(&plain, b"not a zip").unwrap();
    assert!(!is_tofu_backup(&plain));
}

#[tokio::test]
async fn a_note_becomes_a_page_without_source_ids() {
    let (tmp, report) = import(json!({"note": [{
        "id": 4001, "version": 3,
        "note": {"title": "Garden log", "url": "https://notes.example/note/4001/",
                 "create_time": "2024-03-05 21:10:00",
                 "fulltext": "<p>First <b>bed</b> planted.</p>", "abstract": "First bed"}
    }]}))
    .await;
    let page = read(&tmp.path().join("site"), "note/4001.md");
    assert!(page.contains("title: \"Garden log\"\n"), "{page}");
    assert!(page.contains("date: \"2024-03-05T21:10:00+08:00\"\n"));
    assert!(page.contains("origin: \"https://notes.example/note/4001/\"\n"));
    assert!(page.contains("First **bed** planted."));
    assert!(
        !page.contains("version") && !page.contains("4001\"") && !page.contains("id:"),
        "{page}"
    );
    assert_eq!(report.partial, 0);

    let index = read(&tmp.path().join("site"), "note/index.md");
    assert!(index.contains("children_group: \"year\""), "{index}");
}

#[tokio::test]
async fn a_review_with_only_an_abstract_is_counted_partial() {
    let (tmp, report) = import(json!({"review": [{
        "id": 77, "type": "movie", "version": 1,
        "review": {"title": "Slow harbour", "url": "https://movie.example/review/77/",
                   "create_time": "2023-11-02 08:00:00", "rating": {"value": 4, "max": 5},
                   "subject": {"url": "https://movie.example/subject/9/", "title": "Harbour"},
                   "abstract": "A quiet start..."}
    }]}))
    .await;
    let page = read(&tmp.path().join("site"), "review/77.md");
    assert!(
        page.contains("review_of: \"https://movie.example/subject/9/\"\n"),
        "{page}"
    );
    assert!(page.contains("rating: 4\n"));
    assert!(page.ends_with("A quiet start...\n"));
    assert_eq!(report.partial, 1);
    assert!(report.lines().iter().any(|l| l.contains("partial")));
}

#[tokio::test]
async fn a_mark_lands_under_its_subject_type_with_its_comment() {
    let (tmp, _) = import(json!({"interest": [
        {"id": 1, "subject": 555, "type": "book", "status": "done", "version": 2,
         "interest": {"comment": "Short and sharp.", "create_time": "2022-06-01 10:00:00",
                      "rating": {"value": 5, "max": 5}, "tags": ["essay", "reread"],
                      "subject": {"id": "555", "title": "A Small Book", "type": "book",
                                  "url": "https://book.example/subject/555/"}}},
        {"id": 2, "subject": 556, "type": "tv", "status": "mark", "version": 2,
         "interest": {"comment": "", "create_time": "2022-06-02 10:00:00", "rating": null, "tags": [],
                      "subject": {"id": "556", "title": "Untitled Serial", "type": "tv",
                                  "url": "https://tv.example/subject/556/"}}}
    ]})).await;
    let site = tmp.path().join("site");
    let page = read(&site, "book/555.md");
    assert!(page.starts_with("---\ntitle: \"A Small Book\"\ndate: \"2022-06-01T10:00:00+08:00\"\nreview_of: \"https://book.example/subject/555/\"\nrating: 5\ntags: [\"读过\", \"essay\", \"reread\"]\n---\n"), "{page}");
    assert!(!page.contains("origin"));
    assert!(page.ends_with("Short and sharp.\n"));
    let unrated = read(&site, "tv/556.md");
    assert!(
        !unrated.contains("rating") && unrated.contains("tags: [\"想看\"]"),
        "{unrated}"
    );
    assert!(site.join("book/index.md").exists() && site.join("tv/index.md").exists());
}

#[tokio::test]
async fn a_status_links_its_entities_and_quotes_a_reshare() {
    let (tmp, _) = import(json!({"status": [{
        "id": 1, "type": "status",
        "status": {
            "id": "9001", "create_time": "2024-01-09 07:30:00",
            "url": "https://social.example/people/u1/status/9001/?from=feed",
            "sharing_url": "https://social.example/doubanapp/dispatch?uri=x&_i=abc",
            "activity": "recommended",
            "text": "Reading the Small Book today",
            "entities": [{"start": 12, "end": 22, "title": "Small Book", "url": "https://book.example/subject/555/?_spm_id=a1&keep=1"}],
            "images": [],
            "card": {"title": "A linked page", "url": "https://card.example/page?_i=zz"},
            "reshared_status": {"text": "Original words", "sharing_url": "https://social.example/status/1/",
                                "author": {"name": "Some Author", "url": "https://social.example/people/someone/"}}
        }
    }]})).await;
    let page = read(&tmp.path().join("site"), "status/9001.md");
    assert!(page.contains("Reading the [Small Book](https://book.example/subject/555/?keep=1) today"), "{page}");
    assert!(page.contains("> [Some Author](https://social.example/people/someone/): Original words"), "{page}");
    assert!(page.contains("> [https://social.example/status/1/](https://social.example/status/1/)"));
    assert!(page.contains("recommended [A linked page](https://card.example/page)"), "{page}");
    // No title and nothing inferred; the permanent address, not the dispatch link.
    assert!(page.starts_with("---\ndate: \"2024-01-09T07:30:00+08:00\"\norigin: \"https://social.example/people/u1/status/9001/\"\n---\n"), "{page}");
}

#[tokio::test]
async fn a_status_with_only_a_verb_and_a_card_still_gets_a_page() {
    let (tmp, _) = import(json!({"status": [{"id": 1, "status": {
        "id": "9002", "text": "", "activity": "wants to read",
        "card": {"title": "Some Book", "url": "https://book.example/subject/1/"}
    }}]})).await;
    let page = read(&tmp.path().join("site"), "status/9002.md");
    assert!(page.contains("wants to read [Some Book](https://book.example/subject/1/)"), "{page}");
}

#[tokio::test]
async fn a_mark_is_tagged_with_its_progress_in_the_subject_types_words() {
    let row = |id: u32, kind: &str, st: &str| json!({"id": id, "subject": id, "type": kind, "status": st,
        "interest": {"comment": "", "create_time": "2022-01-01 00:00:00", "tags": ["mine"],
                     "subject": {"id": id.to_string(), "title": "T", "type": kind, "url": "https://x.example/s/"}}});
    let (tmp, _) = import(json!({"interest": [
        row(1, "book", "doing"), row(2, "music", "mark"), row(3, "game", "done"), row(4, "drama", "done"), row(5, "odd", "mark")
    ]})).await;
    let site = tmp.path().join("site");
    for (rel, tag) in [("book/1.md", "在读"), ("music/2.md", "想听"), ("game/3.md", "玩过"), ("drama/4.md", "看过"), ("odd/5.md", "想看")] {
        assert!(read(&site, rel).contains(&format!("tags: [\"{tag}\", \"mine\"]")), "{rel}");
    }
}

#[tokio::test]
async fn an_unknown_table_is_counted_and_ignored() {
    let (tmp, report) = import(json!({
        "follower": [{"id": 1}, {"id": 2}],
        "note": [{"id": 5, "note": {"title": "T", "fulltext": "<p>x</p>"}}]
    }))
    .await;
    assert_eq!(report.unknown_tables.get("follower"), Some(&2));
    assert!(!tmp.path().join("site/follower").exists());
    assert!(tmp.path().join("site/note/5.md").exists());
}

#[tokio::test]
async fn a_note_with_an_image_becomes_a_bundle_fetched_with_the_pages_origin() {
    let mut server = mockito::Server::new_async().await;
    let img = server
        .mock("GET", "/pic/a.jpg")
        .match_header("referer", format!("{}/", server.url()).as_str())
        .with_status(200)
        .with_header("content-type", "image/jpeg")
        .with_body(b"\xff\xd8\xff\xe0fake".as_slice())
        .create_async()
        .await;
    let (tmp, report) = import(json!({"note": [{"id": 8, "note": {
        "title": "With picture", "url": format!("{}/note/8/", server.url()),
        "fulltext": format!("<p>See:</p><img src=\"{}/pic/a.jpg\">", server.url())
    }}]}))
    .await;
    img.assert_async().await;
    assert_eq!((report.media_downloaded, report.media_failed), (1, 0));
    let site = tmp.path().join("site");
    let page = read(&site, "note/8/index.md");
    let body = page.rsplit("---\n").next().unwrap();
    assert!(
        body.contains("![](./") && !body.contains("127.0.0.1"),
        "{page}"
    );
    assert!(!site.join("note/8.md").exists());
}

#[test]
fn moss_import_routes_a_backup_zip_to_the_reader() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = backup(tmp.path(), json!({"note": [{"id": 3, "note": {"title": "Routed", "fulltext": "<p>x</p>"}}]}));
    let out = tmp.path().join("site");
    let args = [zip.to_str().unwrap().to_string(), out.to_str().unwrap().to_string()];
    assert_eq!(crate::cli::import::run(&args), 0);
    assert!(out.join("note/3.md").exists());
}

#[tokio::test]
async fn a_table_that_is_not_a_list_and_a_row_without_a_body_are_counted_skipped() {
    let (tmp, report) = import(json!({
        "note": {"oops": true},
        "review": [{"id": 1, "review": {"title": "Empty", "fulltext": "", "abstract": ""}}]
    })).await;
    assert_eq!(report.skipped, 2);
    assert!(!tmp.path().join("site/review/1.md").exists());
}

#[test]
fn a_protocol_relative_image_is_fetched_over_https() {
    let page = note_page(&json!({"id": 1, "note": {"fulltext": "<img src=\"//img1.doubanio.example/a.jpg\"><img src=\"/rel.jpg\">"}})).unwrap();
    assert_eq!(
        page.images(),
        [("//img1.doubanio.example/a.jpg".to_string(), "https://img1.doubanio.example/a.jpg".to_string())]
    );
}

#[tokio::test]
async fn protocol_relative_images_are_fetched_over_https_and_duplicate_ids_get_their_own_folder() {
    let (tmp, report) = import(json!({"note": [
        {"id": 8, "note": {"title": "A", "url": "https://notes.example/note/8/", "fulltext": "<img src=\"//img.invalid/a.jpg\">"}},
        {"id": 8, "note": {"title": "B", "url": "https://notes.example/note/8/", "fulltext": "<img src=\"//img.invalid/b.jpg\">"}}
    ]})).await;
    // `.invalid` never resolves, so both fetches fail; the point is the https URL kept and the two folders.
    assert_eq!(report.media_failed, 2);
    let site = tmp.path().join("site");
    assert!(read(&site, "note/8/index.md").contains("img.invalid/a.jpg"));
    assert!(read(&site, "note/8 2/index.md").contains("img.invalid/b.jpg"));
}

#[test]
fn a_file_url_reaches_the_reader() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = backup(tmp.path(), json!({"note": [{"id": 3, "note": {"title": "T", "fulltext": "<p>x</p>"}}]}));
    let out = tmp.path().join("site");
    let args = [format!("file://{}", zip.display()), out.to_str().unwrap().to_string()];
    assert_eq!(crate::cli::import::run(&args), 0);
    assert!(out.join("note/3.md").exists());
}
