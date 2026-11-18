//! What a site says about who owns it: the head elements and `.well-known`
//! files that make its domain the person's identity on other networks.
//!
//! Two sources feed one list: `[site].me` and `[site].atproto_did`, and each
//! installed plugin's `contributes.identity`, filled from its own table.

use std::path::Path;

use crate::build::cli_output::log_warn_problem;
use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::page::meta::escape_html_attr;
use crate::config::ConfigFile;
use crate::plugins::contributions::identity::{self, Claim, Supplied};
use crate::plugins::types::Plugin;
use crate::types::content::SiteHashes;

#[derive(Debug, Clone, Default)]
pub struct SiteIdentity {
    /// Head elements, escaped, in order: `[site].me` first, then plugins by name.
    head: Vec<String>,
    /// `.well-known` files as (path, body, supplier). Authors cannot ship
    /// these themselves: the scanner skips every dot-folder.
    files: Vec<(String, String, String)>,
}

impl SiteIdentity {
    /// Read `[site].me`, `[site].atproto_did` and every plugin's proofs,
    /// warning about each value it drops and each path two suppliers claim.
    pub fn from_config(config: Option<&ConfigFile>, plugins: &[Plugin]) -> Self {
        let Some(config) = config else { return Self::default() };
        let mut claims: Vec<Supplied> = Vec::new();
        for address in config.site_strings("me") {
            if address.starts_with("https://") || address.starts_with("http://") {
                claims.push(Supplied { supplier: "[site].me".into(), claim: Claim::Head(format!("<link rel=\"me\" href=\"{}\">", escape_html_attr(&address))) });
            } else {
                log_warn_problem!("[site].me: \"{address}\" is not an http or https address, so no rel=\"me\" link is written for it");
            }
        }
        if let Some(did) = config.site_str("atproto_did").map(str::trim) {
            if (did.starts_with("did:plc:") || did.starts_with("did:web:")) && !did.contains(char::is_whitespace) {
                claims.push(Supplied { supplier: "[site].atproto_did".into(), claim: Claim::File { path: ".well-known/atproto-did".into(), body: format!("{did}\n") } });
            } else {
                log_warn_problem!("[site].atproto_did: \"{did}\" does not start with did:plc: or did:web:, so .well-known/atproto-did is not written");
            }
        }
        let (from_plugins, problems) = identity::supplied(plugins, config);
        for sentence in problems {
            log_warn_problem!(target: "plugin", "{sentence}");
        }
        claims.extend(from_plugins);

        let mut out = Self::default();
        for Supplied { supplier, claim } in claims {
            match claim {
                Claim::Head(tag) => {
                    if !out.head.contains(&tag) {
                        out.head.push(tag);
                    }
                }
                Claim::File { path, body } => {
                    if let Some((_, _, first)) = out.files.iter().find(|(p, _, _)| *p == path) {
                        log_warn_problem!(target: "plugin", "{path} is supplied by both {first} and {supplier}; {first}'s is written");
                    } else {
                        out.files.push((path, body, supplier));
                    }
                }
            }
        }
        out
    }

    /// The head elements, each with the head's four-space indent after the
    /// first, or `""`.
    pub fn head_links(&self) -> String {
        self.head.join("\n    ")
    }

    /// Write each `.well-known` file, and name each one the previous build
    /// served that this one drops: it was someone's live handle.
    pub fn emit(&self, previous: &SiteHashes, output_dir: &Path, pending: &mut PendingManifest) -> Result<(), String> {
        let mut gone: Vec<&String> = previous.files.keys()
            .filter(|p| ServedPath::for_well_known(p).is_some())
            .filter(|p| !self.files.iter().any(|(path, _, _)| path == *p))
            .collect();
        gone.sort();
        for path in gone {
            log_warn_problem!("identity: {path} removed");
        }
        let mut ctx = BuildContext::for_render(output_dir, pending);
        for (path, body, supplier) in &self.files {
            let Some(served) = ServedPath::for_well_known(path) else { continue };
            log::info!(target: "build", "identity: {supplier} writes {path}");
            ctx.emit(&served, body.as_bytes(), HashBucket::Files).map_err(|e| format!("Failed to emit {path}: {e}"))?;
        }
        Ok(())
    }
}
