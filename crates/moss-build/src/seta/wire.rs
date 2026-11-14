//! Pure request and response shapes for the seta client: the error bodies it
//! recognizes and the verification request it sends. Status and body in, value
//! out, so they test without a `reqwest::Response`. Also the per-deploy request
//! tally, which holds numbers and an edge code only.

use super::client::SetaError;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, OnceLock};

tokio::task_local! { static DEPLOY_WIRE: Arc<WireCounters>; }

/// What one deploy asked of the hosting server, counted where each request
/// goes out and its answer comes back ([`super::client::MossSetaClient::send`]).
#[derive(Default)]
pub(crate) struct WireCounters {
    requests: AtomicU64,
    failed: AtomicU64,
    sent_bytes: AtomicU64,
    resent_bytes: AtomicU64,
    edge: OnceLock<String>,
}

impl WireCounters {
    /// The tally of the deploy this task runs in; a client built outside one
    /// keeps a tally nobody reads.
    pub(crate) fn current() -> Arc<Self> {
        DEPLOY_WIRE.try_with(Arc::clone).unwrap_or_default()
    }

    pub(crate) async fn scope<F: std::future::Future>(wire: Arc<Self>, body: F) -> F::Output {
        DEPLOY_WIRE.scope(wire, body).await
    }

    /// One request answered (`status`) or lost in transport (`None`).
    /// `file_bytes` is the body of a file upload or chunk, else 0; a failed
    /// request's body is sent again, so it counts as resent.
    pub(crate) fn note(&self, file_bytes: u64, status: Option<u16>, ray: Option<&str>) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        match status {
            Some(s) if (200..300).contains(&s) => { self.sent_bytes.fetch_add(file_bytes, Ordering::Relaxed); }
            Some(s) if s != 429 && s < 500 => {}
            _ => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                self.resent_bytes.fetch_add(file_bytes, Ordering::Relaxed);
            }
        }
        if let Some(ray) = ray {
            let _ = self.edge.set(edge_of(ray).map_or("none".into(), str::to_ascii_uppercase));
        }
    }

    /// Kilobytes round up, so a deploy that sent something never reads 0.
    pub(crate) fn counts(&self) -> [(&'static str, u64); 4] {
        let kb = |a: &AtomicU64| a.load(Ordering::Relaxed).div_ceil(1024);
        [("requests", self.requests.load(Ordering::Relaxed)),
         ("requests_failed", self.failed.load(Ordering::Relaxed)),
         ("sent_kb", kb(&self.sent_bytes)), ("resent_kb", kb(&self.resent_bytes))]
    }

    pub(crate) fn edge(&self) -> &str {
        self.edge.get().map_or("none", String::as_str)
    }
}

/// The location code after the last `-` of a `cf-ray` value: 3 or 4 ASCII
/// letters. The ray id itself is never kept.
pub(super) fn edge_of(ray: &str) -> Option<&str> {
    let code = ray.rsplit_once('-')?.1;
    ((3..=4).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphabetic())).then_some(code)
}

/// The JSON body of a 403 whose `error` field is exactly `code`.
fn error_body(status: u16, body: &str, code: &str) -> Option<serde_json::Value> {
    if status != 403 {
        return None;
    }
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    (json.get("error").and_then(|v| v.as_str()) == Some(code)).then_some(json)
}

/// A refusal the server names in a `code` field: `storage_cap_exceeded` on a
/// 413 and `deploy_superseded` on a 409, whose `error` is the sentence to
/// show; `folder_not_signed_in` on a 403 and `key_has_other_site` on a 409,
/// which name a `site`; `update_required` on a 410, whose `message` is the
/// sentence to show and `min_version` the oldest moss the route accepts. Any other body, or a known code on another status, is
/// not one of these and falls through to the generic path.
pub(super) fn parse_coded_refusal(status: u16, body: &str) -> Option<SetaError> {
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).map(str::to_string);
    match (status, json.get("code")?.as_str()?) {
        (413, "storage_cap_exceeded") => Some(SetaError::StorageCapExceeded {
            cap_bytes: json.get("cap_bytes")?.as_u64()?,
            site_bytes: json.get("site_bytes").and_then(|v| v.as_u64()),
            message: text("error")?,
        }),
        (409, "deploy_superseded") => Some(SetaError::DeploySuperseded { message: text("error")? }),
        (422, "gated_not_enforceable") => Some(SetaError::GatedNotEnforceable { message: text("error")? }),
        (403, "folder_not_signed_in") => Some(SetaError::NotSignedIn { site: text("site")? }),
        (409, "key_has_other_site") => Some(SetaError::KeyHasOtherSite { site: text("site")? }),
        (410, "update_required") => Some(SetaError::UpdateRequired { min_version: text("min_version"), message: text("message")? }),
        _ => None,
    }
}

/// Whether `lang` looks like a language tag: `^[a-z]{2,3}(-[a-z0-9]{2,8}){0,2}$`,
/// case-insensitive.
fn is_lang_tag(lang: &str) -> bool {
    let mut parts = lang.split('-');
    let primary = parts.next().unwrap_or("");
    if !(2..=3).contains(&primary.len()) || !primary.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    rest.len() <= 2
        && rest
            .iter()
            .all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The operating system as a person names it.
fn os_label() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

/// `<app> on <kind>` ("moss command line on Mac Studio"); just the app when
/// the kind is unknown.
pub(super) fn device_name(app: &str, kind: Option<&str>) -> String {
    kind.map_or_else(|| app.to_string(), |kind| format!("{app} on {kind}"))
}

/// The sign-in request. `client` names this app and the kind of computer on
/// the page the emailed link opens, so whoever reads the mail can tell
/// whether they asked for it. `folder` adds the site and the folder's name
/// when known.
pub(super) fn auth_request_body(
    email: &str,
    pubkey: &str,
    session_id: &str,
    lang: Option<&str>,
    folder: &super::client::SignInFolder,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "email": email,
        "pubkey": pubkey,
        "session_id": session_id,
        "client": {
            "name": device_name(crate::system::app_name(), crate::system::device_kind()),
            "version": crate::system::app_version(),
            "os": os_label(),
        },
    });
    if let Some(lang) = lang.filter(|l| is_lang_tag(l)) {
        body["lang"] = lang.into();
    }
    if let Some(site_id) = &folder.site_id {
        body["site_id"] = site_id.as_str().into();
    }
    if let Some(name) = &folder.name {
        body["folder_name"] = name.as_str().into();
    }
    body
}

/// Parse a `site_limit_reached` 403. A body without a numeric `limit` is not
/// this error and falls through to the generic path; a blank `contact` counts
/// as absent.
pub(super) fn parse_site_limit_reached(status: u16, body: &str) -> Option<SetaError> {
    let json = error_body(status, body, "site_limit_reached")?;
    let limit = u32::try_from(json.get("limit")?.as_u64()?).ok()?;
    let contact = json
        .get("contact")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    Some(SetaError::SiteLimitReached { limit, contact })
}

