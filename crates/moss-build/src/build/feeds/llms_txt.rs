//! llms.txt: an index of the site's listed pages for AI tools, in the shape
//! <https://llmstxt.org/> describes. The site title, its description, then one
//! section per top-level folder, each page a link with its frontmatter
//! description. Only a page's title, frontmatter description, weight and date
//! shape the file, so editing a page's words leaves it byte-identical and a
//! publish of that edit does not send it again.

use crate::build::scan::article_map::to_pretty_url;
use crate::build::scan::page_map::external_url;
use crate::build::types::ParsedDocument;

/// The llms.txt index of `documents`' listed pages, linked under `site_url`.
pub fn generate_llms_txt(documents: &[ParsedDocument], site_title: &str, site_description: Option<&str>, site_url: &str) -> String {
    let mut pages: Vec<&ParsedDocument> = documents.iter().filter(|d| d.is_listable()).collect();
    // Weight ascending (none last), then newest first, then by label.
    pages.sort_by(|a, b| {
        a.weight.unwrap_or(i32::MAX).cmp(&b.weight.unwrap_or(i32::MAX))
            .then_with(|| b.date.cmp(&a.date))
            .then_with(|| a.label.cmp(&b.label))
    });

    // Root pages first, then each top-level folder in the order of its first page.
    let mut sections: Vec<(String, Vec<&ParsedDocument>)> = vec![(String::new(), Vec::new())];
    for page in pages {
        // A generated folder page has no source; its address names its folder.
        let path = page.source_path_slashed().unwrap_or_else(|| page.url_path.clone());
        let folder = path.split_once('/').map_or("", |(top, _)| top);
        match sections.iter_mut().find(|(name, _)| name == folder) {
            Some((_, listed)) => listed.push(page),
            None => sections.push((folder.to_string(), vec![page])),
        }
    }

    let base = site_url.trim_end_matches('/');
    let mut out = format!("# {site_title}\n\n");
    if let Some(description) = site_description {
        out.push_str(&format!("> {description}\n\n"));
    }
    for (folder, listed) in sections.iter().filter(|(_, listed)| !listed.is_empty()) {
        out.push_str(&format!("## {}\n\n", folder_title(documents, folder, site_title)));
        for page in listed {
            let url = external_url(&page.raw_frontmatter).unwrap_or_else(|| format!("{base}/{}", to_pretty_url(&page.url_path)));
            let label = page.label.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
            out.push_str(&format!("- [{label}]({url})"));
            if let Some(description) = frontmatter_description(page) {
                out.push_str(&format!(": {description}"));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// A folder's section heading: its index page's label, the homepage's for the
/// root, else the folder's name.
fn folder_title<'a>(documents: &'a [ParsedDocument], folder: &'a str, site_title: &'a str) -> &'a str {
    let index = if folder.is_empty() { "index.html".to_string() } else { format!("{folder}/index.html") };
    documents.iter().find(|d| d.url_path == index).map(|d| d.label.as_str())
        .unwrap_or(if folder.is_empty() { site_title } else { folder })
}

/// The description the author wrote, never one derived from the page's words.
fn frontmatter_description(page: &ParsedDocument) -> Option<String> {
    let text = page.raw_frontmatter.get("description")?.as_str()?;
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::PageKind;

    /// A page written at `source`, at the address moss gives it (`a/b.md` → `a/b/index.html`).
    fn page(title: &str, source: &str) -> ParsedDocument {
        let stem = source.trim_end_matches(".md");
        let url_path = match stem.strip_suffix("index") {
            Some(folder) => format!("{folder}index.html"),
            None => format!("{stem}/index.html"),
        };
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            content: format!("The words of {title}."),
            url_path,
            source_path: Some(source.to_string()),
            reading_time: 1,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn described(mut doc: ParsedDocument, description: &str) -> ParsedDocument {
        doc.raw_frontmatter.insert("description".into(), description.into());
        doc
    }

    const URL: &str = "https://example.org/";

    #[test]
    fn the_index_lists_each_page_as_a_link_under_its_folder() {
        let docs = vec![
            page("Home", "index.md"),
            described(page("About", "about.md"), "Who we are."),
            page("Notes", "notes/index.md"),
            described(page("First Note", "notes/first.md"), "A first\n  note."),
        ];
        let result = generate_llms_txt(&docs, "My Site", Some("A small site"), URL);
        assert_eq!(
            result,
            "# My Site\n\n> A small site\n\n\
             ## Home\n\n- [About](https://example.org/about/): Who we are.\n- [Home](https://example.org/)\n\n\
             ## Notes\n\n- [First Note](https://example.org/notes/first/): A first note.\n- [Notes](https://example.org/notes/)\n\n"
        );
    }

    #[test]
    fn a_generated_folder_page_lists_under_its_folder() {
        let mut generated = page("Events", "events/index.md");
        generated.source_path = None;
        let result = generate_llms_txt(&[page("Home", "index.md"), generated], "Site", None, URL);
        assert!(result.ends_with("## Events\n\n- [Events](https://example.org/events/)\n\n"), "{result}");
    }

    #[test]
    fn a_page_scanned_on_windows_lists_under_its_folder() {
        let mut note = page("First Note", "notes/first.md");
        note.source_path = Some("notes\\first.md".into());
        let result = generate_llms_txt(&[page("Home", "index.md"), page("Notes", "notes/index.md"), note], "Site", None, URL);
        assert!(result.ends_with("## Notes\n\n- [First Note](https://example.org/notes/first/)\n- [Notes](https://example.org/notes/)\n\n"), "{result}");
    }

    #[test]
    fn a_page_body_moves_no_byte_and_a_title_does() {
        let docs = vec![page("Doc", "doc.md"), described(page("Other", "other.md"), "Kept.")];
        let before = generate_llms_txt(&docs, "Site", None, URL);
        let mut edited = docs.clone();
        edited[0].content = "Entirely new words.".into();
        edited[0].description = Some("A summary drawn from the new words.".into());
        assert_eq!(generate_llms_txt(&edited, "Site", None, URL), before);
        edited[0].label = "Renamed".into();
        assert_ne!(generate_llms_txt(&edited, "Site", None, URL), before);
    }

    #[test]
    fn a_page_published_elsewhere_links_there() {
        let mut stub = page("Field Archive", "field.md");
        stub.raw_frontmatter.insert("external_url".into(), "https://archive.example.org/field/".into());
        let result = generate_llms_txt(&[stub], "Site", None, URL);
        assert!(result.contains("- [Field Archive](https://archive.example.org/field/)\n"), "{result}");
    }

    #[test]
    fn drafts_are_left_out() {
        let mut draft = page("Draft Page", "draft.md");
        draft.draft = Some(true);
        let result = generate_llms_txt(&[page("Published", "published.md"), draft], "Site", None, URL);
        assert!(result.contains("Published") && !result.contains("Draft Page"), "{result}");
    }

    #[test]
    fn pages_sort_by_weight_then_newest_first() {
        let weighted = |title: &str, weight: Option<i32>, date: Option<&str>| {
            let mut doc = page(title, &format!("{}.md", title.to_lowercase()));
            doc.weight = weight;
            doc.date = date.map(str::to_string);
            doc
        };
        let docs = vec![
            weighted("Older", None, Some("2024-01-01")),
            weighted("Second", Some(2), None),
            weighted("Newer", None, Some("2024-06-15")),
            weighted("First", Some(1), None),
        ];
        let result = generate_llms_txt(&docs, "Site", None, URL);
        let at = |t: &str| result.find(&format!("[{t}]")).unwrap();
        assert!(at("First") < at("Second") && at("Second") < at("Newer") && at("Newer") < at("Older"), "{result}");
    }

    #[test]
    fn an_empty_site_is_its_header() {
        assert_eq!(generate_llms_txt(&[], "Empty Site", Some("Nothing here"), URL), "# Empty Site\n\n> Nothing here\n\n");
    }
}
