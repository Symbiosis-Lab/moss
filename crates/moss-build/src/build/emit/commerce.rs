//! `_moss/commerce.json`: the site's public description of its request forms
//! and its booking offer, at a fixed, unhashed address so a service can fetch it
//! by the site's URL.
//! On the moss host it also carries `subscribe.turnstile: true`: this build's
//! subscribe script can pass the host's bot check, so the host may require it.
//!
//! Written when the site has a form, a `[services.forms]` setting, a
//! `[services.booking]` offer, or a moss host site id. It sits under `_moss/`,
//! which the publish removal gate does not guard, so removing the last form
//! never refuses a publish.

use std::collections::HashSet;
use std::path::Path;

use moss_core::ast::FormShortcode;

use crate::build::context::BuildContext;
use crate::build::manifest::{link_audit, HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;
use crate::config::services::BookingService;

pub const COMMERCE_PATH: &str = "_moss/commerce.json";

/// Every form of every page, in page order, repeated names included.
fn site_forms(documents: &[ParsedDocument]) -> Vec<FormShortcode> {
    documents
        .iter()
        .filter(|d| d.features.forms)
        .flat_map(|d| moss_core::ast::forms_recursive(&moss_core::ast::parse(&d.content)))
        .collect()
}

fn json(forms: &[FormShortcode], booking: Option<&BookingService>, hosted: bool) -> String {
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
    let mut out = serde_json::json!({ "forms": described });
    if hosted {
        out["subscribe"] = serde_json::json!({"turnstile": true});
    }
    if let Some(booking) = booking {
        out["booking"] = serde_json::to_value(booking).expect("a booking offer serializes");
    }
    out.to_string()
}

/// The problems only the whole site can see: a name used by two forms, forms
/// with nowhere to post, and a booking offer that cannot be published.
fn problems(forms: &[FormShortcode], endpoint: Option<&str>, booking: Option<&Result<BookingService, String>>) -> Vec<String> {
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
    if let Some(Err(problem)) = booking {
        out.push(format!("{problem}; the offer is left out of {COMMERCE_PATH} until it is fixed."));
    }
    out
}

/// The stage hook: reports the site's form problems and writes the description.
pub fn emit(
    documents: &[ParsedDocument],
    endpoint: Option<&str>,
    booking: Option<&Result<BookingService, String>>,
    hosted: bool,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let forms = site_forms(documents);
    for problem in problems(&forms, endpoint, booking) {
        crate::build::cli_output::log_warn_problem!("{problem}");
    }
    let booking = booking.and_then(|b| b.as_ref().ok());
    if forms.is_empty() && endpoint.is_none() && booking.is_none() && !hosted {
        return Ok(());
    }
    BuildContext::for_render(output_dir, pending)
        .emit_held(&ServedPath::for_commerce_json(), json(&forms, booking, hosted).into_bytes(), HashBucket::Files)
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
    for (key, page) in description["booking"]["pages"].as_object().into_iter().flatten() {
        if let Some(page) = page.as_str().filter(|p| link_audit::is_dead(p, &satisfied, stage_dir)) {
            crate::build::cli_output::log_warn_problem!("[services.booking] pages.{key} is `{page}`, a page this build wrote nothing for.");
        }
    }
}

#[cfg(test)]
#[path = "commerce_tests.rs"]
mod tests;
