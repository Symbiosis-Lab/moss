//! Pure request and response shapes for the seta client: the error bodies it
//! recognizes and the verification request it sends. Status and body in, value
//! out, so they test without a `reqwest::Response`.

/// The JSON body of a 403 whose `error` field is exactly `code`.
fn error_body(status: u16, body: &str, code: &str) -> Option<serde_json::Value> {
    if status != 403 {
        return None;
    }
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    (json.get("error").and_then(|v| v.as_str()) == Some(code)).then_some(json)
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

/// Parse a `site_limit_reached` 403 into `(limit, contact)`. A body without a
/// numeric `limit` is not this error and falls through to the generic path; a
/// blank `contact` counts as absent.
pub(super) fn parse_site_limit_reached(status: u16, body: &str) -> Option<(u32, Option<String>)> {
    let json = error_body(status, body, "site_limit_reached")?;
    let limit = u32::try_from(json.get("limit")?.as_u64()?).ok()?;
    let contact = json
        .get("contact")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    Some((limit, contact))
}

