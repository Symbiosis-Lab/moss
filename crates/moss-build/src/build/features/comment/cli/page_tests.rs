use super::*;
use crate::build::types::ParsedDocument;

/// Invented comments on two pages; `a.md` builds to page "pg-a".
fn site() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let d = tempfile::TempDir::new_in(&base).unwrap();
    let social = d.path().join(".moss/data/social");
    std::fs::create_dir_all(&social).unwrap();
    std::fs::write(
        social.join("comment.json"),
        r#"{"articles":{
            "pg-a":{"comments":[
                {"id":"1","content":"<p>Register at https://win.example/r for a bonus</p>","createdAt":"2026-03-01T10:00:00Z","author":{"displayName":"Bonus","url":"https://www.meds.example/"}},
                {"id":"2","content":"<p>Lovely light</p>","createdAt":"2026-03-02T10:00:00Z","author":{"displayName":"Ana","url":"https://ana.example"}}]},
            "pg-b":{"comments":[
                {"id":"3","content":"<p>Elsewhere</p>","createdAt":"2026-03-03T10:00:00Z","author":{"displayName":"Ben"}}]}}}"#,
    )
    .unwrap();
    let doc = ParsedDocument { source_path: Some("a.md".into()), url_path: "a/index.html".into(), uid: Some("pg-a".into()), ..Default::default() };
    crate::build::emit::inventory::write_inventory(&[doc], &d.path().join(".moss"), false, "en", None).unwrap();
    d
}

#[test]
fn a_page_lists_its_own_comments_newest_first() {
    let d = site();
    let page = page_comments(d.path(), "a.md");
    let ids: Vec<_> = page.comments.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["2", "1"], "another page's comment is not listed");
    assert_eq!(page.comments[1].website.as_deref(), Some("meds.example"));
    assert_eq!(page.comments[1].text, "Register at https://win.example/r for a bonus");
    assert!(page.comments.iter().all(|c| !c.hidden));
}

#[test]
fn a_source_no_build_has_reached_has_no_comments() {
    let d = site();
    assert_eq!(page_comments(d.path(), "new.md"), PageComments::default());
}

#[test]
fn a_comment_spam_hid_by_its_host_can_be_shown_again() {
    let d = site();
    let id = crate::identity::keypair::Identity::generate().unwrap();
    id.save(d.path()).unwrap();
    let ps = d.path().to_str().unwrap().to_string();
    crate::infra::home::with_moss_home(|_| {
        super::super::append_owner_event(&ps, super::super::ModKind::Block, super::super::LINK_SOURCE, "ana.example", "", "")
    })
    .unwrap();
    let page = page_comments(d.path(), "a.md");
    let ana = page.comments.iter().find(|c| c.id == "2").unwrap();
    assert!(ana.hidden && ana.directly_hidden, "{ana:?}");
}
