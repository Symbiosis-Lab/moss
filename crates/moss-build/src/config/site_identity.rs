//! What a site says about who owns it: `rel="me"` links in every page's head
//! and the file a Bluesky domain handle is checked against.

use crate::build::cli_output::log_warn_problem;
use crate::build::page::meta::escape_html_attr;
use crate::config::ConfigFile;

#[derive(Debug, Clone, Default)]
pub struct SiteIdentity {
    /// `[site].me`, http(s) addresses only.
    me: Vec<String>,
    /// `[site].atproto_did`, a `did:plc:` or `did:web:` identifier.
    atproto_did: Option<String>,
}

impl SiteIdentity {
    /// Read `[site].me` and `[site].atproto_did`, warning about each value it drops.
    pub fn from_config(config: Option<&ConfigFile>) -> Self {
        let Some(config) = config else { return Self::default() };
        let me = config
            .site_strings("me")
            .into_iter()
            .filter(|address| {
                let ok = address.starts_with("https://") || address.starts_with("http://");
                if !ok {
                    log_warn_problem!("[site].me: \"{address}\" is not an http or https address, so no rel=\"me\" link is written for it");
                }
                ok
            })
            .collect();
        let atproto_did = config.site_str("atproto_did").map(str::trim).and_then(|did| {
            let ok = (did.starts_with("did:plc:") || did.starts_with("did:web:")) && !did.contains(char::is_whitespace);
            if !ok {
                log_warn_problem!("[site].atproto_did: \"{did}\" does not start with did:plc: or did:web:, so .well-known/atproto-did is not written");
            }
            ok.then(|| did.to_string())
        });
        Self { me, atproto_did }
    }

    /// One `<link rel="me">` per address, each with the head's four-space indent
    /// after the first, or `""`.
    pub fn head_links(&self) -> String {
        self.me
            .iter()
            .map(|address| format!("<link rel=\"me\" href=\"{}\">", escape_html_attr(address)))
            .collect::<Vec<_>>()
            .join("\n    ")
    }

    /// The DID to serve at `.well-known/atproto-did`. Authors cannot ship that
    /// file themselves: the scanner skips every dot-folder.
    pub fn atproto_did(&self) -> Option<&str> {
        self.atproto_did.as_deref()
    }
}
