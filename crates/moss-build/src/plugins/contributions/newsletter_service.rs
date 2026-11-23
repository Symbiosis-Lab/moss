//! `contributes.newsletter_service`: a mailing service a site's subscribers
//! can be copied to, connected by a key the person makes in that service and
//! pastes into moss. moss draws the steps and checks the key's shape before
//! anything is sent; the service's server side does the rest.
//!
//! The plugin's name is what `[channels.email] service` names.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct NewsletterServiceContribution {
    /// Name shown to the user; defaults to the plugin's display name.
    #[serde(default)]
    pub display_name: Option<String>,
    pub key: ServiceKey,
    /// What the person does in the service's own window, in its own words. The
    /// first step is the link to `key.url`. `{name}` stands for `key.name`.
    pub steps: Vec<String>,
    /// The service's page where its webhooks are listed, for a plan that has
    /// no webhook left.
    #[serde(default)]
    pub webhooks_url: Option<String>,
    /// The service's page where a sender address is added.
    #[serde(default)]
    pub senders_url: Option<String>,
}

/// The key the person makes, and how moss knows one when it is pasted.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ServiceKey {
    /// The service's page where keys are made.
    pub url: String,
    /// Every key of this service starts with it.
    pub prefix: String,
    /// Every key of this service is this many characters long.
    pub length: u32,
    /// The name the steps give the key; the service may delete a pasted key
    /// only when it carries this name.
    pub name: String,
    /// The service's word for the access the key needs, such as "Full Access".
    pub permission: String,
}

impl NewsletterServiceContribution {
    pub(crate) fn validate(&mut self) -> Result<(), String> {
        use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
        let https = |field: &str, address: &str| match url::Url::parse(address) {
            Ok(u) if u.scheme() == "https" && u.has_host() => Ok(()),
            _ => Err(format!("newsletter_service: {field} '{address}' is not an https address")),
        };
        https("key.url", &self.key.url)?;
        for (field, address) in [("webhooks_url", &self.webhooks_url), ("senders_url", &self.senders_url)] {
            address.as_deref().map_or(Ok(()), |a| https(field, a))?;
        }
        let key = &mut self.key;
        if key.prefix.is_empty() || key.prefix.len() > 10 || !key.prefix.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("newsletter_service: key.prefix is 1 to 10 printable ASCII characters".into());
        }
        if !(key.prefix.len() as u32 + 1..=512).contains(&key.length) {
            return Err("newsletter_service: key.length is longer than the prefix and at most 512".into());
        }
        let name_ok = !key.name.is_empty() && key.name.len() <= 40 && key.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !name_ok {
            return Err("newsletter_service: key.name is lowercase letters, digits and hyphens".into());
        }
        if !(1..=5).contains(&self.steps.len()) {
            return Err("newsletter_service: steps holds 1 to 5 steps".into());
        }
        key.permission = bounded(&key.permission, MAX_NAME);
        if let Some(name) = &mut self.display_name {
            *name = bounded(name, MAX_NAME);
        }
        for step in &mut self.steps {
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
        json!({"name":"sendgrid","version":"1.0.0","contributes":{"newsletter_service":{
            "display_name":"SendGrid",
            "key":{"url":"https://app.sendgrid.com/settings/api_keys","prefix":"SG.","length":69,"name":"moss","permission":"Full Access"},
            "steps":["Open API Keys in SendGrid","Create API Key, name it {name}, keep Full Access, then Create & View","Copy the key and paste it here"]}}})
    }

    fn parse(manifest: Value) -> Result<PluginManifest, String> {
        PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())
    }

    #[test]
    fn a_service_needs_no_script() {
        let m = parse(sendgrid()).unwrap();
        let service = m.contributes.unwrap().newsletter_service.unwrap();
        assert_eq!((service.key.prefix.as_str(), service.key.length), ("SG.", 69));
    }

    #[test]
    fn a_key_moss_could_not_check_or_a_page_off_https_is_refused() {
        for (path, value) in [
            ("url", json!("http://app.sendgrid.com/keys")),
            ("prefix", json!("")),
            ("length", json!(3)),
            ("name", json!("Moss Key")),
        ] {
            let mut m = sendgrid();
            m["contributes"]["newsletter_service"]["key"][path] = value;
            let err = parse(m).unwrap_err();
            assert!(err.contains("newsletter_service"), "{path}: {err}");
        }
        let mut m = sendgrid();
        m["contributes"]["newsletter_service"]["steps"] = json!([]);
        assert!(parse(m).is_err());
        let mut m = sendgrid();
        m["contributes"]["newsletter_service"]["senders_url"] = json!("javascript:alert(1)");
        assert!(parse(m).unwrap_err().contains("senders_url"));
    }
}
