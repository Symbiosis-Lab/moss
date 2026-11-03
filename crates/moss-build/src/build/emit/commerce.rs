//! `_moss/commerce.json`: the site's public description of its request forms,
//! at a fixed, unhashed address so a form service can fetch it by the site's URL.
//!
//! Written when the site has a form or a `[services.forms]` setting. It sits
//! under `_moss/`, which the publish removal gate does not guard, so removing
//! the last form never refuses a publish.

use std::collections::HashSet;
use std::path::Path;

use moss_core::ast::FormShortcode;

use crate::build::context::BuildContext;
use crate::build::manifest::{link_audit, HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;

pub const COMMERCE_PATH: &str = "_moss/commerce.json";

/// Every form of every page, in page order, repeated names included.
fn site_forms(documents: &[ParsedDocument]) -> Vec<FormShortcode> {
    documents
        .iter()
        .filter(|d| d.features.forms)
        .flat_map(|d| moss_core::ast::forms_recursive(&moss_core::ast::parse(&d.content)))
        .collect()
}

fn json(forms: &[FormShortcode]) -> String {
    let mut seen = HashSet::new();
    let described: serde_json::Map<String, serde_json::Value> = forms
        .iter()
        .filter_map(|f| f.name.as_ref().filter(|n| seen.insert((*n).clone())).map(|n| (n, f)))
        .map(|(name, f)| {
            let fields: Vec<_> = f
                .fields
                .iter()
                .map(|field| {
                    let mut o = serde_json::json!({"name": field.name, "type": field.kind, "required": field.required});
                    if !field.choices.is_empty() {
                        o["options"] = serde_json::json!(field.choices);
                    }
                    o
                })
                .collect();
            (name.clone(), serde_json::json!({"fields": fields, "next": f.next, "turnstile": false}))
        })
        .collect();
    serde_json::json!({ "forms": described }).to_string()
}

/// The problems only the whole site can see: a name used by two forms, and
/// forms with nowhere to post.
fn problems(forms: &[FormShortcode], endpoint: Option<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = forms
        .iter()
        .filter_map(|f| f.name.as_deref())
        .filter(|n| !seen.insert(*n))
        .map(|n| format!("`:::form {{#{n}}}` is used by more than one form; a form name must be unique in the site."))
        .collect();
    if !forms.is_empty() && endpoint.is_none() {
        out.push("The site has request forms but no `[services.forms] endpoint` in .moss/config.toml, so they cannot send: published pages do not show them, and the preview simulates sending.".to_string());
    }
    out
}

/// The stage hook: reports the site's form problems and writes the description.
pub fn emit(
    documents: &[ParsedDocument],
    endpoint: Option<&str>,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let forms = site_forms(documents);
    for problem in problems(&forms, endpoint) {
        crate::build::cli_output::log_warn_problem!("{problem}");
    }
    if forms.is_empty() && endpoint.is_none() {
        return Ok(());
    }
    BuildContext::for_render(output_dir, pending)
        .emit_held(&ServedPath::for_commerce_json(), json(&forms).into_bytes(), HashBucket::Files)
        .map_err(|e| format!("Failed to emit {COMMERCE_PATH}: {e}"))
}

/// After the manifest seals: a form whose `next` page this build did not write is a
/// problem, judged the way the link audit judges every other root-relative link.
pub fn audit_next(stage_dir: &Path, sealed: &SealedManifest) {
    let Ok(text) = std::fs::read_to_string(stage_dir.join(COMMERCE_PATH)) else { return };
    let Ok(description) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let files = sealed.files();
    let satisfied: HashSet<&str> = files.keys().map(String::as_str).collect();
    for (name, form) in description["forms"].as_object().into_iter().flatten() {
        if let Some(next) = form["next"].as_str().filter(|n| link_audit::is_dead(n, &satisfied, stage_dir)) {
            crate::build::cli_output::log_warn_problem!("`:::form {{#{name}}}` has `next={next}`, which names a page this build wrote nothing for.");
        }
    }
}

#[cfg(test)]
#[path = "commerce_tests.rs"]
mod tests;
