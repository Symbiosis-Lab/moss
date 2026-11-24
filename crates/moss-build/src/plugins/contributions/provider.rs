//! `contributes.provider`: a third-party provider of one of a site's parts.
//! The envelope (the part, the outside addresses, the credentials) is the same
//! for every part; each part's own fields are its body. Only the newsletter
//! part exists so far: a mailing service the site's subscribers are copied to,
//! connected by a key the person makes in that service. moss draws the steps
//! and checks the key's shape; the service's server side does the rest.
//!
//! The plugin's name is what `[channels.email] provider` names.

use serde::{Deserialize, Serialize};

/// The site's `[channels.email] provider`, refused unless a plugin of that
/// name in the site's folder is a newsletter service.
pub fn for_build(config: Option<&crate::config::ConfigFile>, site: &str) -> Option<Result<String, String>> {
    config?.newsletter_service().map(|service| service.and_then(|name| provided_in(site, &name)))
}

fn provided_in(site: &str, name: &str) -> Result<String, String> {
    let Some(manifest) = crate::plugins::bundled::read_installed_manifest(&crate::plugins::plugin_dir(site, name)) else {
        return Err(format!("[channels.email] provider is \"{name}\", which is not installed in this site: run `moss plugin install {name}`"));
    };
    match manifest.contributes.and_then(|c| c.provider) {
        Some(p) if p.part == Part::Newsletter => Ok(name.to_string()),
        _ => Err(format!("[channels.email] provider is \"{name}\", and the plugin of that name in this site is not a newsletter service")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ProviderContribution {
    pub part: Part,
    /// Every outside address the site's pages or moss reach for this part.
    /// A page the person opens in the service is a link, not an address here.
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub credentials: Vec<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newsletter: Option<NewsletterPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Newsletter,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct NewsletterPart {
    /// What the person does in the service's own window, in its own words. The
    /// first step is the link to the key's `url`. `{name}` stands for
    /// [`KEY_NAME`] and `{permission}` for the key's `permission`.
    pub steps: Vec<String>,
    /// The service's page where its webhooks are listed, for a plan that has
    /// no webhook left.
    #[serde(default)]
    pub webhooks_url: Option<String>,
    /// The service's page where a sender address is added.
    #[serde(default)]
    pub senders_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Key,
}

/// A key the person makes in the service, and how moss knows one when it is
/// pasted or read from `env`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub kind: CredentialKind,
    /// The environment variable that may hold the key, read only by a command
    /// the person runs for this part, never by a build or a preview.
    #[serde(default)]
    pub env: Option<String>,
    /// The service's page where keys are made.
    pub url: String,
    /// Every key of this service starts with it.
    pub prefix: String,
    /// Every key of this service is this many characters long.
    pub length: u32,
    /// Always [`KEY_NAME`]; a manifest cannot set it.
    #[serde(skip_deserializing, default = "key_name")]
    pub name: String,
    /// The service's word for the access the key needs, such as "Full Access".
    pub permission: String,
}

/// The name moss's steps give a pasted key. moss's server deletes a pasted key
/// only when it carries this name, since a key named otherwise may serve
/// another app.
pub const KEY_NAME: &str = "moss";

fn key_name() -> String {
    KEY_NAME.to_string()
}

impl ProviderContribution {
    pub(crate) fn validate(&mut self) -> Result<(), String> {
        use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
        let https = |field: &str, address: &str| match url::Url::parse(address) {
            Ok(u) if u.scheme() == "https" && u.has_host() => Ok(()),
            _ => Err(format!("provider: {field} '{address}' is not an https address")),
        };
        for address in &self.addresses {
            https("addresses", address)?;
        }
        let Part::Newsletter = self.part;
        let (Some(body), [key]) = (self.newsletter.as_mut(), &mut self.credentials[..]) else {
            return Err("provider: a newsletter provider has a `newsletter` body and exactly one credential".into());
        };
        https("credentials.url", &key.url)?;
        for (field, address) in [("newsletter.webhooks_url", &body.webhooks_url), ("newsletter.senders_url", &body.senders_url)] {
            address.as_deref().map_or(Ok(()), |a| https(field, a))?;
        }
        if key.prefix.is_empty() || key.prefix.len() > 10 || !key.prefix.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("provider: credentials.prefix is 1 to 10 printable ASCII characters".into());
        }
        if !(key.prefix.len() as u32 + 1..=512).contains(&key.length) {
            return Err("provider: credentials.length is longer than the prefix and at most 512".into());
        }
        let variable = |v: &str| v.len() <= 64 && v.starts_with(|c: char| c.is_ascii_uppercase())
            && v.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if key.env.as_deref().is_some_and(|v| !variable(v)) {
            return Err("provider: credentials.env is an environment variable name such as SERVICE_API_KEY".into());
        }
        if !(1..=5).contains(&body.steps.len()) {
            return Err("provider: newsletter.steps holds 1 to 5 steps".into());
        }
        key.permission = bounded(&key.permission, MAX_NAME);
        for step in &mut body.steps {
            *step = bounded(step, MAX_SENTENCE);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::plugins::types::PluginManifest;
    use serde_json::{json, Value};

    fn sendgrid() -> Value {
        json!({"name":"sendgrid","version":"1.0.0","contributes":{"provider":{"part":"newsletter","addresses":[],
            "credentials":[{"kind":"key","env":"SENDGRID_API_KEY","url":"https://app.sendgrid.com/settings/api_keys","prefix":"SG.","length":69,"permission":"Full Access"}],
            "newsletter":{"steps":["Open API Keys in SendGrid","Create API Key, name it {name}, keep {permission}, then Create & View","Copy the key and paste it here"]}}}})
    }

    fn parse(manifest: Value) -> Result<PluginManifest, String> {
        PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())
    }

    fn provider(manifest: Value) -> super::ProviderContribution {
        parse(manifest).unwrap().contributes.unwrap().provider.unwrap()
    }

    #[test]
    fn a_provider_that_is_not_installed_names_the_install_command() {
        let site = tempfile::tempdir().unwrap();
        let err = super::provided_in(site.path().to_str().unwrap(), "sendgrid").unwrap_err();
        assert!(err.contains("`moss plugin install sendgrid`"), "{err}");
    }

    #[test]
    fn a_newsletter_provider_needs_no_script() {
        let p = provider(sendgrid());
        assert_eq!(p.part, super::Part::Newsletter);
        let key = &p.credentials[0];
        assert_eq!((key.prefix.as_str(), key.length, key.env.as_deref()), ("SG.", 69, Some("SENDGRID_API_KEY")));
    }

    #[test]
    fn the_key_name_is_moss_s_whatever_the_manifest_says() {
        let mut m = sendgrid();
        m["contributes"]["provider"]["credentials"][0]["name"] = json!("other");
        assert!(parse(m).unwrap_err().contains("name"), "an unknown credential field is refused");
        assert_eq!(provider(sendgrid()).credentials[0].name, super::KEY_NAME);
    }

    #[test]
    fn an_envelope_moss_could_not_check_is_refused() {
        let at = |path: &[&str], value: Value| {
            let mut m = sendgrid();
            let mut v = &mut m["contributes"]["provider"];
            for p in path {
                v = if let Ok(i) = p.parse::<usize>() { &mut v[i] } else { &mut v[*p] };
            }
            *v = value;
            parse(m).unwrap_err()
        };
        for (path, value, said) in [
            (&["credentials", "0", "url"][..], json!("http://app.sendgrid.com/keys"), "credentials.url"),
            (&["credentials", "0", "prefix"], json!(""), "prefix"),
            (&["credentials", "0", "length"], json!(3), "length"),
            (&["credentials", "0", "env"], json!("sendgrid key"), "credentials.env"),
            (&["credentials"], json!([]), "exactly one credential"),
            (&["newsletter", "steps"], json!([]), "steps"),
            (&["newsletter", "senders_url"], json!("javascript:alert(1)"), "senders_url"),
            (&["addresses"], json!(["http://api.example"]), "addresses"),
            (&["part"], json!("comments"), "unknown variant `comments`"),
            (&["sends"], json!("subscribers"), "sends"),
        ] {
            let err = at(path, value);
            assert!(err.contains(said), "{path:?}: {err}");
        }
        let mut m = sendgrid();
        m["contributes"]["provider"].as_object_mut().unwrap().remove("newsletter");
        assert!(parse(m).unwrap_err().contains("`newsletter` body"));
    }
}
