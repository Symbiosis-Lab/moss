//! Every page's term lines and place names, stored where the renderer reads them.

use super::TermIndex;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
pub use moss_core::terms::lines::TermLine;

impl ParsedDocument {
    /// Whether this page draws its place line, which already names its places.
    pub fn draws_place_line(&self) -> bool {
        self.term_lines.iter().any(|line| line.is_place)
    }
}

/// Set every page's [`ParsedDocument::term_lines`] and [`ParsedDocument::place_names`].
pub fn set_term_lines(documents: &mut [ParsedDocument], index: &TermIndex, site_lang: Language) {
    use moss_core::terms::lines::{place_names, term_lines};
    for doc in documents.iter_mut() {
        doc.place_names = place_names(doc, index);
        doc.term_lines = term_lines(doc, index, site_lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::terms::TermKind;
    use crate::build::terms::derive_terms;
    use moss_core::terms::TermClaim;

    fn doc(url: &str, title: &str) -> ParsedDocument {
        ParsedDocument { url_path: url.to_string(), title: title.to_string(), label: title.to_string(), ..Default::default() }
    }

    fn kind(key: &str, fields: &[&str]) -> TermKind {
        TermKind {
            cards: false,
            key: key.to_string(),
            fields: fields.iter().map(|f| f.to_string()).collect(),
            title: key.to_string(),
            titles: Vec::new(),
            counts: Vec::new(),
            is_place: false,
            parents: Default::default(),
            explorer: None,
            line: None,
            slugs: Default::default(),
        }
    }

    fn tags_with_line() -> TermKind {
        TermKind { line: Some(true), ..kind("tags", &["tags"]) }
    }

    fn places() -> TermKind {
        TermKind { is_place: true, ..kind("places", &["location"]) }
    }

    fn tagged(url: &str, title: &str, tags: &[&str]) -> ParsedDocument {
        ParsedDocument { fm_tags: Some(tags.iter().map(|t| t.to_string()).collect()), ..doc(url, title) }
    }

    fn lines_of(mut docs: Vec<ParsedDocument>, kinds: Vec<TermKind>) -> Vec<ParsedDocument> {
        let index = derive_terms(&mut docs, kinds);
        set_term_lines(&mut docs, &index, Language::En);
        docs
    }

    #[test]
    fn a_tags_line_links_each_name_with_the_fields_label() {
        let docs = lines_of(vec![tagged("letters/a/index.html", "A", &["Field Notes", "Slow Reading"])], vec![tags_with_line()]);
        assert_eq!(docs[0].term_lines, vec![TermLine {
            term: "tags".into(),
            field: "tags".into(),
            label: "Tags".into(),
            separator: ": ".into(),
            links: "[Field Notes](/tags/field-notes/), [Slow Reading](/tags/slow-reading/)".into(),
            is_place: false,
        }]);
    }

    #[test]
    fn a_kind_draws_no_line_unless_it_asks_for_one() {
        let docs = lines_of(vec![tagged("letters/a/index.html", "A", &["Field Notes"])], vec![kind("tags", &["tags"])]);
        assert!(docs[0].term_lines.is_empty());
    }

    #[test]
    fn a_kind_draws_one_line_per_field_in_field_order() {
        let mut page = doc("letters/a/index.html", "A");
        page.author = vec!["Ada Example".to_string()];
        page.editor = vec!["Ben Sample".to_string()];
        let people = TermKind { line: Some(true), ..kind("people", &["author", "editor"]) };
        let docs = lines_of(vec![page], vec![people]);
        let fields: Vec<&str> = docs[0].term_lines.iter().map(|l| l.field.as_str()).collect();
        assert_eq!(fields, ["author", "editor"]);
    }

    #[test]
    fn a_german_page_links_its_own_editions_term_page_and_says_its_label_in_german() {
        let mut page = tagged("de/letters/a/index.html", "A", &["Field Notes"]);
        page.edition = "de".to_string();
        page.lang = Language::De;
        let docs = lines_of(vec![page], vec![tags_with_line()]);
        assert_eq!(docs[0].term_lines[0].links, "[Field Notes](/de/tags/field-notes/)");
        assert_eq!(docs[0].term_lines[0].label, "Schlagwörter");
    }

    #[test]
    fn a_claimed_tag_links_the_claiming_page() {
        let mut claim = doc("about/field-notes/index.html", "Field Notes");
        claim.tag_page = Some(TermClaim::UseTitle);
        let docs = lines_of(vec![claim, tagged("letters/a/index.html", "A", &["Field Notes"])], vec![tags_with_line()]);
        assert_eq!(docs[1].term_lines[0].links, "[Field Notes](/about/field-notes/)");
    }

    #[test]
    fn a_page_titled_with_its_only_tag_draws_no_line() {
        let docs = lines_of(
            vec![
                tagged("letters/a/index.html", "field notes", &["Field Notes"]),
                tagged("letters/b/index.html", "Field Notes", &["Field Notes", "Slow Reading"]),
            ],
            vec![tags_with_line()],
        );
        assert!(docs[0].term_lines.is_empty(), "{:?}", docs[0].term_lines);
        assert_eq!(docs[1].term_lines[0].links, "[Slow Reading](/tags/slow-reading/)");
    }

    #[test]
    fn a_name_with_brackets_stays_one_link() {
        let docs = lines_of(vec![tagged("letters/a/index.html", "A", &["Notes [draft]"])], vec![tags_with_line()]);
        let html = crate::build::render::credits::render_byline_html(&[], false, &docs[0].term_lines).unwrap();
        assert!(html.contains(r#"<a href="/tags/notes-draft/">Notes [draft]</a>"#), "{html}");
    }

    #[test]
    fn the_place_line_links_names_in_the_order_written_with_its_label() {
        let mut claim = doc("about/kyoto/index.html", "Kyoto");
        claim.place_page = Some(TermClaim::UseTitle);
        let mut page = doc("posts/a/index.html", "A");
        page.location = vec!["Kyoto".to_string()];
        let docs = lines_of(vec![claim, page], vec![places()]);
        let line = &docs[1].term_lines[0];
        assert_eq!((line.label.as_str(), line.separator.as_str(), line.links.as_str()), ("Location", ": ", "[Kyoto](/about/kyoto/)"));
        assert_eq!((line.term.as_str(), line.field.as_str(), line.is_place), ("places", "location", true));
    }

    #[test]
    fn the_place_line_stays_site_wide_and_in_the_site_language_on_a_translated_page() {
        let mut page = doc("de/posts/a/index.html", "A");
        page.edition = "de".to_string();
        page.lang = Language::De;
        page.location = vec!["Kyoto".to_string()];
        let docs = lines_of(vec![page], vec![places()]);
        assert_eq!(docs[0].term_lines[0].links, "[Kyoto](/places/kyoto/)");
        assert_eq!(docs[0].term_lines[0].label, "Location");
    }

    #[test]
    fn a_place_kind_fed_by_another_field_still_draws_one_location_line() {
        let mut page = doc("posts/a/index.html", "A");
        page.raw_frontmatter = serde_json::from_str(r#"{"place": "Kyoto"}"#).unwrap();
        let docs = lines_of(vec![page], vec![TermKind { is_place: true, ..kind("places", &["place"]) }]);
        assert_eq!(docs[0].term_lines.len(), 1);
        assert_eq!((docs[0].term_lines[0].field.as_str(), docs[0].term_lines[0].label.as_str()), ("location", "Location"));
    }

    #[test]
    fn only_the_first_place_kind_draws_a_line() {
        let mut page = doc("posts/a/index.html", "A");
        page.location = vec!["Kyoto".to_string()];
        let docs = lines_of(vec![page], vec![places(), TermKind { is_place: true, ..kind("venues", &["location"]) }]);
        assert_eq!(docs[0].term_lines.len(), 1);
    }

    #[test]
    fn line_false_drops_the_place_line_but_still_sets_place_names() {
        let mut page = doc("posts/a/index.html", "A");
        page.location = vec!["Kyoto".to_string()];
        let docs = lines_of(vec![page], vec![TermKind { line: Some(false), ..places() }]);
        assert!(docs[0].term_lines.is_empty());
        assert!(!docs[0].draws_place_line());
        assert_eq!(docs[0].place_names.as_deref(), Some("Kyoto"));
    }

    #[test]
    fn a_page_titled_with_its_place_draws_no_line_but_keeps_its_place_names() {
        let mut a = doc("posts/a/index.html", "kyoto");
        a.location = vec!["Kyoto".to_string()];
        let mut b = doc("posts/b/index.html", "Kyoto");
        b.location = vec!["Kyoto".to_string(), "Nara".to_string()];
        let docs = lines_of(vec![a, b], vec![places()]);
        assert!(docs[0].term_lines.is_empty());
        assert_eq!(docs[0].place_names.as_deref(), Some("Kyoto"));
        assert_eq!(docs[1].term_lines[0].links, "[Nara](/places/nara/)");
        assert_eq!(docs[1].place_names.as_deref(), Some("Kyoto, Nara"));
    }

    #[test]
    fn place_names_drops_an_unresolvable_name() {
        let mut page = doc("posts/a/index.html", "A");
        page.location = vec!["Kyoto".to_string(), "！！！".to_string()];
        let docs = lines_of(vec![page], vec![places()]);
        assert_eq!(docs[0].place_names.as_deref(), Some("Kyoto"));
    }

    #[test]
    fn no_place_kind_means_no_place_line_and_no_place_names() {
        let mut page = doc("posts/a/index.html", "A");
        page.location = vec!["Kyoto".to_string()];
        let docs = lines_of(vec![page], vec![kind("tags", &["tags"])]);
        assert!(docs[0].term_lines.is_empty());
        assert_eq!(docs[0].place_names, None);
    }

    #[test]
    fn a_page_with_no_location_has_no_place_line_or_place_names() {
        let docs = lines_of(vec![doc("posts/a/index.html", "A")], vec![places()]);
        assert!(docs[0].term_lines.is_empty());
        assert_eq!(docs[0].place_names, None);
    }
}
