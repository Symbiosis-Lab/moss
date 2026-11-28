//! `contributes.identity`: the proofs that make a site's domain the person's
//! identity on a network, declared as templates and filled on every build from
//! the plugin's own `[services.<plugin-id>]` table.
//!
//! A proof is a file under `.well-known/`, a `<link>` or a `<meta>` in every
//! page's head. Core knows no network: the plugin names the path, the rel or
//! the meta name, and which of its settings fill it. No plugin code runs, so a
//! build without plugin hooks writes the same proofs.

use serde::{Deserialize, Serialize};

use crate::build::page::meta::escape_html_attr;
use crate::config::ConfigFile;
use crate::plugins::types::Plugin;

use super::settings::{Field, FieldSource, FieldType};

/// The `<link rel>` values a proof may write into every page's head. Any other
/// rel (`canonical`, `stylesheet`, `manifest`) would let a plugin reshape every
/// page, so a network that needs one more is added here by name.
const LINK_RELS: &[&str] = &["me"];
/// The `<meta name>` values a proof may write, for the same reason (`robots`,
/// `referrer`).
const META_NAMES: &[&str] = &["fediverse:creator"];
/// The `.well-known` files a proof may write. Others (`webauthn`,
/// `assetlinks.json`, `apple-app-site-association`) grant an app authority
/// over the domain, so a network's proof file is added here by name.
const WELL_KNOWN_FILES: &[&str] = &[".well-known/atproto-did", ".well-known/nostr.json", ".well-known/site.standard.publication"];
/// The one `<link rel>` a copy record may add to a single page's head, and the
/// scheme its href must start with: a record of the page on a network.
const PAGE_LINK_REL: &str = "site.standard.document";
const PAGE_LINK_HREF_PREFIX: &str = "at:";
/// Largest body a `.well-known` file may declare.
const MAX_BODY: usize = 64 * 1024;
/// Longest value a setting may fill into a proof.
const MAX_VALUE: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Proof {
    File(FileProof),
    Link(LinkProof),
    Meta(MetaProof),
}

/// `{ "file": ".well-known/atproto-did", "body": "{did}" }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileProof {
    pub file: String,
    pub body: String,
}

/// `{ "link": "me", "href": "https://{account.host}/@{account.user}" }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkProof {
    pub link: String,
    pub href: String,
}

/// `{ "meta": "fediverse:creator", "content": "@{account.user}@{account.host}" }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaProof {
    pub meta: String,
    pub content: String,
}

/// One proof a build writes, with the plugin (or `[site]` key) it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// A path under `.well-known/` and its body.
    File { path: String, body: String },
    /// One element for every page's head, already escaped.
    Head(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supplied {
    pub supplier: String,
    pub claim: Claim,
}

/// Check a plugin's proofs when its manifest is read. `declared` is the keys
/// its `site_settings` draws: a proof may read only those, so the person can
/// see and change every value that ends up on their domain.
pub fn validate(plugin: &str, proofs: &[Proof], declared: &[Field]) -> Result<(), String> {
    for proof in proofs {
        let (what, templates) = match proof {
            Proof::File(f) => {
                well_known_path_ok(&f.file).map_err(|why| format!("identity `{}`: {why}", f.file))?;
                if f.body.len() > MAX_BODY {
                    return Err(format!("identity `{}`: a body is at most {MAX_BODY} bytes", f.file));
                }
                (f.file.clone(), vec![f.body.as_str()])
            }
            Proof::Link(l) => {
                if !LINK_RELS.contains(&l.link.as_str()) {
                    return Err(format!("identity link `{}`: a proof writes only rel {}", l.link, LINK_RELS.join(", ")));
                }
                (format!("link {}", l.link), vec![l.href.as_str()])
            }
            Proof::Meta(m) => {
                if !META_NAMES.contains(&m.meta.as_str()) {
                    return Err(format!("identity meta `{}`: a proof writes only meta {}", m.meta, META_NAMES.join(", ")));
                }
                (format!("meta {}", m.meta), vec![m.content.as_str()])
            }
        };
        for template in templates {
            let keys = placeholders(template);
            if keys.is_empty() {
                return Err(format!("identity `{what}`: a proof reads at least one setting, such as {{did}}"));
            }
            for key in &keys {
                let field = declared.iter().find(|f| f.key == key.name)
                    .ok_or_else(|| format!("identity `{what}`: `{}` is not a setting plugin `{plugin}` declares in site_settings", key.name))?;
                if !matches!(field.field_type, FieldType::String | FieldType::Acct) {
                    return Err(format!("identity `{what}`: `{}` is not text", key.name));
                }
                if key.part.is_some() && field.field_type != FieldType::Acct {
                    return Err(format!("identity `{what}`: .user and .host are read only from an `acct` setting, and `{}` is not one", key.name));
                }
            }
        }
        if let Proof::File(f) = proof {
            if is_json(&f.file) {
                let sample = fill(&f.body, |_| Some("x".to_string()), json_escape).unwrap_or_default();
                serde_json::from_str::<serde_json::Value>(&sample)
                    .map_err(|e| format!("identity `{}`: a .json body must be JSON with each value inside a string: {e}", f.file))?;
            }
        }
    }
    Ok(())
}

/// Every proof the site's plugins supply, filled from their tables. A proof
/// whose settings are not all set is the person not having set that network
/// up, and is skipped without a word; a value that cannot be written is
/// skipped with a sentence.
///
/// An `acct` setting the manifest marks `from: "account"` reads the folder's
/// signed-in account while its table has no value of its own.
pub fn supplied(plugins: &[Plugin], config: &ConfigFile, project_path: &str) -> (Vec<Supplied>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut sorted: Vec<&Plugin> = plugins.iter().collect();
    sorted.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    for plugin in sorted {
        let Some(proofs) = plugin.manifest.contributes.as_ref().and_then(|c| c.identity.as_ref()) else { continue };
        let name = &plugin.manifest.name;
        let table = config.section(&["services", name]);
        let declared: &[Field] = plugin.manifest.contributes.as_ref().and_then(|c| c.site_settings.as_ref()).map_or(&[], |s| &s.settings);
        let mut bad = Vec::new();
        let mut get = |key: &Key| -> Option<String> {
            let field = declared.iter().find(|f| f.key == key.name);
            let written = table.and_then(|t| t.get(&key.name)).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
            let signed_in;
            let raw = match written {
                Some(raw) => raw,
                None if field.is_some_and(|f| f.from == Some(FieldSource::Account)) => {
                    signed_in = match signed_in_acct(project_path, name)? {
                        Some(acct) => acct,
                        None => {
                            bad.push(key.name.clone());
                            return None;
                        }
                    };
                    &signed_in
                }
                None => return None,
            };
            if raw.len() > MAX_VALUE || raw.chars().any(char::is_control) {
                bad.push(key.name.clone());
                return None;
            }
            let pattern = field.and_then(|f| f.pattern.as_deref());
            if super::settings::pattern_refuses(pattern, raw) {
                bad.push(key.name.clone());
                return None;
            }
            let Some(part) = key.part else { return Some(raw.to_string()) };
            let Some((user, host)) = split_acct(raw) else {
                bad.push(key.name.clone());
                return None;
            };
            Some(if part == Part::User { user } else { host }.to_string())
        };
        for proof in proofs {
            let claim = match proof {
                Proof::File(f) => {
                    let escape = if is_json(&f.file) { json_escape } else { plain };
                    fill(&f.body, &mut get, escape).map(|body| Claim::File { path: f.file.clone(), body })
                }
                Proof::Link(l) => fill(&l.href, &mut get, plain)
                    .filter(|href| href.starts_with("https://") || href.starts_with("http://"))
                    .map(|href| Claim::Head(format!("<link rel=\"{}\" href=\"{}\">", l.link, escape_html_attr(&href)))),
                Proof::Meta(m) => fill(&m.content, &mut get, plain)
                    .map(|content| Claim::Head(format!("<meta name=\"{}\" content=\"{}\">", m.meta, escape_html_attr(&content)))),
            };
            if let Some(claim) = claim {
                out.push(Supplied { supplier: name.clone(), claim });
            }
        }
        bad.sort();
        bad.dedup();
        problems.extend(bad.into_iter().map(|key| format!("[services.{name}] `{key}` cannot be written into the site's identity proof, so that proof is left out")));
    }
    (out, problems)
}

/// The folder's signed-in account as `@name@host`. Outer `None`: no account, or
/// signed out. Inner `None`: the account's name is not an address on its own server.
fn signed_in_acct(project_path: &str, plugin: &str) -> Option<Option<String>> {
    let binding = crate::plugins::manager::accounts::recorded(project_path, plugin)?;
    let name = binding.name?;
    Some(split_acct(&name).filter(|(_, host)| Some(*host) == binding.host.as_deref()).map(|(user, host)| format!("@{user}@{host}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    User,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    name: String,
    part: Option<Part>,
}

/// One piece of a template: literal text, or a setting to fill in. A
/// placeholder is `{name}`, `{name.user}` or `{name.host}` with a lowercase
/// name; any other brace is text, so a JSON body needs no escaping.
enum Piece<'a> {
    Text(&'a str),
    Key(Key),
}

fn pieces(template: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut i = 0;
    while let Some(offset) = template[i..].find('{') {
        let open = i + offset;
        let after = &template[open + 1..];
        let key = after.find('}').and_then(|close| parse_key(&after[..close]).map(|k| (k, close)));
        match key {
            Some((key, close)) => {
                out.push(Piece::Text(&template[text_start..open]));
                out.push(Piece::Key(key));
                i = open + close + 2;
                text_start = i;
            }
            None => i = open + 1,
        }
    }
    out.push(Piece::Text(&template[text_start..]));
    out
}

fn placeholders(template: &str) -> Vec<Key> {
    pieces(template).into_iter().filter_map(|p| match p { Piece::Key(k) => Some(k), Piece::Text(_) => None }).collect()
}

fn parse_key(inner: &str) -> Option<Key> {
    let (name, part) = match inner.split_once('.') {
        None => (inner, None),
        Some((name, "user")) => (name, Some(Part::User)),
        Some((name, "host")) => (name, Some(Part::Host)),
        Some(_) => return None,
    };
    let ok = !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    ok.then(|| Key { name: name.to_string(), part })
}

/// Fill every placeholder, or `None` when any setting is unset.
fn fill(template: &str, mut get: impl FnMut(&Key) -> Option<String>, escape: fn(&str) -> String) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    for piece in pieces(template) {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Key(key) => out.push_str(&escape(&get(&key)?)),
        }
    }
    Some(out)
}

/// `name@host` or `@name@host`, the shape of an `acct:` address.
pub(crate) fn split_acct(value: &str) -> Option<(&str, &str)> {
    let (user, host) = value.strip_prefix('@').unwrap_or(value).split_once('@')?;
    let ok = |s: &str| !s.is_empty() && !s.contains(['@', '/', ' ', '"', '<', '>']);
    (ok(user) && ok(host)).then_some((user, host))
}

pub(crate) fn well_known_path_ok(path: &str) -> Result<(), String> {
    if WELL_KNOWN_FILES.contains(&path) { Ok(()) } else { Err(format!("a proof writes only {}", WELL_KNOWN_FILES.join(", "))) }
}

/// A copy record's page link: only [`PAGE_LINK_REL`], with an `at:` address
/// that fits a setting's cap.
pub(crate) fn page_link_ok(rel: &str, href: &str) -> Result<(), String> {
    if rel != PAGE_LINK_REL {
        return Err(format!("a page link writes only rel {PAGE_LINK_REL}"));
    }
    if !href.starts_with(PAGE_LINK_HREF_PREFIX) || href.len() > MAX_VALUE || href.contains(char::is_whitespace) {
        return Err(format!("a {PAGE_LINK_REL} link is an {PAGE_LINK_HREF_PREFIX} address of at most {MAX_VALUE} bytes"));
    }
    Ok(())
}

/// A copy record's `.well-known` file: an allowlisted path and a body under the cap.
pub(crate) fn site_file_ok(path: &str, body: &str) -> Result<(), String> {
    well_known_path_ok(path)?;
    if body.len() > MAX_BODY {
        return Err(format!("a body is at most {MAX_BODY} bytes"));
    }
    Ok(())
}

pub(crate) fn page_link_tag(href: &str) -> String {
    format!("<link rel=\"{PAGE_LINK_REL}\" href=\"{}\">", escape_html_attr(href))
}

fn is_json(path: &str) -> bool {
    path.ends_with(".json")
}

fn plain(value: &str) -> String {
    value.to_string()
}

fn json_escape(value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_default();
    quoted[1..quoted.len() - 1].to_string()
}

#[cfg(test)]
mod tests {
    use crate::plugins::types::PluginManifest;
    use serde_json::json;

    fn parse(settings: serde_json::Value, identity: serde_json::Value) -> Result<PluginManifest, String> {
        let manifest = json!({"name":"net","version":"1.0.0","contributes":{
            "site_settings":{"settings":settings},"identity":identity}});
        PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())
    }

    fn text(key: &str) -> serde_json::Value {
        json!([{"key":key,"type":"string","label":"L"}])
    }

    #[test]
    fn a_well_formed_proof_of_each_kind_parses() {
        let acct = json!([{"key":"account","type":"acct","label":"Account"},{"key":"did","type":"string","label":"DID"}]);
        let proofs = json!([
            {"file":".well-known/atproto-did","body":"{did}"},
            {"link":"me","href":"https://{account.host}/@{account.user}"},
            {"meta":"fediverse:creator","content":"{account}"}]);
        assert!(parse(acct, proofs).is_ok());
    }

    #[test]
    fn a_link_rel_or_meta_name_off_the_allowlist_is_refused() {
        let err = parse(text("u"), json!([{"link":"canonical","href":"https://{u}"}])).unwrap_err();
        assert!(err.contains("canonical"), "{err}");
        let err = parse(text("u"), json!([{"meta":"robots","content":"{u}"}])).unwrap_err();
        assert!(err.contains("robots"), "{err}");
    }

    #[test]
    fn a_file_other_than_a_named_proof_is_refused() {
        for path in ["index.html", ".well-known/../index.html", ".well-known/X", ".well-known/webauthn", ".well-known/assetlinks.json"] {
            assert!(parse(text("d"), json!([{"file":path,"body":"{d}"}])).is_err(), "{path}");
        }
    }

    #[test]
    fn a_proof_may_read_only_declared_text_settings_and_parts_only_of_an_acct() {
        let err = parse(text("did"), json!([{"file":".well-known/atproto-did","body":"{other}"}])).unwrap_err();
        assert!(err.contains("`other`"), "{err}");
        let err = parse(text("account"), json!([{"meta":"fediverse:creator","content":"{account.user}"}])).unwrap_err();
        assert!(err.contains("acct"), "{err}");
        let err = parse(text("d"), json!([{"file":".well-known/atproto-did","body":"no setting"}])).unwrap_err();
        assert!(err.contains("at least one setting"), "{err}");
    }

    #[test]
    fn a_json_body_must_stay_json_once_filled() {
        assert!(parse(text("k"), json!([{"file":".well-known/nostr.json","body":"{\"names\":{\"_\":\"{k}\"}}"}])).is_ok());
        assert!(parse(text("k"), json!([{"file":".well-known/nostr.json","body":"{\"names\": {k}}"}])).is_err());
    }

    #[test]
    fn an_acct_setting_accepts_only_name_at_host() {
        assert_eq!(super::split_acct("@ana@social.example"), Some(("ana", "social.example")));
        assert_eq!(super::split_acct("ana@social.example"), Some(("ana", "social.example")));
        for bad in ["ana", "@ana", "ana@", "a@b@c", "ana@host/x"] {
            assert_eq!(super::split_acct(bad), None, "{bad}");
        }
    }

    fn did_plugin() -> crate::plugins::types::Plugin {
        let settings = json!([{"key":"did","type":"string","label":"DID","pattern":"^did:[a-z]+:[A-Za-z0-9._:%-]+$","pattern_message":"A DID looks like did:plc:abc"}]);
        let proofs = json!([{"file":".well-known/atproto-did","body":"{did}"}]);
        crate::plugins::types::Plugin { manifest: parse(settings, proofs).unwrap(), path: std::path::PathBuf::new() }
    }

    fn supplied_with(did: &str) -> (Vec<super::Supplied>, Vec<String>) {
        let config = crate::config::ConfigFile::parse(&format!("[services.net]\ndid = \"{did}\"\n")).unwrap();
        super::supplied(&[did_plugin()], &config, "")
    }

    #[test]
    fn a_value_off_its_settings_pattern_writes_no_proof_and_says_why() {
        let (proofs, problems) = supplied_with("not a did");
        assert!(proofs.is_empty(), "{proofs:?}");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`did`"), "{problems:?}");

        let (proofs, problems) = supplied_with("did:plc:abc123");
        assert_eq!((proofs.len(), problems.len()), (1, 0));
    }
}
