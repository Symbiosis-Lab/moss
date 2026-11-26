//! A plugin that holds an account at one company: `contributes.account` says
//! how the person signs in to it, once, and `contributes.provider` lists the
//! parts of the site that one sign-in serves, each with what it needs from the
//! account and what its actions cost. Each part's own fields are its body. A
//! plugin's `deploy_target` and `channel` read as its host and channels
//! entries ([`parts`]).
//!
//! The plugin's name is what a part's config line names, such as
//! `[channels.email] provider`.

use serde::{Deserialize, Serialize};

use crate::plugins::types::PluginManifest;

/// The site's `[channels.email] provider`, refused unless a plugin of that
/// name in the site's folder is a newsletter service.
pub fn for_build(config: Option<&crate::config::ConfigFile>, site: &str) -> Option<Result<String, String>> {
    config?.newsletter_service().map(|service| service.and_then(|name| newsletter_provider(site, &name).map(|_| name)))
}

/// A newsletter service: the key its account signs in with, what the key
/// needs, and the part's own fields.
#[derive(Debug, Clone)]
pub struct Newsletter {
    pub key: Credential,
    pub needs: String,
    pub body: NewsletterPart,
}

/// The newsletter provider the plugin `name` in the site's folder contributes.
pub fn newsletter_provider(site: &str, name: &str) -> Result<Newsletter, String> {
    let Some(manifest) = crate::plugins::bundled::read_installed_manifest(&crate::plugins::plugin_dir(site, name)) else {
        return Err(format!("[channels.email] provider is \"{name}\", which is not installed in this site: run `moss plugin install {name}`"));
    };
    let contributes = manifest.contributes.unwrap_or_default();
    let entry = contributes.provider.into_iter().flatten().find(|e| e.part == Part::Newsletter);
    match (contributes.account, entry) {
        (Some(SignIn::Key(key)), Some(ProviderEntry { needs: Some(needs), newsletter: Some(body), .. })) => Ok(Newsletter { key, needs, body }),
        _ => Err(format!("[channels.email] provider is \"{name}\", and the plugin of that name in this site is not a newsletter service")),
    }
}

/// The script tag of the analytics provider the plugin `name` in the site's
/// folder contributes, carrying the site's `site_id`.
pub fn analytics_tag(site: &str, name: &str, site_id: &str) -> Result<String, String> {
    let Some(manifest) = crate::plugins::bundled::read_installed_manifest(&crate::plugins::plugin_dir(site, name)) else {
        return Err(format!("[services.analytics] provider is \"{name}\", which is not installed in this site: run `moss plugin install {name}`"));
    };
    let entry = manifest.contributes.unwrap_or_default().provider.into_iter().flatten().find(|e| e.part == Part::Analytics);
    let Some(body) = entry.and_then(|e| e.analytics) else {
        return Err(format!("[services.analytics] provider is \"{name}\", and the plugin of that name in this site is not an analytics service"));
    };
    Ok(body.tag(site_id))
}

/// How the person signs in to the plugin's account. One per plugin, whatever
/// parts it serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SignIn {
    /// A key or token the person makes in the service and pastes.
    Key(Credential),
    /// A session the person signs in to on the service's own page, read from
    /// a channel's `login` address; a manifest cannot declare one.
    Session { address: String, domains: Vec<String> },
    /// The plugin exports `login`, and moss runs it.
    Code,
    /// Nothing to sign in to.
    None,
}

/// One part of the site the account serves.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    pub part: Part,
    /// Every outside address the site's pages or moss reach for this part.
    /// A page the person opens in the service is a link, not an address here.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// What this part needs from the account, in the service's own word (a
    /// permission or a scope). The plugin maps the refusal of any call of the
    /// part to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub costs: Vec<Cost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newsletter: Option<NewsletterPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<AddressPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytics: Option<AnalyticsPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Newsletter,
    /// The DNS of the site's domain.
    Address,
    Analytics,
    /// Read from `deploy_target`; not declared in `provider`.
    Host,
    /// Read from `channel`; not declared in `provider`.
    Channels,
}

impl Part {
    /// The name a manifest writes.
    pub fn name(&self) -> &'static str {
        match self {
            Part::Newsletter => "newsletter",
            Part::Address => "address",
            Part::Analytics => "analytics",
            Part::Host => "host",
            Part::Channels => "channels",
        }
    }
}

/// What one action of a part charges, before moss runs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    /// The plugin's name for the action, such as `post` or `publish`.
    pub action: String,
    /// The shape of the call the price follows, such as `with_link`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// A decimal number, such as `"0.20"`: never a float, which would round.
    pub amount: String,
    /// An ISO 4217 currency code, or `credits` for the provider's own.
    pub unit: String,
    /// The provider's page that states the price.
    pub price_url: String,
}

/// The account's sign-in, from `contributes.account` or, for a channel that
/// declares none, from its `login`: an address is a session on the plugin's
/// domains, an exported `login` is the plugin's code.
pub fn sign_in(manifest: &PluginManifest) -> Option<SignIn> {
    use super::Login;
    let contributes = manifest.contributes.as_ref()?;
    if let Some(account) = &contributes.account {
        return Some(account.clone());
    }
    match contributes.channel.as_ref()?.login.as_ref()? {
        Login::Export => Some(SignIn::Code),
        Login::Address(address) => Some(SignIn::Session {
            address: address.clone(),
            domains: manifest.domains.clone().or_else(|| manifest.domain.clone().map(|d| vec![d])).unwrap_or_default(),
        }),
    }
}

/// Every part the plugin's one account serves: its `provider` entries, then
/// a host entry for `deploy_target` and a channels entry for `channel`.
pub fn parts(manifest: &PluginManifest) -> Vec<ProviderEntry> {
    let Some(contributes) = manifest.contributes.as_ref() else { return Vec::new() };
    let read = |part| ProviderEntry { part, addresses: Vec::new(), needs: None, costs: Vec::new(), newsletter: None, address: None, analytics: None };
    let mut parts = contributes.provider.clone().unwrap_or_default();
    parts.extend(contributes.deploy_target.as_ref().map(|_| read(Part::Host)));
    parts.extend(contributes.channel.as_ref().map(|_| read(Part::Channels)));
    parts
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct NewsletterPart {
    /// What the person does in the service's own window, in its own words. The
    /// first step is the link to the key's `url`. `{name}` stands for
    /// [`KEY_NAME`] and `{permission}` for the part's `needs`.
    pub steps: Vec<String>,
    /// The service's page where its webhooks are listed, for a plan that has
    /// no webhook left.
    #[serde(default)]
    pub webhooks_url: Option<String>,
    /// The service's page where a sender address is added.
    #[serde(default)]
    pub senders_url: Option<String>,
}

/// The DNS host of the domains whose name servers it answers for. The plugin's
/// code exports the zone's verbs: `dns_zones`, `dns_records`, `dns_set` and
/// `dns_delete`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct AddressPart {
    /// Name-server suffixes such as `ns.cloudflare.com`. They only decide
    /// which signed-in accounts moss asks first; a zone is the account's when
    /// the account lists it.
    pub nameservers: Vec<String>,
}

/// An analytics service the pages reach through its script. The plugin's code
/// exports `analytics_read`, which reads the visits back into the folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsPart {
    /// The script every page loads.
    pub script: String,
    /// The script tag's attributes, where `{site_id}` stands for the site's
    /// `[services.analytics] site_id`.
    #[serde(default)]
    pub attributes: std::collections::BTreeMap<String, String>,
    /// What `analytics_read` returns.
    pub reads: Reads,
}

impl AnalyticsPart {
    fn tag(&self, site_id: &str) -> String {
        use moss_core::media::html_escape_attr as attr;
        let attributes: String = self.attributes.iter().map(|(k, v)| format!(" {k}=\"{}\"", attr(&v.replace("{site_id}", site_id)))).collect();
        format!(r#"<script defer src="{}"{attributes}></script>"#, attr(&self.script))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Reads {
    /// One record per visit.
    Events,
    /// Visits per page per day.
    DailyCounts,
}

/// A key the person makes in the service, and how moss knows one when it is
/// pasted or read from `env`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    /// The environment variable that may hold the key, read only by a command
    /// the person runs for one of the account's parts, never by a build or a
    /// preview.
    #[serde(default)]
    pub env: Option<String>,
    /// The service's page where keys are made.
    pub url: String,
    /// Every key of this service starts with it; empty when the service's
    /// keys share no beginning.
    #[serde(default)]
    pub prefix: String,
    /// How many characters a key of this service has.
    pub length: Length,
    /// Always [`KEY_NAME`]; a manifest cannot set it.
    #[serde(skip_deserializing, default = "key_name")]
    pub name: String,
}

/// The plugin secret a key account's paste is kept under, for a plugin with
/// code: the plugin reads it with `getSecret("token")`.
pub const TOKEN: &str = "token";

/// The name moss's steps give a pasted key. moss's server deletes a pasted key
/// only when it carries this name, since a key named otherwise may serve
/// another app.
pub const KEY_NAME: &str = "moss";

fn key_name() -> String {
    KEY_NAME.to_string()
}

/// A number of characters: `69`, or `[40, 64]` from the fewest to the most
/// for a service whose keys come in several forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Length {
    Exactly(u32),
    Between([u32; 2]),
}

impl Length {
    fn range(self) -> std::ops::RangeInclusive<u32> {
        match self {
            Length::Exactly(n) => n..=n,
            Length::Between([min, max]) => min..=max,
        }
    }
}

impl std::fmt::Display for Length {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Length::Exactly(n) => write!(f, "{n}"),
            Length::Between([min, max]) => write!(f, "{min} to {max}"),
        }
    }
}

impl Credential {
    /// The key in `paste`, without the spaces and line breaks a copy carries,
    /// when it has this service's prefix and length.
    pub fn fits(&self, paste: &str) -> Option<String> {
        let key: String = paste.split_whitespace().collect();
        (key.starts_with(&self.prefix) && self.length.range().contains(&(key.chars().count() as u32))).then_some(key)
    }

    /// What a key of this service looks like, for a paste that is not one.
    pub fn shape(&self) -> String {
        match self.prefix.as_str() {
            "" => format!("it is {} characters", self.length),
            prefix => format!("it starts with {prefix} and is {} characters", self.length),
        }
    }
}

fn https(field: &str, address: &str) -> Result<(), String> {
    match url::Url::parse(address) {
        Ok(u) if u.scheme() == "https" && u.has_host() && address.len() <= MAX_URL_BYTES => Ok(()),
        _ => Err(format!("{field} '{address}' is not an https address")),
    }
}

/// Most entries a manifest list may hold.
const MAX_ITEMS: usize = 16;
const MAX_URL_BYTES: usize = 2048;

/// `amount` is a decimal such as `0.20`, with at most six places, in an ISO
/// 4217 currency or `credits`.
fn amount(amount: &str, unit: &str) -> Result<(), String> {
    let (whole, frac) = amount.split_once('.').unwrap_or((amount, ""));
    let digits = |s: &str, max: usize| s.len() <= max && s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole, 9) || !digits(frac, 6) || amount.ends_with('.') {
        return Err(format!("'{amount}' is a decimal number such as \"0.20\""));
    }
    if !(unit == "credits" || (unit.len() == 3 && unit.bytes().all(|b| b.is_ascii_uppercase()))) {
        return Err(format!("'{unit}' is a currency code such as USD, or credits"));
    }
    Ok(())
}

impl SignIn {
    /// The `kind` a manifest writes.
    pub fn kind(&self) -> &'static str {
        match self {
            SignIn::Key(_) => "key",
            SignIn::Session { .. } => "session",
            SignIn::Code => "code",
            SignIn::None => "none",
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let refuse = |why: &str| Err(format!("account: {why}"));
        match self {
            SignIn::Key(key) => {
                if key.prefix.len() > 10 || !key.prefix.bytes().all(|b| b.is_ascii_graphic()) {
                    return refuse("prefix is at most 10 printable ASCII characters");
                }
                let lengths = key.length.range();
                if lengths.is_empty() || *lengths.start() <= key.prefix.len() as u32 || *lengths.end() > 512 {
                    return refuse("length is longer than the prefix and at most 512, and a range runs from the fewest to the most");
                }
                let variable = |v: &str| v.len() <= 64 && v.starts_with(|c: char| c.is_ascii_uppercase())
                    && v.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
                if !key.env.as_deref().is_none_or(variable) {
                    return refuse("env is an environment variable name such as SERVICE_API_KEY");
                }
                https("account: url", &key.url)
            }
            SignIn::Session { .. } | SignIn::Code => refuse(&format!("a {} sign-in is a channel's `login`, not an `account`", self.kind())),
            SignIn::None => Ok(()),
        }
    }
}

impl Cost {
    fn validate(&mut self) -> Result<(), String> {
        use moss_core::untrusted_text::{bounded, MAX_NAME};
        https("provider: costs.price_url", &self.price_url)?;
        self.action = bounded(&self.action, MAX_NAME);
        self.variant = self.variant.as_deref().map(|v| bounded(v, MAX_NAME));
        amount(&self.amount, &self.unit).map_err(|why| format!("provider: costs: {why}"))
    }
}

/// Checks `contributes.account` and `contributes.provider` together, and
/// clamps the text moss shows.
pub(crate) fn validate(account: Option<&SignIn>, entries: &mut [ProviderEntry], has_channel: bool, has_entry: bool) -> Result<(), String> {
    use moss_core::untrusted_text::{bounded, MAX_NAME, MAX_SENTENCE};
    if let Some(account) = account {
        if has_channel {
            return Err("account: a channel plugin signs in through its channel's `login`, so it declares no `account`".into());
        }
        account.validate()?;
    } else if !entries.is_empty() {
        return Err("provider: a provider declares its `account`, or `{\"kind\": \"none\"}` when there is nothing to sign in to".into());
    }
    for (i, entry) in entries.iter().enumerate() {
        if entries[..i].iter().any(|e| e.part == entry.part) {
            return Err("provider: a part is listed twice".into());
        }
    }
    for entry in entries.iter_mut() {
        if entry.addresses.len() > MAX_ITEMS || entry.addresses.iter().any(|a| a.len() > MAX_URL_BYTES) {
            return Err(format!("provider: addresses holds at most {MAX_ITEMS} entries of at most {MAX_URL_BYTES} bytes"));
        }
        for address in &entry.addresses {
            https("provider: addresses", address)?;
        }
        if entry.costs.len() > MAX_ITEMS {
            return Err(format!("provider: costs holds at most {MAX_ITEMS} entries"));
        }
        for cost in &mut entry.costs {
            cost.validate()?;
        }
        entry.needs = entry.needs.as_deref().map(|n| bounded(n, MAX_NAME));
        let other_bodies = match entry.part {
            Part::Newsletter => entry.address.is_some() || entry.analytics.is_some(),
            Part::Address => entry.newsletter.is_some() || entry.analytics.is_some(),
            Part::Analytics => entry.newsletter.is_some() || entry.address.is_some(),
            Part::Host | Part::Channels => false,
        };
        if other_bodies {
            return Err(format!("provider: the {} entry carries another part's body", entry.part.name()));
        }
        match entry.part {
            Part::Host | Part::Channels => {
                return Err("provider: a host is declared by `deploy_target` and channels by `channel`, which read as entries here".into());
            }
            Part::Newsletter => {
                let (Some(SignIn::Key(_)), Some(_), Some(body)) = (account, entry.needs.as_ref(), entry.newsletter.as_mut()) else {
                    return Err("provider: a newsletter provider signs in with a key and has `needs` and a `newsletter` body".into());
                };
                for (field, address) in [("newsletter.webhooks_url", &body.webhooks_url), ("newsletter.senders_url", &body.senders_url)] {
                    address.as_deref().map_or(Ok(()), |a| https(&format!("provider: {field}"), a))?;
                }
                if !(1..=5).contains(&body.steps.len()) {
                    return Err("provider: newsletter.steps holds 1 to 5 steps".into());
                }
                for step in &mut body.steps {
                    *step = bounded(step, MAX_SENTENCE);
                }
            }
            Part::Address => {
                let (true, Some(body)) = (has_entry, entry.address.as_ref()) else {
                    return Err("provider: an address provider has code (`entry`), which runs the zone's verbs, and an `address` body".into());
                };
                let suffix = |n: &String| !n.is_empty() && n.len() <= 253 && !n.starts_with('.') && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
                if !(1..=MAX_ITEMS).contains(&body.nameservers.len()) || !body.nameservers.iter().all(suffix) {
                    return Err(format!("provider: address.nameservers holds 1 to {MAX_ITEMS} lowercase name-server suffixes such as ns.example.com"));
                }
            }
            Part::Analytics => {
                let (true, Some(body)) = (has_entry, entry.analytics.as_mut()) else {
                    return Err("provider: an analytics provider has code (`entry`), which reads the visits back, and an `analytics` body".into());
                };
                https("provider: analytics.script", &body.script)?;
                if !entry.addresses.contains(&body.script) {
                    return Err("provider: analytics.script is one of the part's `addresses`".into());
                }
                let name = |k: &String| k.starts_with("data-") && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
                if body.attributes.len() > MAX_ITEMS || !body.attributes.keys().all(name) {
                    return Err(format!("provider: analytics.attributes holds at most {MAX_ITEMS} `data-` attributes"));
                }
                for value in body.attributes.values_mut() {
                    *value = bounded(value, MAX_SENTENCE);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Part, SignIn};
    use crate::plugins::types::PluginManifest;
    use serde_json::{json, Value};

    fn sendgrid() -> Value {
        json!({"name":"sendgrid","version":"1.0.0","contributes":{
            "account":{"kind":"key","env":"SENDGRID_API_KEY","url":"https://app.sendgrid.com/settings/api_keys","prefix":"SG.","length":69},
            "provider":[{"part":"newsletter","needs":"Full Access",
                "newsletter":{"steps":["Open API Keys in SendGrid","Create API Key, name it {name}, keep {permission}, then Create & View","Copy the key and paste it here"]}}]}})
    }

    /// One account serving a host and a priced newsletter, from a channel-less
    /// plugin with code.
    fn one_account_two_parts(account: Value) -> Value {
        json!({"name":"cloud","version":"1.0.0","entry":"main.js","contributes":{
            "account": account,
            "deploy_target": {},
            "provider":[{"part":"newsletter","needs":"Mail send","newsletter":{"steps":["Paste it here"]},
                "costs":[{"action":"send","amount":"0.001","unit":"USD","price_url":"https://cloud.example/pricing"}]}]}})
    }

    fn parse(manifest: Value) -> Result<PluginManifest, String> {
        PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())
    }

    #[test]
    fn a_provider_that_is_not_installed_names_the_install_command() {
        let site = tempfile::tempdir().unwrap();
        let err = super::newsletter_provider(site.path().to_str().unwrap(), "sendgrid").unwrap_err();
        assert!(err.contains("`moss plugin install sendgrid`"), "{err}");
    }

    #[test]
    fn a_newsletter_provider_needs_no_script() {
        let m = parse(sendgrid()).unwrap();
        let Some(SignIn::Key(key)) = super::sign_in(&m) else { panic!("a key account") };
        assert_eq!((key.prefix.as_str(), key.length, key.env.as_deref()), ("SG.", super::Length::Exactly(69), Some("SENDGRID_API_KEY")));
        let parts = super::parts(&m);
        assert_eq!((parts.len(), parts[0].part, parts[0].needs.as_deref()), (1, Part::Newsletter, Some("Full Access")));
        assert!(!crate::plugins::contributions::runs_code(&m));
    }

    #[test]
    fn the_key_name_is_moss_s_whatever_the_manifest_says() {
        let mut m = sendgrid();
        m["contributes"]["account"]["name"] = json!("other");
        assert!(parse(m).unwrap_err().contains("name"), "an unknown account field is refused");
        let Some(SignIn::Key(key)) = super::sign_in(&parse(sendgrid()).unwrap()) else { panic!() };
        assert_eq!(key.name, super::KEY_NAME);
    }

    #[test]
    fn one_sign_in_serves_every_part_the_plugin_declares() {
        let m = parse(one_account_two_parts(json!({"kind":"key","url":"https://cloud.example/tokens","prefix":"ck_","length":40}))).unwrap();
        let parts: Vec<_> = super::parts(&m).into_iter().map(|p| p.part).collect();
        assert_eq!(parts, [Part::Newsletter, Part::Host]);
        for part in [Part::Newsletter, Part::Host, Part::Channels] {
            assert_eq!(json!(part.name()), serde_json::to_value(part).unwrap());
        }
        let cost = &super::parts(&m)[0].costs[0];
        assert_eq!((cost.amount.as_str(), cost.unit.as_str()), ("0.001", "USD"));
    }

    /// One token that publishes the site, holds its domain's DNS and reads its
    /// analytics.
    fn cloudflare() -> Value {
        json!({"name":"cloudflare","version":"1.0.0","entry":"main.bundle.js","contributes":{
            "account":{"kind":"key","env":"CLOUDFLARE_API_TOKEN","url":"https://dash.cloudflare.com/profile/api-tokens","length":[40,64]},
            "deploy_target":{},
            "provider":[
                {"part":"address","needs":"Zone · DNS · Edit","addresses":["https://api.cloudflare.com/client/v4"],
                    "address":{"nameservers":["ns.cloudflare.com"]}},
                {"part":"analytics","needs":"Account · Account Analytics · Read",
                    "addresses":["https://static.cloudflareinsights.com/beacon.min.js","https://api.cloudflare.com/client/v4"],
                    "analytics":{"script":"https://static.cloudflareinsights.com/beacon.min.js",
                        "attributes":{"data-cf-beacon":"{\"token\": \"{site_id}\"}"},"reads":"daily_counts"}}]}})
    }

    #[test]
    fn one_token_serves_the_host_the_address_and_analytics() {
        let m = parse(cloudflare()).unwrap();
        let parts = super::parts(&m);
        assert_eq!(parts.iter().map(|p| p.part).collect::<Vec<_>>(), [Part::Address, Part::Analytics, Part::Host]);
        assert_eq!(parts[0].address.as_ref().unwrap().nameservers, ["ns.cloudflare.com"]);
        let analytics = parts[1].analytics.as_ref().unwrap();
        assert_eq!((analytics.reads, analytics.attributes["data-cf-beacon"].as_str()), (super::Reads::DailyCounts, "{\"token\": \"{site_id}\"}"));
        for part in [Part::Address, Part::Analytics] {
            assert_eq!(json!(part.name()), serde_json::to_value(part).unwrap());
        }
    }

    #[test]
    fn a_key_account_with_code_is_kept_for_the_plugin_as_its_token() {
        let m = parse(cloudflare()).unwrap();
        let secrets: Vec<_> = m.declared_secrets(&Default::default()).into_iter().map(|f| (f.key.clone(), f.help_url.clone())).collect();
        assert_eq!(secrets, [(super::TOKEN.to_string(), Some("https://dash.cloudflare.com/profile/api-tokens".to_string()))]);
        assert!(parse(sendgrid()).unwrap().declared_secrets(&Default::default()).is_empty(), "seta holds a newsletter key");
    }

    #[test]
    fn the_analytics_tag_carries_the_site_s_id_escaped() {
        let site = tempfile::tempdir().unwrap();
        let site = site.path().to_str().unwrap();
        let dir = crate::plugins::plugin_dir(site, "cloudflare");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), cloudflare().to_string()).unwrap();
        assert_eq!(super::analytics_tag(site, "cloudflare", "ab\"12").unwrap(),
            r#"<script defer src="https://static.cloudflareinsights.com/beacon.min.js" data-cf-beacon="{&quot;token&quot;: &quot;ab&quot;12&quot;}"></script>"#);
        assert!(super::analytics_tag(site, "umami", "x").unwrap_err().contains("`moss plugin install umami`"));
    }

    #[test]
    fn a_key_with_no_prefix_and_several_forms_is_known_by_its_length() {
        let Some(SignIn::Key(key)) = super::sign_in(&parse(cloudflare()).unwrap()) else { panic!("a key account") };
        let legacy = "a".repeat(40);
        let scannable = format!("cfut_{}{}", "b".repeat(40), "c".repeat(8));
        assert_eq!(key.fits(&format!(" {legacy}\n")), Some(legacy));
        assert_eq!(key.fits(&scannable), Some(scannable));
        assert_eq!(key.fits(&"a".repeat(39)), None);
        assert_eq!(key.fits(&"a".repeat(65)), None);
        assert_eq!(key.shape(), "it is 40 to 64 characters");
        let Some(SignIn::Key(sendgrid)) = super::sign_in(&parse(sendgrid()).unwrap()) else { panic!() };
        assert_eq!(sendgrid.shape(), "it starts with SG. and is 69 characters");
    }

    #[test]
    fn an_address_or_analytics_entry_moss_could_not_run_is_refused() {
        let at = |path: &[&str], value: Value| {
            let mut m = cloudflare();
            let mut v = &mut m;
            for p in path {
                v = if let Ok(i) = p.parse::<usize>() { &mut v[i] } else { &mut v[*p] };
            }
            *v = value;
            parse(m).unwrap_err()
        };
        for (path, value, said) in [
            (&["entry"][..], Value::Null, "has code (`entry`), which runs the zone's verbs"),
            (&["contributes", "provider", "0", "address"], Value::Null, "an `address` body"),
            (&["contributes", "provider", "0", "address", "nameservers"], json!([]), "address.nameservers"),
            (&["contributes", "provider", "0", "address", "nameservers"], json!(["NS.Cloudflare.com"]), "address.nameservers"),
            (&["contributes", "provider", "0", "address", "nameservers"], json!([".cloudflare.com"]), "address.nameservers"),
            (&["contributes", "provider", "0", "newsletter"], json!({"steps":["s"]}), "carries another part's body"),
            (&["contributes", "provider", "1", "analytics"], Value::Null, "an `analytics` body"),
            (&["contributes", "provider", "1", "analytics", "script"], json!("http://static.cloudflareinsights.com/beacon.min.js"), "analytics.script"),
            (&["contributes", "provider", "1", "addresses"], json!(["https://api.cloudflare.com/client/v4"]), "one of the part's `addresses`"),
            (&["contributes", "provider", "1", "analytics", "attributes"], json!({"onload":"x"}), "`data-` attributes"),
            (&["contributes", "provider", "1", "analytics", "reads"], json!("visits"), "unknown variant `visits`"),
        ] {
            let err = at(path, value);
            assert!(err.contains(said), "{path:?}: {err}");
        }
    }

    #[test]
    fn a_channel_s_login_reads_as_its_account_s_sign_in() {
        let channel = |login: Value| json!({"name":"net","version":"1.0.0","entry":"main.js","domains":["net.example","staging.net.example"],
            "contributes":{"channel":{"login": login}}});
        let m = parse(channel(json!("https://net.example/login"))).unwrap();
        let session = super::sign_in(&m).unwrap();
        assert_eq!(session, SignIn::Session {
            address: "https://net.example/login".into(),
            domains: vec!["net.example".into(), "staging.net.example".into()],
        });
        assert_eq!(json!(session.kind()), serde_json::to_value(&session).unwrap()["kind"]);
        assert_eq!(super::sign_in(&parse(channel(json!(true))).unwrap()), Some(SignIn::Code));
        assert_eq!(super::sign_in(&parse(channel(json!(false))).unwrap()), None);
        assert_eq!(super::parts(&m)[0].part, Part::Channels);
    }

    #[test]
    fn every_sign_in_kind_parses_and_is_refused_incomplete() {
        let fine = [json!({"kind":"none"})];
        for account in fine {
            let m = json!({"name":"p","version":"1.0.0","entry":"main.js","contributes":{"account": account.clone(), "deploy_target": {}}});
            let sign_in = super::sign_in(&parse(m).unwrap()).unwrap();
            assert_eq!(json!(sign_in.kind()), account["kind"]);
            assert_eq!(serde_json::to_value(sign_in).unwrap(), account);
        }
        let refused = [
            (json!({"kind":"session","address":"https://net.example/login","domains":["net.example"]}), "a session sign-in is a channel's `login`"),
            (json!({"kind":"code"}), "a code sign-in is a channel's `login`"),
            (json!({"kind":"device","device_url":"https://id.example/d","token_url":"https://id.example/t","client_id":"m"}), "unknown variant `device`"),
            (json!({"kind":"password"}), "unknown variant `password`"),
        ];
        for (account, said) in refused {
            let m = json!({"name":"p","version":"1.0.0","entry":"main.js","contributes":{"account": account, "deploy_target": {}}});
            let err = parse(m).unwrap_err();
            assert!(err.contains(said), "{said}: {err}");
        }
    }

    #[test]
    fn a_channel_plugin_declares_no_account() {
        for channel in [json!({"login":true}), json!({})] {
            let m = json!({"name":"p","version":"1.0.0","entry":"main.js","contributes":{"account":{"kind":"none"},"channel":channel}});
            assert!(parse(m).unwrap_err().contains("declares no `account`"));
        }
    }

    #[test]
    fn an_envelope_moss_could_not_check_is_refused() {
        let at = |path: &[&str], value: Value| {
            let mut m = sendgrid();
            let mut v = &mut m["contributes"];
            for p in path {
                v = if let Ok(i) = p.parse::<usize>() { &mut v[i] } else { &mut v[*p] };
            }
            *v = value;
            parse(m).unwrap_err()
        };
        let newsletter = json!({"part":"newsletter","needs":"x","newsletter":{"steps":["s"]}});
        for (path, value, said) in [
            (&["account", "url"][..], json!("http://app.sendgrid.com/keys"), "account: url"),
            (&["account", "prefix"], json!("sendgrid-key-"), "prefix"),
            (&["account", "length"], json!([69, 40]), "length"),
            (&["account", "length"], json!(3), "length"),
            (&["account", "env"], json!("sendgrid key"), "env"),
            (&["account"], json!({"kind":"none"}), "signs in with a key"),
            (&["account"], Value::Null, "declares its `account`"),
            (&["provider", "0", "needs"], Value::Null, "has `needs`"),
            (&["provider", "0", "newsletter"], Value::Null, "`newsletter` body"),
            (&["provider", "0", "newsletter", "steps"], json!([]), "steps"),
            (&["provider", "0", "newsletter", "senders_url"], json!("javascript:alert(1)"), "senders_url"),
            (&["provider", "0", "addresses"], json!(["http://api.example"]), "addresses"),
            (&["provider", "0", "addresses"], json!(vec!["https://api.example"; 17]), "addresses holds at most"),
            (&["provider", "0", "part"], json!("comments"), "unknown variant `comments`"),
            (&["provider", "0", "part"], json!("host"), "`deploy_target`"),
            (&["provider", "0", "sends"], json!("subscribers"), "sends"),
            (&["provider"], json!([newsletter.clone(), newsletter]), "listed twice"),
        ] {
            let err = at(path, value);
            assert!(err.contains(said), "{path:?}: {err}");
        }
        let mut m = sendgrid();
        m["contributes"]["provider"][0]["addresses"] = json!([format!("https://api.example/{}", "p".repeat(300))]);
        assert!(parse(m).is_ok(), "an address may be longer than a domain name");
    }

    #[test]
    fn a_cost_is_a_decimal_in_a_currency_or_credits_with_its_price_page() {
        let mut long = one_account_two_parts(json!({"kind":"key","url":"https://cloud.example/t","prefix":"ck_","length":40}));
        long["contributes"]["provider"][0]["costs"][0]["action"] = json!("a".repeat(500));
        let m = parse(long).unwrap();
        assert!(super::parts(&m)[0].costs[0].action.chars().count() <= moss_core::untrusted_text::MAX_NAME + 1, "the action is clamped");
        let with = |cost: Value| {
            let mut m = one_account_two_parts(json!({"kind":"key","url":"https://cloud.example/t","prefix":"ck_","length":40}));
            m["contributes"]["provider"][0]["costs"] = json!([cost]);
            parse(m).map(|_| ())
        };
        let cost = |amount: &str, unit: &str, page: &str| json!({"action":"publish","amount":amount,"unit":unit,"price_url":page});
        assert_eq!(with(cost("15", "credits", "https://cloud.example/pricing")), Ok(()));
        assert_eq!(with(json!({"action":"post","variant":"with_link","amount":"0.20","unit":"USD","price_url":"https://x.example/p"})), Ok(()));
        for (bad, said) in [
            (cost("0.2.0", "USD", "https://x.example/p"), "decimal number"),
            (cost(".5", "USD", "https://x.example/p"), "decimal number"),
            (cost("5.", "USD", "https://x.example/p"), "decimal number"),
            (cost("1.0000001", "USD", "https://x.example/p"), "decimal number"),
            (cost("1", "dollars", "https://x.example/p"), "currency code"),
            (json!({"action":"a","amount":0.2,"unit":"USD","price_url":"https://x.example/p"}), "invalid type"),
            (cost("1", "usd", "https://x.example/p"), "currency code"),
            (cost("1", "USD", "http://x.example/p"), "costs.price_url"),
        ] {
            let err = with(bad).unwrap_err();
            assert!(err.contains(said), "{said}: {err}");
        }
    }
}
