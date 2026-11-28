//! `_moss/commerce.json`: the site's public description of its request forms
//! and its booking offer, at a fixed, unhashed address so a service can fetch it
//! by the site's URL.
//! On the moss host it also carries `subscribe.turnstile: true`: this build's
//! subscribe script can pass the host's bot check, so the host may require it.
//! And `subscribe.fields`, the questions the site's subscribe forms ask, keyed
//! by the edition scope each form posts, so the host can check the answers.
//! And `subscribe.newsletter`, the service `[channels.email] provider` names,
//! for the host to send each subscriber on to.
//!
//! Written when the site has a form, a `[services.forms]` setting, a
//! `[services.booking]` offer, or a moss host site id. It sits under `_moss/`,
//! which the publish removal gate does not guard, so removing the last form
//! never refuses a publish.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use moss_core::ast::{FieldKind, FormField, FormShortcode, SubscribeShortcode};

use crate::build::context::BuildContext;
use crate::build::manifest::{link_audit, HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::ParsedDocument;
use crate::config::services::{BookingService, FormsOff};

pub const COMMERCE_PATH: &str = "_moss/commerce.json";

/// Every form of every page, in page order, repeated names included; and every
/// subscribe form that asks questions, with its page's edition scope and URL
/// path. Each page is parsed once for both.
fn site_forms(documents: &[ParsedDocument]) -> (Vec<FormShortcode>, Vec<(String, String, SubscribeShortcode)>) {
    let scopes = crate::build::features::email::site_scopes(documents);
    let mut forms = Vec::new();
    let mut subscribes = Vec::new();
    for d in documents.iter().filter(|d| d.features.forms) {
        let ast = moss_core::ast::parse(&d.content);
        forms.extend(moss_core::ast::forms_recursive(&ast));
        let scope = crate::build::features::email::detect_page_scope(&d.url_path, &scopes);
        subscribes.extend(moss_core::ast::question_subscribes_recursive(&ast).into_iter().map(|s| (scope.clone(), d.url_path.clone(), s)));
    }
    (forms, subscribes)
}

/// The fields as a service reads them; a choice's "Other" box follows its
/// field as a text field whose `other_of` names that field, with the box's
/// own words as its `label`, since a reader of the answers sees no page.
fn described(fields: &[&FormField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .flat_map(|field| {
            let mut o = serde_json::json!({"name": field.name, "type": field.kind, "required": field.required});
            if !field.choices.is_empty() {
                o["options"] = serde_json::json!(field.choices);
            }
            let other = field.other.as_ref().map(|b| serde_json::json!({"name": b.name, "label": b.label, "type": FieldKind::Text, "required": false, "other_of": field.name}));
            std::iter::once(o).chain(other)
        })
        .collect()
}

/// `subscribe.fields`: per scope, every question its subscribe forms ask, the
/// subscribe address left out. Choices are the union across the forms and a
/// question is required only where every form asks it as required; forms that
/// disagree on a question's kind or choices are a problem.
fn subscribe_fields(subscribes: &[(String, String, SubscribeShortcode)]) -> (serde_json::Value, Vec<String>) {
    let mut by_scope: BTreeMap<&str, (usize, Vec<(serde_json::Value, usize, &str)>)> = BTreeMap::new();
    let mut problems = Vec::new();
    for (scope, page, sub) in subscribes {
        let email_at = sub.fields.iter().position(|f| f.kind == FieldKind::Email || f.name == "email");
        let asked: Vec<&FormField> = sub.fields.iter().enumerate().filter(|(i, _)| Some(*i) != email_at).map(|(_, f)| f).collect();
        let (forms, fields) = by_scope.entry(scope.as_str()).or_default();
        *forms += 1;
        for field in described(&asked) {
            let Some((known, required, first)) = fields.iter_mut().find(|(f, _, _)| f["name"] == field["name"]) else {
                let required = usize::from(field["required"] == true);
                fields.push((field, required, page.as_str()));
                continue;
            };
            *required += usize::from(field["required"] == true);
            if known["type"] != field["type"] || known["options"] != field["options"] {
                problems.push(format!(
                    "The subscribe forms on `{first}` and `{page}` both ask `{}`, with different kinds or choices; a question asked twice in one edition must be asked the same way.",
                    field["name"].as_str().unwrap_or_default()
                ));
            }
            for choice in field["options"].as_array().into_iter().flatten() {
                let options = known["options"].as_array_mut();
                if let Some(options) = options.filter(|o| !o.contains(choice)) {
                    options.push(choice.clone());
                }
            }
        }
    }
    let fields = by_scope
        .into_iter()
        .map(|(scope, (forms, fields))| {
            let described: Vec<_> = fields
                .into_iter()
                .map(|(mut f, required, _)| {
                    f["required"] = serde_json::Value::Bool(required == forms);
                    f
                })
                .collect();
            (scope.to_string(), serde_json::Value::Array(described))
        })
        .collect::<serde_json::Map<_, _>>();
    (serde_json::Value::Object(fields), problems)
}

fn json(forms: &[FormShortcode], subscribe_fields: serde_json::Value, newsletter: Option<serde_json::Value>, booking: Option<&BookingService>, hosted: bool) -> String {
    let mut seen = HashSet::new();
    let described: serde_json::Map<String, serde_json::Value> = forms
        .iter()
        .filter_map(|f| f.name.as_ref().filter(|n| seen.insert((*n).clone())).map(|n| (n, f)))
        .map(|(name, f)| {
            let fields = described(&f.fields.iter().collect::<Vec<_>>());
            (name.clone(), serde_json::json!({"fields": fields, "next": f.next, "turnstile": hosted}))
        })
        .collect();
    let mut out = serde_json::json!({ "forms": described });
    if hosted {
        out["subscribe"] = serde_json::json!({"turnstile": true});
        if subscribe_fields.as_object().is_some_and(|f| !f.is_empty()) {
            out["subscribe"]["fields"] = subscribe_fields;
        }
        if let Some(newsletter) = newsletter {
            out["subscribe"]["newsletter"] = newsletter;
        }
    }
    if let Some(booking) = booking {
        out["booking"] = serde_json::to_value(booking).expect("a booking offer serializes");
    }
    out.to_string()
}

/// The problems only the whole site can see: a name used by two forms or by a
/// subscribe form's edition (their entries would share one file in the folder),
/// forms with nowhere to post, and a booking offer or newsletter service that cannot be published.
fn problems(forms: &[FormShortcode], scopes: &HashSet<&str>, endpoint: Result<&str, FormsOff>, booking: Option<&Result<BookingService, String>>, newsletter: Option<&Result<serde_json::Value, String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out: Vec<String> = forms
        .iter()
        .filter_map(|f| f.name.as_deref())
        .filter(|n| !seen.insert(*n))
        .map(|n| format!("`:::form {{#{n}}}` is used by more than one form; a form name must be unique in the site."))
        .collect();
    let mut reported = HashSet::new();
    let shared = forms.iter().filter_map(|f| f.name.as_deref()).filter(|n| scopes.contains(n) && reported.insert(*n));
    out.extend(shared.map(|n| format!("`:::form {{#{n}}}` has the name of the `{n}` edition's subscribe form, and both keep their entries in .moss/data/forms/{n}.csv; rename the form.")));
    match endpoint {
        _ if forms.is_empty() => {}
        Ok(_) => {}
        Err(FormsOff::SwitchedOff) => out.push("Request forms are switched off (`[services.forms] enabled = false`): published pages do not show them.".to_string()),
        Err(FormsOff::NoEndpoint) => out.push("The site has request forms but no `[services.forms] endpoint` in .moss/config.toml, so they cannot send: published pages do not show them, and the preview simulates sending.".to_string()),
    }
    if let Some(Err(problem)) = booking {
        out.push(format!("{problem}; the offer is left out of {COMMERCE_PATH} until it is fixed."));
    }
    if let Some(Err(problem)) = newsletter {
        out.push(format!("{problem}; the newsletter service is left out of {COMMERCE_PATH} until it is fixed."));
    }
    out
}

/// `subscribe.newsletter` as published, or why it is left out.
fn destination(service: &Result<String, String>, hosted: bool) -> Result<serde_json::Value, String> {
    match service {
        Ok(_) if !hosted => Err("[channels.email] provider is read only by the moss host, and this site is published elsewhere".to_string()),
        s => s.clone().map(|provider| serde_json::json!({ "provider": provider })),
    }
}

/// The stage hook: reports the site's form problems and writes the description.
pub fn emit(
    documents: &[ParsedDocument],
    endpoint: Result<&str, FormsOff>,
    booking: Option<&Result<BookingService, String>>,
    newsletter: Option<&Result<String, String>>,
    hosted: bool,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let (forms, subscribes) = site_forms(documents);
    let (subscribe_fields, disagreements) = subscribe_fields(&subscribes);
    let newsletter = newsletter.map(|n| destination(n, hosted));
    let scopes: HashSet<&str> = subscribes.iter().map(|(scope, _, _)| scope.as_str()).collect();
    for problem in problems(&forms, &scopes, endpoint, booking, newsletter.as_ref()).into_iter().chain(disagreements) {
        crate::build::cli_output::log_warn_problem!("{problem}");
    }
    let booking = booking.and_then(|b| b.as_ref().ok());
    if forms.is_empty() && endpoint.is_err() && booking.is_none() && !hosted {
        return Ok(());
    }
    BuildContext::for_render(output_dir, pending)
        .emit_held(&ServedPath::for_commerce_json(), json(&forms, subscribe_fields, newsletter.and_then(Result::ok), booking, hosted).into_bytes(), HashBucket::Files)
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
