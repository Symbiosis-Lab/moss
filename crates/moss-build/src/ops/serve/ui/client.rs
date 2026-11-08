//! Local owner discovery authenticates with the token file, never a cookie.
use super::super::{carrier_token, ownership};
use std::path::Path;

pub struct OwnerLink {
    pub local: String,
    pub public: String,
}

/// Blocking, for a command-line caller with no runtime of its own.
pub fn link_for_owner(folder: &Path) -> Result<Option<OwnerLink>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let Some(owner) = runtime.block_on(ownership::find_live_owner(folder)) else {
        return Ok(None);
    };
    let local = owner
        .ui_url
        .ok_or("this folder's live owner does not serve the editor")?;
    let token = std::fs::read_to_string(carrier_token::token_path(folder))
        .map_err(|_| "cannot read the live owner's token file")?;
    let endpoint = format!("{}/__moss/sign-in/new", owner.url);
    // Redirects must not carry a local capability to another listener.
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let value: serde_json::Value = agent
        .post(&endpoint)
        .set(carrier_token::TOKEN_HEADER, token.trim())
        .send_string("")
        .map_err(|_| "could not request a sign-in link from the live owner")?
        .into_json()
        .map_err(|_| "invalid sign-in response")?;
    let public = value["url"].as_str().ok_or("missing sign-in link")?.to_owned();
    let url = url::Url::parse(&public).map_err(|_| "invalid sign-in link")?;
    if url.path() != "/__moss/sign-in"
        || url.query().is_some()
        || !url.fragment().is_some_and(|f| f.starts_with("code="))
    {
        return Err("invalid sign-in link".into());
    }
    let local = format!("{local}/__moss/sign-in#{}", url.fragment().unwrap());
    Ok(Some(OwnerLink { local, public }))
}
