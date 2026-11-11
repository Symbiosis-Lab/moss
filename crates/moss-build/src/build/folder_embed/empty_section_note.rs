//! The note a page gets when headings were left out because the folder embeds under them list nothing.

use crate::build::types::ParsedDocument;

/// `ids` are the anchors of the headings left out, in document order.
pub(super) fn note_dropped_sections(doc: &ParsedDocument, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
    let list = |ids: &[&String]| ids.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ");
    let all: Vec<&String> = ids.iter().collect();
    // A later heading of the same name may have taken a dropped heading's anchor.
    let unanswered: Vec<&String> = ids.iter().filter(|id| !doc.html_content.contains(&format!(" id=\"{id}\""))).collect();
    let target = if unanswered.is_empty() {
        String::new()
    } else {
        format!("; a link to {} has no target until then", list(&unanswered))
    };
    crate::build::cli_output::log_note!(
        "[{page}] left out the heading {}: the folder embeds under it list nothing yet, and it appears once a page exists{target}.",
        list(&all)
    );
}
