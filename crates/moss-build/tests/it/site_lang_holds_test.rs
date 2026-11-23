//! A site that declares its language keeps it on every page whose words lean
//! another way: English names and bios on a Traditional Chinese site are still
//! a Chinese page. Only the page's own `lang:` (or a folder's declaration, a
//! language-named folder or a language suffix) says otherwise. Guessing from
//! the words is left to a site that declares nothing.

use crate::site::Site;

const BIOS: &str = "Translation award, season three\n\n\
Jane Doe is a translator and editor based in London. She has translated novels, essays and poetry from \
Chinese into English for many years, and her work has appeared in several literary magazines and anthologies. \
John Roe is a writer and translator who teaches comparative literature and has published widely on poetry \
and translation in English-language journals.\n\n第三屆翻譯獎。\n";

fn files() -> Vec<(&'static str, String)> {
    vec![
        ("index.md", "---\ntitle: 首頁\n---\n\n歡迎來到我們的網站，這裡有獎學金與翻譯獎的消息。\n".to_string()),
        ("awards/index.md", "---\ntitle: 翻譯獎\n---\n\n翻譯獎的介紹。\n".to_string()),
        ("awards/season-3.md", format!("---\ntitle: 第三屆\n---\n\n{BIOS}")),
    ]
}

fn as_refs<'a>(files: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    files.iter().map(|(p, b)| (*p, b.as_str())).collect()
}

#[test]
fn an_english_heavy_page_on_a_declared_chinese_site_stays_chinese() {
    let files = files();
    let site = Site::in_language("zh-tw", "", &as_refs(&files));

    for page in ["awards/season-3/index.html", "awards/index.html"] {
        let html = site.html(page);
        assert!(html.contains(r#"<html data-moss-html-version="1" lang="zh-Hant">"#), "{page}: {}", &html[..html.len().min(400)]);
        assert!(!html.contains("Skip to content"), "{page} has English chrome");
    }
}

#[test]
fn a_pages_own_lang_still_overrides_the_declared_site() {
    let mut files = files();
    files.push(("awards/notes.md", format!("---\ntitle: Notes\nlang: en\n---\n\n{BIOS}")));
    let site = Site::in_language("zh-tw", "", &as_refs(&files));

    let html = site.html("awards/notes/index.html");
    assert!(html.contains(r#"<html data-moss-html-version="1" lang="en">"#), "{}", &html[..html.len().min(400)]);
}
