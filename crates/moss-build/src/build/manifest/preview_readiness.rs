//! Readiness projection for the selected preview page.
//!
//! Input evidence and direct output receipts meet here: a route is usable only
//! when its requested source closure is resolved and its directly linked CSS
//! and scripts match the bytes under the root the preview will serve.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewReadiness {
    Usable,
    Pending,
    Missing,
}

pub(super) fn preview_readiness_for(
    files: &HashMap<String, String>,
    source_to_output: &HashMap<String, String>,
    evidence: Option<&std::collections::BTreeMap<String, crate::build::cloud_ledger::InputEntry>>,
    embeds: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    uses_places: bool,
    requirement: &crate::system::folder_session::PreviewRequirement,
    registered: impl Fn(&str) -> bool,
) -> PreviewReadiness {
    use crate::build::cloud_ledger::{InputState, unresolved_preview_entries};
    use crate::system::folder_session::PreviewSource;
    let Some(evidence) = evidence else { return PreviewReadiness::Pending; };
    let output = files.keys()
        .filter(|key| registered(key))
        .find(|key| crate::build::served_path::served_address(key) == requirement.url_path);
    if matches!(&requirement.source, PreviewSource::Unresolved) {
        return PreviewReadiness::Pending;
    }
    let mut needed = std::collections::BTreeSet::new();
    // Authored pages also compose nav and child/card listings from the
    // corpus. Until the renderer records a narrower per-page metadata read
    // set, every page depends on the page metadata it could not read.
    let mut source_pending = false;
    let mut source_rel = None;
    if let PreviewSource::File(source) = &requirement.source {
        let rel = moss_core::slug::normalize_separators(&source.to_string_lossy());
        source_rel = Some(rel.clone());
        needed.insert(rel.clone());
        needed.extend(embeds.get(&rel).into_iter().flat_map(|deps| deps.iter().cloned()));
        source_pending = evidence.get(&rel).is_some_and(|entry|
            matches!(entry.state, InputState::Pending { .. } | InputState::ReadError { .. }));
        if let Some(output) = output {
            if source_to_output.get(&rel).is_none_or(|mapped| mapped != output) {
                return if source_pending { PreviewReadiness::Pending } else { PreviewReadiness::Missing };
            }
        }
    }
    let unresolved = unresolved_preview_entries(evidence, &needed, true, uses_places, source_rel.as_deref());
    if output.is_none() {
        return if source_pending || !unresolved.is_empty() { PreviewReadiness::Pending }
            else { PreviewReadiness::Missing };
    }
    if unresolved.is_empty() { PreviewReadiness::Usable } else { PreviewReadiness::Pending }
}

/// Check the selected page's render-blocking local dependencies against this
/// generation's manifest and bytes under the root that will actually serve it.
/// Images and other page resources are deliberately outside this readiness
/// proof; they may still be produced by the background pipeline.
pub(crate) fn required_page_outputs_present(
    files: &HashMap<String, String>,
    page: &str,
    served_root: &Path,
) -> bool {
    use lol_html::{element, HtmlRewriter, Settings};

    let Some(page_entry) = files.get(page) else { return false; };
    let (mode, expected) = crate::types::content::parse_entry(page_entry);
    if mode != crate::types::content::MODE_FILE { return false; }
    let Ok(page_bytes) = std::fs::read(served_file_path(served_root, page)) else { return false; };
    // Generated HTML keeps preview-only source annotations in staging; the
    // manifest receipt describes the exact transformed bytes materialize will
    // promote. Compare the same transformed form without changing served HTML.
    let shipped_page = crate::build::ship::apply_transform(
        crate::build::ship::transform_for(page), &page_bytes,
    );
    if format!("{:016x}", xxhash_rust::xxh3::xxh3_64(&shipped_page)) != expected { return false; }
    let Ok(html) = std::str::from_utf8(&page_bytes) else { return false; };

    let references = std::cell::RefCell::new(Vec::new());
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("link[rel][href]", |el| {
                    let stylesheet = el.get_attribute("rel").is_some_and(|rel| {
                        rel.split_ascii_whitespace().any(|token| token.eq_ignore_ascii_case("stylesheet"))
                    });
                    if stylesheet {
                        if let Some(href) = el.get_attribute("href") { references.borrow_mut().push(href); }
                    }
                    Ok(())
                }),
                element!("script[src]", |el| {
                    if let Some(src) = el.get_attribute("src") { references.borrow_mut().push(src); }
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );
    if rewriter.write(html.as_bytes()).is_err() || rewriter.end().is_err() { return false; }

    references.into_inner().into_iter().all(|reference| {
        let key = match required_output_key(page, &reference) {
            Ok(None) => return true,
            Ok(Some(key)) => key,
            Err(()) => return false,
        };
        let Some(entry) = files.get(&key) else { return false; };
        let (mode, expected) = crate::types::content::parse_entry(entry);
        let bytes = std::fs::read(served_file_path(served_root, &key));
        let matches = bytes.as_ref().is_ok_and(|bytes| format!("{:016x}", xxhash_rust::xxh3::xxh3_64(bytes)) == expected);
        mode == crate::types::content::MODE_FILE && matches
    })
}

fn served_file_path(root: &Path, key: &str) -> std::path::PathBuf {
    // Callers reach this only for a key already obtained from the manifest;
    // generated `_moss/` outputs are trusted there but rejected as user input.
    crate::build::served_path::ServedPath::from_cached(key)
        .map(|path| path.to_disk(root))
        .unwrap_or_else(|_| root.join(".__invalid_required_output_path__"))
}

fn required_output_key(page: &str, reference: &str) -> Result<Option<String>, ()> {
    let reference = moss_core::html_entities::decode(reference);
    let reference = reference.trim();
    if reference.is_empty() || reference.starts_with('#')
        || reference.starts_with("//")
        || reference.contains(':') && reference.find(':').is_some_and(|colon| {
            reference[..colon].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
    {
        return Ok(None);
    }
    let path = reference.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() { return Ok(None); }
    if path.starts_with('/') {
        let keys = crate::build::manifest::link_audit::candidate_keys(path).ok_or(())?;
        let key = keys.into_iter().find(|key| !key.ends_with("/index.html")).ok_or(())?;
        return Ok(Some(key));
    }

    let decoded = percent_encoding::percent_decode_str(path).decode_utf8().map_err(|_| ())?;
    let mut components: Vec<&str> = page.rsplit_once('/')
        .map(|(parent, _)| parent.split('/').collect()).unwrap_or_default();
    for component in decoded.split('/') {
        match component {
            "" | "." => {}
            ".." => { components.pop().ok_or(())?; }
            part => components.push(part),
        }
    }
    let key = components.join("/");
    // Dependency paths are accepted as cache output names only after the
    // caller finds that exact normalized key in the sealed manifest.
    crate::build::served_path::ServedPath::from_cached(&key).map_err(|_| ())?;
    Ok(Some(key))
}

impl super::PendingManifest {
    /// Existing input proof plus the selected page's readable output receipts.
    pub(crate) fn preview_readiness(
        &self,
        requirement: &crate::system::folder_session::PreviewRequirement,
        served_root: &Path,
    ) -> PreviewReadiness {
        let evidence = self.input_evidence.as_ref().map(|entry| entry.snapshot());
        let readiness = preview_readiness_for(
            &self.inner.files,
            &self.inner.source_to_output,
            evidence.as_ref(),
            &self.preview_embeds,
            self.preview_uses_places,
            requirement,
            |key| self.touched.contains(key),
        );
        if readiness != PreviewReadiness::Usable { return readiness; }
        let output = self.inner.files.keys()
            .find(|key| crate::build::served_path::served_address(key) == requirement.url_path);
        if output.is_some_and(|key| required_page_outputs_present(&self.inner.files, key, served_root)) {
            PreviewReadiness::Usable
        } else {
            PreviewReadiness::Pending
        }
    }
}

impl super::SealedManifest {
    pub(crate) fn preview_output_key(
        &self,
        requirement: &crate::system::folder_session::PreviewRequirement,
    ) -> Option<&str> {
        self.inner.files.keys()
            .find(|key| crate::build::served_path::served_address(key) == requirement.url_path)
            .map(String::as_str)
    }

    /// Keep input closure and essential output proof on the same selected seal.
    pub(crate) fn preview_readiness_with_outputs(
        &self,
        requirement: &crate::system::folder_session::PreviewRequirement,
        served_root: &Path,
    ) -> PreviewReadiness {
        let readiness = self.preview_readiness(requirement);
        if readiness != PreviewReadiness::Usable { return readiness; }
        self.preview_output_key(requirement)
            .filter(|key| required_page_outputs_present(&self.inner.files, key, served_root))
            .map_or(PreviewReadiness::Pending, |_| PreviewReadiness::Usable)
    }

    /// Apply the shared critical-output decision to existing ship evidence.
    pub(crate) fn mark_missing_preview_outputs(
        &mut self,
        requirement: &crate::system::folder_session::PreviewRequirement,
        served_root: &Path,
    ) -> Option<String> {
        let page = self.preview_output_key(requirement)?.to_owned();
        if required_page_outputs_present(&self.inner.files, &page, served_root) { return None; }
        let detail = "selected page has a missing or mismatched local stylesheet/script output";
        self.mark_unverified(page.clone(), detail.to_string());
        Some(format!("{page} ({detail})"))
    }
}
