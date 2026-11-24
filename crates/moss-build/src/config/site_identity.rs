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
use crate::build::scan::article_map::to_pretty_url;
use crate::plugins::contributions::identity::{self, Claim, Supplied};
use crate::syndication::record::{CopyRecord, VerifyFile, VerifyLink};
use crate::plugins::types::Plugin;
use crate::types::content::SiteHashes;

#[derive(Debug, Clone, Default)]
pub struct SiteIdentity {
    /// Head elements, escaped, in order: `[site].me` first, then plugins by name.
    head: Vec<String>,
    /// `.well-known` files as (path, body, supplier). Authors cannot ship
    /// these themselves: the scanner skips every dot-folder.
    files: Vec<(String, String, String)>,
    /// Head elements for one page, keyed by its pretty url path, from copy records.
    page_links: std::collections::HashMap<String, Vec<String>>,
}

impl SiteIdentity {
    /// Read `[site].me`, `[site].atproto_did` and every plugin's proofs,
    /// warning about each value it drops and each path two suppliers claim.
    pub fn from_config(config: Option<&ConfigFile>, plugins: &[Plugin], project_path: &Path) -> Self {
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
        let (from_plugins, problems) = identity::supplied(plugins, config, &project_path.to_string_lossy());
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

    /// Add what the site's copy records ask it to carry: a link for the page
    /// each names and a `.well-known` file each. Only records for `site_url`
    /// count; each ask passes the identity allowlists or is dropped with a
    /// warning, and where two records give one path two bodies the newest wins.
    pub fn add_copy_verification(&mut self, mut records: Vec<CopyRecord>, site_url: &str) {
        let site = site_url.trim_end_matches('/');
        records.retain(|r| r.verify.is_some() && r.site_url.as_deref().is_some_and(|u| u.trim_end_matches('/') == site));
        records.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        let mut from_records: Vec<(String, String, String)> = Vec::new();
        for record in records {
            let supplier = format!("the {} copy of {}", record.channel, record.source_path);
            let Some(verify) = record.verify else { continue };
            if let Some(VerifyLink { rel, href }) = verify.page_link {
                match identity::page_link_ok(&rel, &href) {
                    Ok(()) => {
                        let tag = identity::page_link_tag(&href);
                        let links = self.page_links.entry(to_pretty_url(&record.url_path)).or_default();
                        if !links.contains(&tag) {
                            links.push(tag);
                        }
                    }
                    Err(why) => log_warn_problem!(target: "plugin", "copy verification: {supplier} asks for a link that is dropped: {why}"),
                }
            }
            if let Some(VerifyFile { path, body }) = verify.site_file {
                if let Err(why) = identity::site_file_ok(&path, &body) {
                    log_warn_problem!(target: "plugin", "copy verification: {supplier} asks for {path}, which is dropped: {why}");
                } else if let Some(at) = from_records.iter().position(|(p, _, _)| *p == path) {
                    if from_records[at].1 != body {
                        log_warn_problem!(target: "plugin", "copy verification: {path} is asked for with different bodies; {supplier}'s, the newest, is written");
                    }
                    from_records[at] = (path, body, supplier);
                } else {
                    from_records.push((path, body, supplier));
                }
            }
        }
        for (path, body, supplier) in from_records {
            if let Some((_, _, first)) = self.files.iter().find(|(p, _, _)| *p == path) {
                log_warn_problem!(target: "plugin", "{path} is supplied by both {first} and {supplier}; {first}'s is written");
            } else {
                self.files.push((path, body, supplier));
            }
        }
    }

    /// The head elements for each page that has any, joined as the head wants them.
    pub fn page_links(&self) -> std::collections::HashMap<String, String> {
        self.page_links.iter().map(|(page, tags)| (page.clone(), tags.join("\n    "))).collect()
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
