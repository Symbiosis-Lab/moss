//! The note a page gets when headings were left out because the folder embeds under them list nothing.

use crate::build::types::ParsedDocument;

/// `dropped` are the anchors of the headings left out, in document order, each with whether a kept heading of the same name answers it.
pub(super) fn note_dropped_sections(doc: &ParsedDocument, dropped: &[(String, bool)]) {
    if dropped.is_empty() {
        return;
    }
    let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
    let list = |ids: &[&String]| ids.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ");
    let all: Vec<&String> = dropped.iter().map(|(id, _)| id).collect();
    let unanswered: Vec<&String> = dropped.iter().filter(|(_, answered)| !answered).map(|(id, _)| id).collect();
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
