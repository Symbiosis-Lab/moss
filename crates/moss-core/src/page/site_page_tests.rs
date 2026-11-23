use std::collections::HashMap;

use super::{site_pages, PageFacts};
use crate::page::SiteBundle;
use crate::resolve::embeds::derived::{DerivedFacts, DerivedRecord};

fn pages(texts: &[(&str, &str)], other_files: &[&str], bundle: SiteBundle) -> Vec<PageFacts> {
    let texts: HashMap<String, String> = texts.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut files: Vec<String> = texts.keys().cloned().chain(other_files.iter().map(|f| f.to_string())).collect();
    files.sort();
    site_pages(&texts, &SiteBundle { files, lang: "en".into(), ..bundle }, "site")
}

fn label_of<'a>(pages: &'a [PageFacts], url: &str) -> &'a str {
    &pages.iter().find(|p| p.url_path == url).unwrap_or_else(|| panic!("no page at {url}")).label
}

fn report_bundle(fragments: &[&str]) -> SiteBundle {
    SiteBundle {
        derivable: [("pdf".to_string(), vec!["text".to_string(), "summary".to_string()])].into(),
        derived: [("report.pdf".to_string(), DerivedRecord {
            fragments: fragments.iter().map(|f| f.to_string()).collect(),
            facts: DerivedFacts { title: Some("Annual report".into()), date: Some("2020-06-09".into()), ..Default::default() },
        })].into(),
        ..Default::default()
    }
}

#[test]
fn inferred_title_and_date_come_from_an_embed_the_page_still_has() {
    let page = "---\ninferred: [title, date]\n---\n\n![[report.pdf#text]]\n";
    let got = pages(&[("notes.md", page)], &["report.pdf"], report_bundle(&["text"]));
    assert_eq!(label_of(&got, "notes/index.html"), "Annual report");
    assert_eq!(got[0].date.as_deref(), Some("2020-06-09"));

    let removed = pages(&[("notes.md", "---\ninferred: [title, date]\n---\n\nNo embed.\n")], &["report.pdf"], report_bundle(&["text"]));
    assert_ne!(label_of(&removed, "notes/index.html"), "Annual report");
    assert_eq!(removed[0].date, None);
}

#[test]
fn a_fragment_the_derivation_did_not_produce_gives_no_facts() {
    let page = "---\ninferred: [title]\n---\n\n![[report.pdf#summary]]\n";
    let got = pages(&[("notes.md", page)], &["report.pdf"], report_bundle(&["text"]));
    assert_ne!(label_of(&got, "notes/index.html"), "Annual report");
}

#[test]
fn a_page_s_own_title_wins_over_an_inferred_one() {
    let page = "---\ntitle: Mine\ninferred: [title]\n---\n\n![[report.pdf#text]]\n";
    let got = pages(&[("notes.md", page)], &["report.pdf"], report_bundle(&["text"]));
    assert_eq!(label_of(&got, "notes/index.html"), "Mine");
}

#[test]
fn an_untitled_post_is_labelled_from_its_cleaned_body() {
    let got = pages(&[("2024-01-02.md", "%%draft note%% Walked {++to the++} river today.\n")], &[], SiteBundle::default());
    assert_eq!(got.len(), 1);
    assert!(!got[0].label.contains("draft note"), "{}", got[0].label);
    assert!(got[0].label.contains("to the river"), "{}", got[0].label);
}

#[test]
fn an_untitled_post_label_reads_math_as_the_site_sets_it() {
    let body = [("2024-01-02.md", "Costs $[^a]$ here.\n\n[^a]: Note.\n")];
    let on = pages(&body, &[], SiteBundle::default());
    let off = pages(&body, &[], SiteBundle { math: Some(false), ..Default::default() });
    assert_ne!(on[0].label, off[0].label);
}

#[test]
fn a_folder_with_a_verbatim_index_html_has_no_markdown_home() {
    let got = pages(&[("app/app.md", "# App notes\n")], &["app/index.html"], SiteBundle::default());
    assert!(got.iter().all(|p| p.url_path != "app/index.html"), "{:?}", got.iter().map(|p| &p.url_path).collect::<Vec<_>>());

    let open = pages(&[("app/app.md", "# App notes\n")], &[], SiteBundle::default());
    assert_eq!(open[0].url_path, "app/index.html");
}
