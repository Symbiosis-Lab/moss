//! Which account a folder is signed in to on each channel, and what a
//! channel's `login` answered.
//!
//! The binding is `[accounts.<plugin>]` in `.moss/state.toml`: moss writes it,
//! plugins cannot, and it travels with the folder. Plugins from before this
//! record wrote `boundUserId` / `boundUserName` into their own config, which is
//! still read until every channel answers `login` with a [`LoginOutcome`].

use super::PluginManager;
use crate::plugins::types::Plugin;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// An account on a channel, as moss records it: `id` is the provider's stable
/// identifier, opaque to moss; `name` is shown, and where a channel's `acct`
/// setting is filled by sign-in it is also the address an identity proof
/// writes, so it must be `@name@host` on `host`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AccountBinding {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The account's own server, where its copies live, for a channel whose
    /// accounts live on many servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// What a folder reads as signed in to: its record, or what an older plugin
/// wrote into its own config, where a handle can stand without an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundAccount {
    pub id: Option<String>,
    pub name: Option<String>,
}

impl From<AccountBinding> for BoundAccount {
    fn from(b: AccountBinding) -> Self {
        Self { id: Some(b.id), name: b.name }
    }
}

/// What a channel's `login` answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum LoginOutcome {
    Completed(AccountBinding),
    Cancelled,
    Failed { message: String },
    /// A `login` from before `LoginResult` that did not fail: its binding, if
    /// any, is in the plugin's own config.
    Unsaid,
}

/// Trimmed, non-empty text. A number counts only where older plugins wrote
/// ids as numbers (`legacy`); a `LoginResult` names its account in text.
fn text(v: Option<&Value>, legacy: bool) -> Option<String> {
    let s = match v? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) if legacy => n.to_string(),
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

impl LoginOutcome {
    /// Read a `login` export's return value as the plugin wrote it.
    pub fn from_returned(returned: Option<&Value>) -> Self {
        let field = |k: &str| returned.and_then(|r| r.get(k));
        let failed = |fallback: &str| Self::Failed { message: text(field("message"), false).unwrap_or_else(|| fallback.into()) };
        // An older `login` answers a HookResult, whose failure never reaches
        // here: the hook call already fails with its message.
        let Some(status) = field("status") else { return Self::Unsaid };
        match status.as_str() {
            Some("completed") => {
                let account = field("account");
                let id = account.and_then(|a| a.get("id"));
                let host = match account.and_then(|a| a.get("host")) {
                    None => None,
                    Some(v) => match v.as_str().and_then(super::copies::host_of) {
                        Some(host) => Some(host),
                        None => return Self::Failed { message: "the sign-in named the account's server by something other than a host".into() },
                    },
                };
                match text(id, false) {
                    Some(id) => Self::Completed(AccountBinding { id, name: text(account.and_then(|a| a.get("name")), false), host }),
                    None if id.is_some_and(|v| !v.is_string()) => Self::Failed { message: "the sign-in named the account by something other than text".into() },
                    None => Self::Failed { message: "the sign-in finished without naming the account".into() },
                }
            }
            Some("cancelled") => Self::Cancelled,
            Some("failed") => failed("the sign-in failed"),
            _ => Self::Failed { message: format!("login answered an unknown status: {status}") },
        }
    }
}

/// A finished `login`, as the app needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct SignedIn {
    pub outcome: LoginOutcome,
    /// The folder had no account before and has one now: the moment a
    /// channel that fills the folder runs its first sync.
    pub newly_bound: bool,
}

const ACCOUNTS: &str = "accounts";

/// `plugin`'s slot in `[accounts]`.
enum Slot {
    Bound(AccountBinding),
    SignedOut,
}

fn slot(project_path: &str, plugin: &str) -> Option<Slot> {
    let path = Path::new(project_path).join(".moss").join("state.toml");
    let text = crate::build::site_config::read_managed_toml(&path).ok()??;
    let root: toml::Table = toml::from_str(&text).ok()?;
    let value = root.get(ACCOUNTS)?.get(plugin)?;
    if value.as_str() == Some(SIGNED_OUT) {
        return Some(Slot::SignedOut);
    }
    let binding: AccountBinding = value.clone().try_into().ok()?;
    (!binding.id.trim().is_empty()).then_some(Slot::Bound(binding))
}

/// The folder's typed record for `plugin`: a sign out reads as none, and the
/// legacy `boundUserName` is not consulted.
pub(crate) fn recorded(project_path: &str, plugin: &str) -> Option<AccountBinding> {
    match slot(project_path, plugin)? {
        Slot::Bound(binding) => Some(binding),
        Slot::SignedOut => None,
    }
}

/// The account `plugin` is signed in to in this folder, if any.
pub fn account_binding(project_path: &str, plugin: &str) -> Option<BoundAccount> {
    let bound = recorded(project_path, plugin).map(BoundAccount::from).or_else(|| {
        let config = crate::plugins::discovery::get_plugin_config(project_path, plugin).ok()?;
        let bound = BoundAccount { id: text(config.get("boundUserId"), true), name: text(config.get("boundUserName"), true) };
        (bound.id.is_some() || bound.name.is_some()).then_some(bound)
    })?;
    Some(BoundAccount { name: super::copies::shown_name(bound.name, bound.id.as_deref()), ..bound })
}

/// The server of the account `plugin` is signed in to in this folder, when
/// the sign-in named one.
pub fn account_host(project_path: &str, plugin: &str) -> Option<String> {
    recorded(project_path, plugin)?.host
}

/// Record that this folder is signed in to `binding` on `plugin`.
pub fn bind_account(project_path: &str, plugin: &str, binding: &AccountBinding) -> Result<(), String> {
    let value = toml::Value::try_from(binding).map_err(|e| format!("could not record the {plugin} account: {e}"))?;
    crate::vault::deployment_state::update_state(project_path, |root| {
        crate::vault::config::subtable(root, ACCOUNTS)?.insert(plugin.to_string(), value);
        Ok(())
    })
}

/// Written in place of a cleared binding. A conflicted copy of `state.toml`
/// fills in only the tables this file lacks, so an empty slot would let a
/// sibling written before the sign out bring the account back; a value that is
/// not a table keeps it out, and reads as no binding. Whichever the file under
/// the canonical name holds, mark or binding, wins over a sibling; the merge
/// has no clock. The row reads this computer's session either way.
const SIGNED_OUT: &str = "signed-out";

/// Whether the person signed this folder out of `plugin`, and has not signed
/// in again since.
pub fn marked_signed_out(project_path: &str, plugin: &str) -> bool {
    matches!(slot(project_path, plugin), Some(Slot::SignedOut))
}

/// A sign-in that bound the folder some other way (an older plugin writing
/// its own binding keys) lifts the sign out too.
fn lift_sign_out(project_path: &str, plugin: &str) -> Result<(), String> {
    if !marked_signed_out(project_path, plugin) {
        return Ok(());
    }
    crate::vault::deployment_state::update_state(project_path, |root| {
        crate::vault::config::subtable(root, ACCOUNTS)?.remove(plugin);
        Ok(())
    })
}

/// The plugin-config keys channels from before `LoginResult` bind with.
const LEGACY_BINDING_KEYS: [&str; 3] = ["boundUserId", "boundUserName", "boundUserHandle"];

/// Forget which account this folder is signed in to on `plugin`. A folder
/// with nothing to forget is left byte for byte as it was.
pub fn unbind_account(project_path: &str, plugin: &str) -> Result<(), String> {
    let forgot_legacy = forget_legacy_binding(&Path::new(project_path).join(".moss").join("plugins").join(plugin))?;
    if recorded(project_path, plugin).is_none() && !forgot_legacy {
        return Ok(());
    }
    crate::vault::deployment_state::update_state(project_path, |root| {
        crate::vault::config::subtable(root, ACCOUNTS)?.insert(plugin.to_string(), toml::Value::String(SIGNED_OUT.into()));
        Ok(())
    })
}

/// Whether the plugin's folder held a legacy binding, now removed. The
/// binding is read from `config.json` and `config.toml` together, so both
/// are cleared.
fn forget_legacy_binding(plugin_dir: &Path) -> Result<bool, String> {
    let json = forget_legacy_json(&plugin_dir.join("config.json"))?;
    let toml = forget_legacy_toml(&plugin_dir.join("config.toml"))?;
    Ok(json || toml)
}

fn forget_legacy_toml(config: &Path) -> Result<bool, String> {
    let Ok(text) = std::fs::read_to_string(config) else { return Ok(false) };
    let Ok(mut table) = toml::from_str::<toml::Table>(&text) else { return Ok(false) };
    if LEGACY_BINDING_KEYS.iter().filter_map(|k| table.remove(*k)).count() == 0 {
        return Ok(false);
    }
    let out = crate::infra::toml_rewrite::apply_changes(&text, &table)?;
    crate::infra::atomic_write::write_atomic(config, &out).map_err(|e| format!("could not clear {}: {e}", config.display()))?;
    Ok(true)
}

fn forget_legacy_json(config: &Path) -> Result<bool, String> {
    let Ok(text) = std::fs::read_to_string(config) else { return Ok(false) };
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(&text) else { return Ok(false) };
    if LEGACY_BINDING_KEYS.iter().filter_map(|k| map.remove(*k)).count() == 0 {
        return Ok(false);
    }
    let out = serde_json::to_string_pretty(&map).map_err(|e| e.to_string())?;
    crate::infra::atomic_write::write_atomic(config, &out).map_err(|e| format!("could not clear {}: {e}", config.display()))?;
    Ok(true)
}

/// What a channel's account row shows: the folder's binding read against the
/// session this computer holds. A `name` is only ever a name the channel or
/// the binding gave: an account id is never shown in its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AccountState {
    SignedIn { name: Option<String> },
    /// The folder is bound, to `name` when it has one, and this computer has
    /// no session for it.
    SignedOut { name: Option<String> },
    /// This computer could not read its session: `why` is the channel's error,
    /// or none when no runtime here may run the channel. `bound` says the
    /// folder has a binding, and `name` is its name, if it has one.
    Unread { name: Option<String>, bound: bool, why: Option<String> },
    NoAccount,
}

/// The row for a channel whose session was not read, from the binding alone.
pub fn account_unread(project_path: &str, plugin: &str, why: Option<String>) -> AccountState {
    let bound = account_binding(project_path, plugin);
    AccountState::Unread { bound: bound.is_some(), name: bound.and_then(|b| b.name), why }
}

impl PluginManager {
    /// The row a channel's settings show. The binding syncs with the folder and
    /// the session belongs to this computer, so a channel's `copy_account` has
    /// the last word, and a channel without one is taken at its binding.
    pub async fn account_state(&self, plugin: &str) -> AccountState {
        let bound = account_binding(&self.project_path, plugin);
        match self.copy_account(plugin).await {
            super::copies::Account::Known { id, name } => {
                let same = bound.filter(|b| b.id.as_deref() == Some(id.as_str()));
                AccountState::SignedIn { name: name.or_else(|| same.and_then(|b| b.name)) }
            }
            super::copies::Account::SignedOut => bound.map_or(AccountState::NoAccount, |b| AccountState::SignedOut { name: b.name }),
            super::copies::Account::Unknown(why) => account_unread(&self.project_path, plugin, Some(why)),
            super::copies::Account::Unsaid => bound.map_or(AccountState::NoAccount, |b| AccountState::SignedIn { name: b.name }),
        }
    }

    /// Sign this computer out of `plugin`'s account and forget it in this
    /// folder: the plugin's `logout` first, while the credential still exists
    /// to revoke and the plugin can forget what its sign-in learned; then the
    /// secrets the manifest declares; then the binding. Every step runs; the
    /// answer names each that failed, so the person can try again.
    pub async fn sign_out(&self, plugin: &Plugin, context_json: &str, secrets: &crate::identity::secrets::SecretStore) -> Vec<String> {
        let name = &plugin.manifest.name;
        let mut failed = Vec::new();
        match self.execute_plugin_javascript(plugin, "logout", context_json, super::AskedBy::Person).await {
            Err(e) if !crate::engine::is_missing_export(&e) => failed.push(format!("{name} could not sign out: {e}")),
            _ => {}
        }
        match crate::identity::keystore::Scope::plugin(name) {
            Ok(scope) => {
                let keys = plugin.manifest.user_supplied_secret_keys().into_iter().chain(plugin.manifest.plugin_owned_secret_keys());
                for key in keys {
                    if let Err(e) = secrets.remove(&scope, &key) {
                        failed.push(format!("could not forget the {name} credential {key}: {e:?}"));
                    }
                }
            }
            Err(e) => failed.push(format!("could not forget the {name} credentials: {e:?}")),
        }
        if let Err(e) = unbind_account(&self.project_path, name) {
            failed.push(e);
        }
        failed
    }

    /// Run `plugin`'s `login` and record the account it names.
    pub async fn sign_in(&self, plugin: &Plugin, context_json: &str) -> Result<SignedIn, String> {
        let name = &plugin.manifest.name;
        let before = account_binding(&self.project_path, name).is_some();
        // A press while this login waits on the browser starts it again rather
        // than joining it. The old run is cancelled, as the watchdog would, since
        // its code may catch the cancellation and keep waiting. The engine's
        // answer to the cancel must reach the old run, and its teardown (which
        // closes the panel) must finish, before the new run registers.
        if crate::plugins::runtime::browser_sign_in::end_wait(name) {
            self.engine.cancel_hook(name, "login");
            self.host.state.hooks.in_flight_hooks.settled(&super::singleflight_key(name, "login", super::AskedBy::Person)).await;
        }
        let (_, returned) = self.execute_plugin_javascript(plugin, "login", context_json, super::AskedBy::Person).await?;
        let outcome = LoginOutcome::from_returned(returned.as_ref());
        if let LoginOutcome::Completed(account) = &outcome {
            bind_account(&self.project_path, name, account)?;
        }
        let newly_bound = !before && account_binding(&self.project_path, name).is_some();
        if newly_bound {
            lift_sign_out(&self.project_path, name)?;
        }
        Ok(SignedIn { outcome, newly_bound })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::adapter_host::AdapterHost;
    use crate::plugins::manager::HookWatchdog;
    use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};
    use serde_json::json;

    fn completed(id: &str, name: Option<&str>) -> LoginOutcome {
        LoginOutcome::Completed(AccountBinding { id: id.into(), name: name.map(str::to_string), host: None })
    }

    #[test]
    fn a_login_answer_reads_as_one_of_four_outcomes() {
        let read = |v: serde_json::Value| LoginOutcome::from_returned(Some(&v));
        assert_eq!(read(json!({"status": "completed", "account": {"id": "did:plc:1", "name": "@ada"}})), completed("did:plc:1", Some("@ada")));
        assert_eq!(read(json!({"status": "completed", "account": {"id": 42}})), LoginOutcome::Failed { message: "the sign-in named the account by something other than text".into() });
        assert_eq!(read(json!({"status": "completed", "account": {"name": "@ada"}})), LoginOutcome::Failed { message: "the sign-in finished without naming the account".into() });
        assert_eq!(read(json!({"status": "cancelled"})), LoginOutcome::Cancelled);
        assert_eq!(read(json!({"status": "failed", "message": "server refused"})), LoginOutcome::Failed { message: "server refused".into() });
        assert!(matches!(read(json!({"status": "maybe"})), LoginOutcome::Failed { .. }));
        assert_eq!(read(json!({"success": true})), LoginOutcome::Unsaid);
        assert_eq!(LoginOutcome::from_returned(None), LoginOutcome::Unsaid);
    }

    fn folder_with_channel(login_body: &str) -> (tempfile::TempDir, String) {
        folder_with_hooks(&format!("async login(ctx) {{ {login_body} }}"))
    }

    async fn sign_in(project: &str) -> SignedIn {
        try_sign_in(project).await.unwrap()
    }

    async fn try_sign_in(project: &str) -> Result<SignedIn, String> {
        let manager = PluginManager::new(project, AdapterHost::headless(crate::build::ports::reporter::discard_owned())).unwrap();
        let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == "social").unwrap();
        let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
        let context = serde_json::to_string(&ProcessContext::new(project.into(), info, TriggerContext::SettingsManual)).unwrap();
        manager.sign_in(&plugin, &context).await
    }

    #[tokio::test]
    async fn a_completed_sign_in_binds_the_folder_to_the_account_it_names() {
        let (_dir, project) = folder_with_channel(r#"return { status: "completed", account: { id: "did:plc:1", name: "@ada" } };"#);
        let first = sign_in(&project).await;
        assert_eq!(first, SignedIn { outcome: completed("did:plc:1", Some("@ada")), newly_bound: true });
        assert_eq!(account_binding(&project, "social"), Some(BoundAccount { id: Some("did:plc:1".into()), name: Some("@ada".into()) }));
        let path = std::path::Path::new(&project).join(".moss/state.toml");
        let (bytes, modified) = (std::fs::read(&path).unwrap(), std::fs::metadata(&path).unwrap().modified().unwrap());
        assert!(String::from_utf8_lossy(&bytes).contains("[accounts.social]"));
        assert!(!sign_in(&project).await.newly_bound, "signing in again binds nothing new");
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "the same account again leaves the synced file alone");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    /// How Matters and every channel from before `LoginResult` bind: `login`
    /// returns no status and writes the handle into its own config.
    #[tokio::test]
    async fn an_older_login_that_writes_its_own_binding_binds_the_folder_and_a_dismiss_does_not() {
        let write_config = |config: &str| format!(
            r#"await globalThis.__TAURI__.core.invoke("write_plugin_file", {{ relativePath: "config.json", content: JSON.stringify({config}) }}); return {{ success: true }};"#
        );
        let (_dir, project) = folder_with_channel(&write_config(r#"{ boundUserName: "@ada" }"#));
        assert_eq!(sign_in(&project).await, SignedIn { outcome: LoginOutcome::Unsaid, newly_bound: true });
        assert!(!sign_in(&project).await.newly_bound, "a re-login syncs on the next build");
        let (_dir, project) = folder_with_channel(&write_config(r#"{ boundUserName: "", login_dismissed: true }"#));
        assert_eq!(sign_in(&project).await, SignedIn { outcome: LoginOutcome::Unsaid, newly_bound: false });
        let (_dir, project) = folder_with_channel(r#"return { success: false, message: "wrong password" };"#);
        assert_eq!(try_sign_in(&project).await, Err("Hook failed: wrong password".into()), "an older login's failure keeps its message");
        assert_eq!(account_binding(&project, "social"), None);
    }

    #[tokio::test]
    async fn a_sign_in_waiting_on_nobody_is_cancelled_at_the_watchdogs_limit() {
        let (_dir, project) = folder_with_channel("await new Promise(() => {});");
        let watchdog = HookWatchdog { limit: std::time::Duration::from_secs(1), check: std::time::Duration::from_millis(100) };
        let manager = PluginManager::new(&project, AdapterHost::headless(crate::build::ports::reporter::discard_owned())).unwrap().with_watchdog(watchdog);
        let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == "social").unwrap();
        let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
        let context = serde_json::to_string(&ProcessContext::new(project.clone(), info, TriggerContext::SettingsManual)).unwrap();
        let started = std::time::Instant::now();
        let refused = manager.sign_in(&plugin, &context).await.unwrap_err();
        assert_eq!(refused, "Hook 'social::login' timed out - no progress for 1 seconds");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the shortened limit, not the default minute");
        assert_eq!(account_binding(&project, "social"), None);
    }

    #[tokio::test]
    async fn a_cancelled_failed_or_unsaid_sign_in_binds_nothing() {
        let (_dir, project) = folder_with_channel(r#"return { status: "cancelled" };"#);
        assert_eq!(sign_in(&project).await, SignedIn { outcome: LoginOutcome::Cancelled, newly_bound: false });
        assert_eq!(account_binding(&project, "social"), None);
        let (_dir, project) = folder_with_channel(r#"return { status: "failed", message: "server refused" };"#);
        assert_eq!(sign_in(&project).await, SignedIn { outcome: LoginOutcome::Failed { message: "server refused".into() }, newly_bound: false });
        assert_eq!(account_binding(&project, "social"), None);
        let (_dir, project) = folder_with_channel("return undefined;");
        assert_eq!(sign_in(&project).await, SignedIn { outcome: LoginOutcome::Unsaid, newly_bound: false });
        assert!(!std::path::Path::new(&project).join(".moss/state.toml").exists());
    }

    fn write(project: &str, rel: &str, body: &str) {
        let path = std::path::Path::new(project).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn an_account_and_a_publish_keep_each_others_record() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write(project, ".moss/state.toml", "schema_version = 1\n\n[deployment]\nsite_id = \"site-1\"\n");
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        crate::vault::deployment_state::update_domain_observation(project, |_| None).unwrap();
        let state = std::fs::read_to_string(dir.path().join(".moss/state.toml")).unwrap();
        assert!(state.contains("site_id = \"site-1\""), "{state}");
        assert_eq!(account_binding(project, "social").and_then(|b| b.id), Some("acct:1".into()));
    }

    #[test]
    fn a_binding_an_older_plugin_wrote_still_reads_and_the_record_wins() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write(project, ".moss/plugins/social/config.json", r#"{"boundUserName": "@ada"}"#);
        assert_eq!(account_binding(project, "social"), Some(BoundAccount { id: None, name: Some("@ada".into()) }), "a handle is never an id");
        write(project, ".moss/plugins/social/config.json", r#"{"boundUserId": 42, "boundUserName": ""}"#);
        assert_eq!(account_binding(project, "social"), Some(BoundAccount { id: Some("42".into()), name: None }));
        write(project, ".moss/plugins/social/config.json", r#"{"boundUserName": "", "login_dismissed": true}"#);
        assert_eq!(account_binding(project, "social"), None, "a dismissed sign-in is not an account");
        write(project, ".moss/plugins/social/config.json", r#"{"boundUserName": "@old"}"#);
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: Some("@new".into()), host: None }).unwrap();
        assert_eq!(account_binding(project, "social").and_then(|b| b.name), Some("@new".into()));
    }

    #[tokio::test]
    async fn the_server_a_sign_in_names_owns_that_servers_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap().to_string();
        write(&project, ".moss/plugins/social/manifest.json", r#"{"name":"social","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{"channel":{"login":true,"syndication":{"forms":["short"]}}}}"#);
        write(&project, ".moss/plugins/social/main.bundle.js", r#"var Social = { async login(ctx) {
            return { status: "completed", account: { id: "social.example.net/1", name: "@a@social.example.net", host: "https://Social.Example.net/" } }; } };"#);
        write(&project, ".moss/config.toml", "[channels.social]\n");
        assert!(matches!(sign_in(&project).await.outcome, LoginOutcome::Completed(_)));

        use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
        use crate::syndication::plan::{decide, Action, PlanInputs};
        let listed = "https://social.example.net/@a/1";
        let post: ArticleInfo = serde_json::from_value(json!({ "source_path": "a.md", "title": "A", "content": "Hello.", "url_path": "a/", "date": null,
            "uid": "aa11aa11", "syndicate": ["social"], "frontmatter": { "uid": "aa11aa11", "syndicated": [listed] } })).unwrap();
        let mut articles = ArticleMap::default();
        articles.articles.insert(post.url_path.clone(), post);
        let channels = crate::syndication::landing::channels(dir.path());
        let config = crate::config::ConfigFile::parse("").unwrap();
        let records = crate::syndication::record::Records::at(dir.path().join("copies"));
        let none = std::collections::HashSet::new();
        let plans = decide(&PlanInputs { folder: dir.path(), config: &config, site_url: Some("https://example.org"), articles: &articles, previously_live: None, edited_sources: &none, channels: &channels }, &records);
        let actions: Vec<_> = plans.iter().flat_map(|p| p.entries.iter().map(|e| e.action.clone())).collect();
        assert_eq!(actions, [Action::Adopt { url: listed.into() }], "a copy on the signed-in server is adopted, not posted again");
    }

    #[test]
    fn a_sign_ins_server_is_a_host_or_the_sign_in_fails() {
        let read = |host: serde_json::Value| LoginOutcome::from_returned(Some(&json!({"status": "completed", "account": {"id": "s/1", "host": host}})));
        let bound = |host: &str| LoginOutcome::Completed(AccountBinding { id: "s/1".into(), name: None, host: Some(host.into()) });
        assert_eq!(read(json!("https://Social.Example.net:443/")), bound("social.example.net"));
        assert_eq!(read(json!("social.example.net")), bound("social.example.net"));
        let refused = LoginOutcome::Failed { message: "the sign-in named the account's server by something other than a host".into() };
        assert_eq!(read(json!(7)), refused);
        assert_eq!(read(json!("")), refused);
        assert_eq!(read(json!(null)), refused);
    }

    /// A channel whose bundle is `body` (the members of its global), with one
    /// hidden secret, `token`.
    fn folder_with_hooks(body: &str) -> (tempfile::TempDir, String) {
        folder_with_plugin("social", body)
    }

    fn folder_with_plugin(name: &str, body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap().to_string();
        write(&project, &format!(".moss/plugins/{name}/manifest.json"), &format!(r#"{{"name":"{name}","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{{"channel":{{"login":true,
            "setup":{{"settings":[{{"key":"token","type":"secret","label":"Token","hidden":true}}]}}}}}}}}"#));
        write(&project, &format!(".moss/plugins/{name}/main.bundle.js"), &format!("var Social = {{ {body} }};"));
        (dir, project)
    }

    fn manager(project: &str) -> (PluginManager, Plugin) {
        let manager = PluginManager::new(project, AdapterHost::headless(crate::build::ports::reporter::discard_owned())).unwrap();
        let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == "social").unwrap();
        (manager, plugin)
    }

    fn context(project: &str) -> String {
        let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
        serde_json::to_string(&ProcessContext::new(project.into(), info, TriggerContext::SettingsManual)).unwrap()
    }

    const WRITES_LOGGED_OUT: &str = r#"async logout(ctx) { await globalThis.__TAURI__.core.invoke("write_plugin_file", { relativePath: "logged-out", content: "yes" }); return { success: true }; }"#;

    #[tokio::test]
    async fn sign_out_runs_the_plugins_logout_and_forgets_the_credentials_and_the_binding() {
        let (dir, project) = folder_with_hooks(WRITES_LOGGED_OUT);
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()), host: None }).unwrap();
        write(&project, ".moss/plugins/social/config.json", r#"{"boundUserId": "acct:1", "boundUserName": "ada", "publication": "https://ada.example"}"#);
        let app = tempfile::tempdir().unwrap();
        let secrets = crate::identity::secrets::SecretStore::in_app_data(app.path());
        let scope = crate::identity::keystore::Scope::plugin("social").unwrap();
        secrets.set(&scope, "token", "t0k").unwrap();
        let (manager, plugin) = manager(&project);
        assert_eq!(manager.sign_out(&plugin, &context(&project), &secrets).await, Vec::<String>::new());
        assert!(dir.path().join(".moss/plugins/social/logged-out").exists(), "the plugin's logout ran");
        assert_eq!(secrets.get(&scope, "token").unwrap(), None);
        assert_eq!(account_binding(&project, "social"), None);
        let config: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join(".moss/plugins/social/config.json")).unwrap()).unwrap();
        assert_eq!(config, json!({"publication": "https://ada.example"}), "moss forgets only the binding keys; the plugin's logout owns the rest");
    }

    #[tokio::test]
    async fn a_logout_that_fails_is_reported_and_the_rest_of_sign_out_still_runs() {
        let (_dir, project) = folder_with_hooks(r#"async logout(ctx) { throw new Error("revoke refused"); }"#);
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        let app = tempfile::tempdir().unwrap();
        let secrets = crate::identity::secrets::SecretStore::in_app_data(app.path());
        let (manager, plugin) = manager(&project);
        let failed = manager.sign_out(&plugin, &context(&project), &secrets).await;
        assert!(matches!(failed.as_slice(), [one] if one.contains("revoke refused")), "{failed:?}");
        assert_eq!(account_binding(&project, "social"), None);
    }

    #[tokio::test]
    async fn a_channel_with_no_logout_signs_out_without_a_failure() {
        let (_dir, project) = folder_with_hooks("");
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        let app = tempfile::tempdir().unwrap();
        let secrets = crate::identity::secrets::SecretStore::in_app_data(app.path());
        let (manager, plugin) = manager(&project);
        assert_eq!(manager.sign_out(&plugin, &context(&project), &secrets).await, Vec::<String>::new());
        assert_eq!(account_binding(&project, "social"), None);
    }

    #[test]
    fn a_folder_signed_out_does_not_ask_to_sign_in_until_it_signs_in_again() {
        use crate::plugins::discovery::plugin_needs_connection;
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        assert!(plugin_needs_connection(project, "social"));
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        unbind_account(project, "social").unwrap();
        assert!(!plugin_needs_connection(project, "social"), "a build after Sign out does not open the sign-in");
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None, host: None }).unwrap();
        assert!(!marked_signed_out(project, "social"), "signing in again lifts it");
    }

    #[test]
    fn signing_out_forgets_a_binding_the_settings_file_still_holds() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let settings = "# the person's own note\nlanguage = 'zh'\nboundUserName = \"@ada\"\n";
        write(project, ".moss/plugins/social/config.toml", settings);
        write(project, ".moss/plugins/social/config.json", "{\"signedOut\": false}");
        assert!(account_binding(project, "social").is_some());
        unbind_account(project, "social").unwrap();
        assert_eq!(account_binding(project, "social"), None, "the row names no account after Sign out");
        assert!(marked_signed_out(project, "social"));
        assert_eq!(std::fs::read_to_string(dir.path().join(".moss/plugins/social/config.toml")).unwrap(), "# the person's own note\nlanguage = 'zh'\n");
    }

    #[test]
    fn signing_out_a_folder_with_no_account_leaves_its_state_alone() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        write(project, ".moss/state.toml", "[deployment]\nsite_id = \"site-1\"\n");
        let path = dir.path().join(".moss/state.toml");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        unbind_account(project, "social").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[deployment]\nsite_id = \"site-1\"\n");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        unbind_account(project, "social").unwrap();
        let (bytes, modified) = (std::fs::read(&path).unwrap(), std::fs::metadata(&path).unwrap().modified().unwrap());
        unbind_account(project, "social").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "signing out twice writes once");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn a_conflicted_copy_from_before_the_sign_out_does_not_bring_the_account_back() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()), host: None }).unwrap();
        let before = std::fs::read_to_string(dir.path().join(".moss/state.toml")).unwrap();
        unbind_account(project, "social").unwrap();
        write(project, ".moss/state 2.toml", &before);
        crate::vault::synced_siblings::reconcile_synced_state(&crate::moss_paths::MossPaths::new(dir.path()));
        assert!(!dir.path().join(".moss/state 2.toml").exists(), "the sibling was folded in");
        assert_eq!(account_binding(project, "social"), None);
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None, host: None }).unwrap();
        assert_eq!(account_binding(project, "social").and_then(|b| b.id), Some("acct:2".into()), "signing in again replaces the mark");
    }

    #[test]
    fn a_conflicted_copy_from_after_a_sign_in_does_not_sign_the_folder_out() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        unbind_account(project, "social").unwrap();
        let signed_out_copy = std::fs::read_to_string(dir.path().join(".moss/state.toml")).unwrap();
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None, host: None }).unwrap();
        write(project, ".moss/state 2.toml", &signed_out_copy);
        crate::vault::synced_siblings::reconcile_synced_state(&crate::moss_paths::MossPaths::new(dir.path()));
        assert_eq!(account_binding(project, "social").and_then(|b| b.id), Some("acct:2".into()));
    }

    #[tokio::test]
    async fn an_older_login_after_a_sign_out_lifts_it() {
        let (_dir, project) = folder_with_channel(r#"await globalThis.__TAURI__.core.invoke("write_plugin_file", { relativePath: "config.json", content: JSON.stringify({ boundUserName: "@ada" }) }); return { success: true };"#);
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None, host: None }).unwrap();
        unbind_account(&project, "social").unwrap();
        assert!(sign_in(&project).await.newly_bound);
        assert!(!marked_signed_out(&project, "social"));
    }

    async fn state_with(copy_account: &str, bound: Option<Option<&str>>) -> AccountState {
        let (_dir, project) = folder_with_hooks(&format!("async copy_account(q) {{ return {copy_account}; }}"));
        if let Some(name) = bound {
            bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: name.map(Into::into), host: None }).unwrap();
        }
        manager(&project).0.account_state("social").await
    }

    const ADA: Option<Option<&str>> = Some(Some("@ada"));
    const ID_ONLY: Option<Option<&str>> = Some(None);

    fn named(name: &str) -> Option<String> {
        Some(name.into())
    }

    #[tokio::test]
    async fn the_account_row_reads_the_binding_against_this_computers_session() {
        let signed_out = r#"{ success: true, copy: { signed_out: true } }"#;
        assert_eq!(state_with(signed_out, ADA).await, AccountState::SignedOut { name: named("@ada") });
        assert_eq!(state_with(signed_out, None).await, AccountState::NoAccount);
        let same = r#"{ success: true, copy: { account: "acct:1" } }"#;
        assert_eq!(state_with(same, ADA).await, AccountState::SignedIn { name: named("@ada") });
        let other = r#"{ success: true, copy: { account: "acct:9" } }"#;
        assert_eq!(state_with(other, ADA).await, AccountState::SignedIn { name: None }, "another account's session is neither shown under this binding's name nor by its id");
        let (_dir, project) = folder_with_hooks("");
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()), host: None }).unwrap();
        assert_eq!(manager(&project).0.account_state("social").await, AccountState::SignedIn { name: named("@ada") }, "no copy_account: the binding");
        let failed = r#"{ success: false, error: "rate limited" }"#;
        assert!(matches!(state_with(failed, ADA).await, AccountState::Unread { name: Some(n), bound: true, why: Some(_) } if n == "@ada"), "a failed read is neither signed in nor signed out");
        assert!(matches!(state_with(failed, None).await, AccountState::Unread { name: None, bound: false, why: Some(_) }));
        let refused = r#"{ success: false, message: "Sign in first", copy: { refused: { cause: "auth", message: "Sign in first" } } }"#;
        assert_eq!(state_with(refused, None).await, AccountState::NoAccount, "a refusal for want of a credential is signed out, not an error");
        assert_eq!(state_with(refused, ADA).await, AccountState::SignedOut { name: named("@ada") });
        let offline = r#"{ success: false, message: "offline", copy: { refused: { cause: "network" } } }"#;
        assert_eq!(state_with(offline, None).await, AccountState::Unread { name: None, bound: false, why: Some("offline".into()) }, "any other refusal keeps the channel's own words");
    }

    #[tokio::test]
    async fn an_account_row_never_shows_an_id_as_its_name() {
        let same = r#"{ success: true, copy: { account: "acct:1" } }"#;
        assert_eq!(state_with(same, ID_ONLY).await, AccountState::SignedIn { name: None }, "a binding without a name");
        let echoed = r#"{ success: true, copy: { account: "acct:1", name: "acct:1" } }"#;
        assert_eq!(state_with(echoed, None).await, AccountState::SignedIn { name: None }, "a channel that answers its id as the name");
        let signed_out = r#"{ success: true, copy: { signed_out: true } }"#;
        assert_eq!(state_with(signed_out, ID_ONLY).await, AccountState::SignedOut { name: None });
        let failed = r#"{ success: false, error: "rate limited" }"#;
        assert!(matches!(state_with(failed, ID_ONLY).await, AccountState::Unread { name: None, bound: true, why: Some(_) }), "an unread row keeps its binding without a name");
        let (_dir, project) = folder_with_hooks("");
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some("acct:1".into()), host: None }).unwrap();
        assert_eq!(manager(&project).0.account_state("social").await, AccountState::SignedIn { name: None }, "a binding whose name is its id");
    }

    /// The desktop as a sign-in page sees it: one browser panel, and an event
    /// bus whose `browser-closed` fires however the panel closes.
    #[derive(Default)]
    struct Desk {
        pages: std::sync::Mutex<Vec<String>>,
        closes: std::sync::atomic::AtomicUsize,
        signed_in_closes: std::sync::atomic::AtomicUsize,
        /// Addresses opened in the person's own browser.
        browsed: std::sync::Mutex<Vec<String>>,
        listeners: std::sync::Mutex<Vec<(String, std::sync::Arc<dyn Fn(&str) + Send + Sync>)>>,
    }

    impl Desk {
        fn fire(&self, name: &str, payload: &str) {
            let handlers: Vec<_> = self.listeners.lock().unwrap().iter().filter(|(n, _)| n == name).map(|(_, h)| h.clone()).collect();
            for handler in handlers {
                handler(payload);
            }
        }

        fn closes(&self) -> usize {
            self.closes.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn signed_in_closes(&self) -> usize {
            self.signed_in_closes.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// Until the plugin has its page open and listens for the click on it.
        async fn page_waiting_for(&self, event: &str) {
            for _ in 0..500 {
                let listening = self.listeners.lock().unwrap().iter().any(|(n, _)| n == event);
                if listening && !self.pages.lock().unwrap().is_empty() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            panic!("the sign-in never opened its page and listened for {event}");
        }
    }

    #[async_trait::async_trait]
    impl crate::engine::app_host::AppHost for Desk {
        async fn get_plugin_cookies(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
        async fn set_plugin_cookies(&self, _: &str, _: &str, _: Value) -> Result<(), String> { unreachable!() }
        async fn clear_plugin_cookies(&self, _: &str, _: &str) -> Result<(), String> { unreachable!() }
        async fn open_action_panel(&self, _: Option<String>, _: Option<String>, _: bool, _: Option<String>) -> Result<(), String> { unreachable!() }
        async fn set_action_panel_html(&self, html: String, _: Option<String>, _: Option<String>) -> Result<(), String> {
            self.pages.lock().unwrap().push(html);
            Ok(())
        }
        async fn close_action_panel(&self, _: Option<String>) -> Result<(), String> {
            self.closes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.fire("browser-closed", r#"{"reason":"programmatic"}"#);
            Ok(())
        }
        async fn close_signed_in_page(&self) -> Result<(), String> {
            self.signed_in_closes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        async fn open_system_browser(&self, url: String) -> Result<(), String> {
            self.browsed.lock().unwrap().push(url);
            Ok(())
        }
        async fn return_to_editor(&self) -> Result<(), String> { unreachable!() }
        async fn fill_browser_panel(&self, _: &str, _: &str, _: Value) -> Result<Value, String> { unreachable!() }
        async fn prompt_credential(&self, _: &str, _: &str, _: &str, _: Option<String>) -> Result<Option<String>, String> { unreachable!() }
        fn listen(&self, name: String, handler: Box<dyn Fn(&str) + Send + Sync + 'static>) -> crate::engine::app_host::ListenerId {
            let mut listeners = self.listeners.lock().unwrap();
            listeners.push((name, handler.into()));
            listeners.len() as u32
        }
        fn unlisten(&self, _: crate::engine::app_host::ListenerId) {}
        fn emit_plugin_event(&self, _: &str, _: Option<String>) {}
    }

    fn manager_at_desk(project: &str) -> (PluginManager, Plugin, std::sync::Arc<Desk>) {
        manager_of_at_desk(project, "social")
    }

    fn manager_of_at_desk(project: &str, name: &str) -> (PluginManager, Plugin, std::sync::Arc<Desk>) {
        let desk = std::sync::Arc::new(Desk::default());
        let host = AdapterHost {
            state: crate::engine::host_fns::HostState::standalone(),
            app: Some(desk.clone()),
            reporter: crate::build::ports::reporter::discard_owned(),
            spawner: std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner),
        };
        let watchdog = HookWatchdog { limit: std::time::Duration::from_secs(1), check: std::time::Duration::from_millis(100) };
        let manager = PluginManager::new(project, host).unwrap().with_watchdog(watchdog);
        let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == name).unwrap();
        (manager, plugin, desk)
    }

    /// What a channel's server picker does: open the page, then wait for the
    /// person's click on it, with no host call in flight meanwhile.
    const PICKS_A_SERVER: &str = r#"
        await globalThis.__TAURI__.core.invoke("set_action_panel_html", { html: "<button>Continue</button>" });
        const server = await new Promise((picked) => { globalThis.__TAURI__.event.listen("picker:continue", (e) => picked(e.payload.server)); });"#;

    /// The person takes longer than the watchdog's (shortened) limit to click.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_sign_in_waiting_on_its_page_outlives_the_watchdog_and_finishes_on_the_click() {
        let (_dir, project) = folder_with_channel(&format!(r#"{PICKS_A_SERVER}
            return {{ status: "completed", account: {{ id: server + "/1", name: "@ada" }} }};"#));
        let (manager, plugin, desk) = manager_at_desk(&project);
        let ctx = context(&project);
        let (signed_in, ()) = tokio::join!(manager.sign_in(&plugin, &ctx), async {
            desk.page_waiting_for("picker:continue").await;
            tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
            desk.fire("picker:continue", r#"{"server":"social.example.net"}"#);
        });
        assert_eq!(signed_in.unwrap().outcome, completed("social.example.net/1", Some("@ada")));
        assert_eq!(desk.signed_in_closes(), 1, "the sign-in's page closes with it");
    }

    /// However the sign-in ends, its page does not outlive it; only a sign-in
    /// that worked closes it as signed in.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_sign_in_that_ends_closes_its_page() {
        let endings = [
            (r#"return { status: "completed", account: { id: "s/1" } };"#, true),
            ("return;", true),
            (r#"throw new Error("server refused");"#, false),
            (r#"return { status: "cancelled" };"#, false),
            (r#"return { status: "failed", message: "no" };"#, false),
        ];
        for (ending, signed_in) in endings {
            let (_dir, project) = folder_with_channel(&format!(r#"await globalThis.__TAURI__.core.invoke("set_action_panel_html", {{ html: "<p>Pick</p>" }}); {ending}"#));
            let (manager, plugin, desk) = manager_at_desk(&project);
            let _ = manager.sign_in(&plugin, &context(&project)).await;
            assert_eq!((desk.signed_in_closes(), desk.closes()), if signed_in { (1, 0) } else { (0, 1) }, "{ending}");
            assert!(!manager.host.state.hooks.is_making_progress("social", "login"), "{ending}");
        }
    }

    /// A click on a finished sign-in's page reaches a listener that ended with
    /// the hook, and the next sign-in runs as usual.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_click_on_a_finished_sign_in_page_reaches_nothing() {
        let (_dir, project) = folder_with_channel(&format!(r#"{PICKS_A_SERVER}
            return {{ status: "completed", account: {{ id: server + "/1" }} }};"#));
        let (manager, plugin, desk) = manager_at_desk(&project);
        let ctx = context(&project);
        let (first, ()) = tokio::join!(manager.sign_in(&plugin, &ctx), async {
            desk.page_waiting_for("picker:continue").await;
            desk.fire("picker:continue", r#"{"server":"one.example"}"#);
        });
        assert_eq!(first.unwrap().outcome, completed("one.example/1", None));
        desk.fire("picker:continue", r#"{"server":"stale.example"}"#);
        let again = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(manager.sign_in(&plugin, &ctx), async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                desk.fire("picker:continue", r#"{"server":"two.example"}"#);
            })
        });
        let (second, ()) = again.await.expect("a stale click must not wedge the next sign-in");
        assert_eq!(second.unwrap().outcome, completed("two.example/1", None));
        assert_eq!(desk.signed_in_closes(), 2);
    }

    /// The person closing the page ends the wait: a sign-in still waiting for
    /// a click on it is cut off at the watchdog's limit instead of hanging.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_person_closing_the_page_puts_the_sign_in_back_under_the_watchdog() {
        let (_dir, project) = folder_with_channel(&format!(r#"{PICKS_A_SERVER} return {{ status: "cancelled" }};"#));
        let (manager, plugin, desk) = manager_at_desk(&project);
        let ctx = context(&project);
        let waiting = tokio::time::timeout(std::time::Duration::from_secs(10), manager.sign_in(&plugin, &ctx));
        let (signed_in, ()) = tokio::join!(waiting, async {
            desk.page_waiting_for("picker:continue").await;
            desk.fire("browser-closed", r#"{"reason":"user"}"#);
        });
        assert_eq!(signed_in.expect("a closed page must not hold the sign-in forever").unwrap_err(), "Hook 'social::login' timed out - no progress for 1 seconds");
        assert_eq!(desk.closes(), 0, "a page already closed is not closed again");
    }

    /// `startBrowserSignIn` as moss-api writes it, over the same host calls.
    const BROWSER_SIGN_IN: &str = r#"
        const startBrowserSignIn = async () => {
            const { redirectUri } = await globalThis.__TAURI__.core.invoke("browser_sign_in_start", {});
            return { redirectUri, open: async (url) => {
                try {
                    const { params } = await globalThis.__TAURI__.core.invoke("browser_sign_in_wait", { url });
                    return Object.fromEntries(params);
                } catch (e) {
                    const message = e instanceof Error ? e.message : String(e);
                    if (message !== "sign-in cancelled") throw e;
                    const cancelled = new Error(message);
                    cancelled.name = "SignInCancelled";
                    throw cancelled;
                }
            } };
        };
        const authorize = (s) => "https://social.example.net/authorize?redirect_uri=" + encodeURIComponent(s.redirectUri);"#;

    /// The person closes the browser tab, which tells moss nothing, and presses
    /// Sign in again: the press starts a fresh sign-in instead of joining the
    /// one left waiting, for each shape a channel's login takes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_press_after_the_browser_tab_closed_starts_a_fresh_sign_in() {
        // One test with its own plugin name: a waiting sign-in is kept per plugin
        // name for the whole process, so any other test's press would end it.
        let shapes = [
            // Every failure caught and returned as a refusal.
            ("bluesky", format!(r#"{BROWSER_SIGN_IN}
                try {{
                    const s = await startBrowserSignIn();
                    const reply = await s.open(authorize(s));
                    return {{ status: "completed", account: {{ id: "did:" + reply.code }} }};
                }} catch (e) {{ return {{ success: false, message: e.message }}; }}"#)),
            // A cancelled wait answered as a cancelled sign-in.
            ("threads", format!(r#"{BROWSER_SIGN_IN}
                try {{
                    const s = await startBrowserSignIn();
                    const reply = await s.open(authorize(s));
                    return {{ status: "completed", account: {{ id: "threads/" + reply.code }} }};
                }} catch (e) {{
                    if (e.name === "SignInCancelled") return {{ status: "cancelled" }};
                    return {{ status: "failed", message: e.message }};
                }}"#)),
            // A cancellation caught, then a page of its own that waits on the person.
            ("lingers", format!(r#"{BROWSER_SIGN_IN}
                try {{
                    const s = await startBrowserSignIn();
                    const reply = await s.open(authorize(s));
                    return {{ status: "completed", account: {{ id: "lingers/" + reply.code }} }};
                }} catch (e) {{
                    await globalThis.__TAURI__.core.invoke("set_action_panel_html", {{ html: "<p>Stopped</p>" }});
                    return await new Promise(() => {{}});
                }}"#)),
            // A server picked on moss's own page first, then the browser.
            ("mastodon", format!(r#"{BROWSER_SIGN_IN} {PICKS_A_SERVER}
                try {{
                    const s = await startBrowserSignIn();
                    const reply = await s.open(authorize(s));
                    return {{ status: "completed", account: {{ id: server + "/" + reply.code }} }};
                }} catch (e) {{
                    if (e.name === "SignInCancelled") return {{ status: "cancelled" }};
                    return {{ status: "failed", message: e.message }};
                }}"#)),
        ];
        for (shape, login) in shapes {
            let (_dir, project) = folder_with_plugin("browsing", &format!("async login(ctx) {{ {login} }}"));
            let (manager, plugin, desk) = manager_of_at_desk(&project, "browsing");
            let ctx = context(&project);
            let picks = shape == "mastodon";
            let browsed = |n: usize| {
                let desk = desk.clone();
                async move {
                    for _ in 0..500 {
                        if let Some(url) = desk.browsed.lock().unwrap().get(n - 1) {
                            return url::Url::parse(url).unwrap().query_pairs().find(|(k, _)| k == "redirect_uri").map(|(_, v)| v.into_owned()).unwrap();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    panic!("{shape}: sign-in {n} never opened the browser");
                }
            };
            let pick = |n: usize| {
                let desk = desk.clone();
                async move {
                    if !picks {
                        return;
                    }
                    for _ in 0..500 {
                        if desk.pages.lock().unwrap().len() >= n {
                            desk.fire("picker:continue", r#"{"server":"social.example.net"}"#);
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    panic!("{shape}: sign-in {n} never opened its picker");
                }
            };
            let presses = async {
                let (first, (abandoned, second)) = tokio::join!(manager.sign_in(&plugin, &ctx), async {
                    pick(1).await;
                    let abandoned = browsed(1).await;
                    let (second, ()) = tokio::join!(manager.sign_in(&plugin, &ctx), async {
                        pick(2).await;
                        let redirect = browsed(2).await;
                        let client = crate::system::proxy_reqwest::client_builder().no_proxy().build().unwrap();
                        client.get(format!("{redirect}?code=again")).send().await.unwrap_or_else(|e| panic!("{shape}: the second sign-in's redirect: {e:?}"));
                    });
                    (abandoned, second)
                });
                (first, abandoned, second)
            };
            let (first, abandoned, second) = tokio::time::timeout(std::time::Duration::from_secs(20), presses)
                .await
                .unwrap_or_else(|_| panic!("{shape}: the second press joined the sign-in left in the closed tab"));
            let id = match shape { "mastodon" => "social.example.net/again".to_string(), "bluesky" => "did:again".to_string(), _ => format!("{shape}/again") };
            assert_eq!(second.unwrap().outcome, completed(&id, None), "{shape}");
            assert!(!matches!(first.map(|s| s.outcome), Ok(LoginOutcome::Completed(_))), "{shape}: the abandoned sign-in finishes nothing");
            let port = abandoned.trim_start_matches("http://").split('/').next().unwrap().to_string();
            assert!(tokio::net::TcpStream::connect(&port).await.is_err(), "{shape}: the abandoned sign-in stopped listening");
            assert_eq!(account_binding(&project, "browsing").and_then(|b| b.id), Some(id), "{shape}");
        }
    }

    /// A channel that declares its one hidden secret keeps everything a
    /// sign-in learns inside it; a key per server is refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_sign_in_may_write_only_the_secret_its_manifest_declares() {
        let (_dir, project) = folder_with_channel(r#"
            await globalThis.__TAURI__.core.invoke("set_plugin_secret", { key: "app-social.example.net", value: "client" });
            return { status: "completed", account: { id: "s/1" } };"#);
        write(&project, ".moss/plugins/social/manifest.json", r#"{"name":"social","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{"channel":{"login":true,
            "setup":{"settings":[{"key":"session","type":"secret","label":"Session","hidden":true}]}}}}"#);
        let err = try_sign_in(&project).await.unwrap_err();
        assert!(err.contains("does not declare 'app-social.example.net' as a hidden `secret` setting"), "{err}");
        assert_eq!(account_binding(&project, "social"), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_sign_in_keeps_what_it_learns_in_its_declared_hidden_secret() {
        let (_dir, project) = folder_with_channel(r#"
            await globalThis.__TAURI__.core.invoke("set_plugin_secret", { key: "session", value: "client" });
            return { status: "completed", account: { id: "s/1" } };"#);
        write(&project, ".moss/plugins/social/manifest.json", r#"{"name":"social","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{"channel":{"login":true,
            "setup":{"settings":[{"key":"session","type":"secret","label":"Session","hidden":true}]}}}}"#);
        sign_in(&project).await;
        let store = crate::identity::secrets::SecretStore::require("test").unwrap();
        let scope = crate::identity::keystore::Scope::plugin("social").unwrap();
        assert_eq!(store.get(&scope, "session").unwrap().as_deref(), Some("client"));
        store.remove(&scope, "session").unwrap();
    }
}
