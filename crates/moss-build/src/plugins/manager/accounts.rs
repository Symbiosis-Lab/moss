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
/// identifier; `name` is only shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AccountBinding {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
                match text(id, false) {
                    Some(id) => Self::Completed(AccountBinding { id, name: text(account.and_then(|a| a.get("name")), false) }),
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

fn recorded(project_path: &str, plugin: &str) -> Option<AccountBinding> {
    match slot(project_path, plugin)? {
        Slot::Bound(binding) => Some(binding),
        Slot::SignedOut => None,
    }
}

/// The account `plugin` is signed in to in this folder, if any.
pub fn account_binding(project_path: &str, plugin: &str) -> Option<BoundAccount> {
    recorded(project_path, plugin).map(BoundAccount::from).or_else(|| {
        let config = crate::plugins::discovery::get_plugin_config(project_path, plugin).ok()?;
        let bound = BoundAccount { id: text(config.get("boundUserId"), true), name: text(config.get("boundUserName"), true) };
        (bound.id.is_some() || bound.name.is_some()).then_some(bound)
    })
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
    let config = Path::new(project_path).join(".moss").join("plugins").join(plugin).join("config.json");
    let forgot_legacy = forget_legacy_binding(&config)?;
    if recorded(project_path, plugin).is_none() && !forgot_legacy {
        return Ok(());
    }
    crate::vault::deployment_state::update_state(project_path, |root| {
        crate::vault::config::subtable(root, ACCOUNTS)?.insert(plugin.to_string(), toml::Value::String(SIGNED_OUT.into()));
        Ok(())
    })
}

/// Whether `config` held a legacy binding, now removed.
fn forget_legacy_binding(config: &Path) -> Result<bool, String> {
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
/// session this computer holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum AccountState {
    SignedIn { name: String },
    /// The folder is bound to `name`, and this computer has no session for it.
    SignedOut { name: String },
    /// This computer could not read its session: `why` is the channel's error,
    /// or none when no runtime here may run the channel. `name` is the
    /// binding's, if the folder has one.
    Unread { name: Option<String>, why: Option<String> },
    NoAccount,
}

/// The row for a channel whose session was not read, from the binding alone.
pub fn account_unread(project_path: &str, plugin: &str, why: Option<String>) -> AccountState {
    AccountState::Unread { name: account_binding(project_path, plugin).and_then(|b| b.name.or(b.id)), why }
}

impl PluginManager {
    /// The row a channel's settings show. The binding syncs with the folder and
    /// the session belongs to this computer, so a channel's `copy_account` has
    /// the last word, and a channel without one is taken at its binding.
    pub async fn account_state(&self, plugin: &str) -> AccountState {
        let bound = account_binding(&self.project_path, plugin);
        let shown = |b: &BoundAccount| b.name.clone().or_else(|| b.id.clone());
        match self.copy_account(plugin).await {
            super::copies::Account::Known { id, name } => {
                let same = bound.as_ref().filter(|b| b.id.as_deref() == Some(id.as_str()));
                AccountState::SignedIn { name: name.or_else(|| same.and_then(shown)).unwrap_or(id) }
            }
            super::copies::Account::SignedOut => bound.as_ref().and_then(shown).map_or(AccountState::NoAccount, |name| AccountState::SignedOut { name }),
            super::copies::Account::Unknown(why) => account_unread(&self.project_path, plugin, Some(why)),
            super::copies::Account::Unsaid => bound.as_ref().and_then(shown).map_or(AccountState::NoAccount, |name| AccountState::SignedIn { name }),
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
    use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};
    use serde_json::json;

    fn completed(id: &str, name: Option<&str>) -> LoginOutcome {
        LoginOutcome::Completed(AccountBinding { id: id.into(), name: name.map(str::to_string) })
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
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
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
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: Some("@new".into()) }).unwrap();
        assert_eq!(account_binding(project, "social").and_then(|b| b.name), Some("@new".into()));
    }

    /// A channel whose bundle is `body` (the members of its global), with one
    /// hidden secret, `token`.
    fn folder_with_hooks(body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap().to_string();
        write(&project, ".moss/plugins/social/manifest.json", r#"{"name":"social","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{"channel":{"login":true,
            "setup":{"settings":[{"key":"token","type":"secret","label":"Token","hidden":true}]}}}}"#);
        write(&project, ".moss/plugins/social/main.bundle.js", &format!("var Social = {{ {body} }};"));
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
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()) }).unwrap();
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
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
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
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
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
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
        unbind_account(project, "social").unwrap();
        assert!(!plugin_needs_connection(project, "social"), "a build after Sign out does not open the sign-in");
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None }).unwrap();
        assert!(!marked_signed_out(project, "social"), "signing in again lifts it");
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
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
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
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()) }).unwrap();
        let before = std::fs::read_to_string(dir.path().join(".moss/state.toml")).unwrap();
        unbind_account(project, "social").unwrap();
        write(project, ".moss/state 2.toml", &before);
        crate::vault::synced_siblings::reconcile_synced_state(&crate::moss_paths::MossPaths::new(dir.path()));
        assert!(!dir.path().join(".moss/state 2.toml").exists(), "the sibling was folded in");
        assert_eq!(account_binding(project, "social"), None);
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None }).unwrap();
        assert_eq!(account_binding(project, "social").and_then(|b| b.id), Some("acct:2".into()), "signing in again replaces the mark");
    }

    #[test]
    fn a_conflicted_copy_from_after_a_sign_in_does_not_sign_the_folder_out() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        bind_account(project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
        unbind_account(project, "social").unwrap();
        let signed_out_copy = std::fs::read_to_string(dir.path().join(".moss/state.toml")).unwrap();
        bind_account(project, "social", &AccountBinding { id: "acct:2".into(), name: None }).unwrap();
        write(project, ".moss/state 2.toml", &signed_out_copy);
        crate::vault::synced_siblings::reconcile_synced_state(&crate::moss_paths::MossPaths::new(dir.path()));
        assert_eq!(account_binding(project, "social").and_then(|b| b.id), Some("acct:2".into()));
    }

    #[tokio::test]
    async fn an_older_login_after_a_sign_out_lifts_it() {
        let (_dir, project) = folder_with_channel(r#"await globalThis.__TAURI__.core.invoke("write_plugin_file", { relativePath: "config.json", content: JSON.stringify({ boundUserName: "@ada" }) }); return { success: true };"#);
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: None }).unwrap();
        unbind_account(&project, "social").unwrap();
        assert!(sign_in(&project).await.newly_bound);
        assert!(!marked_signed_out(&project, "social"));
    }

    async fn state_with(copy_account: &str, bound: Option<&str>) -> AccountState {
        let (_dir, project) = folder_with_hooks(&format!("async copy_account(q) {{ return {copy_account}; }}"));
        if let Some(name) = bound {
            bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some(name.into()) }).unwrap();
        }
        manager(&project).0.account_state("social").await
    }

    #[tokio::test]
    async fn the_account_row_reads_the_binding_against_this_computers_session() {
        let signed_out = r#"{ success: true, copy: { signed_out: true } }"#;
        assert_eq!(state_with(signed_out, Some("@ada")).await, AccountState::SignedOut { name: "@ada".into() });
        assert_eq!(state_with(signed_out, None).await, AccountState::NoAccount);
        let same = r#"{ success: true, copy: { account: "acct:1" } }"#;
        assert_eq!(state_with(same, Some("@ada")).await, AccountState::SignedIn { name: "@ada".into() });
        let other = r#"{ success: true, copy: { account: "acct:9" } }"#;
        assert_eq!(state_with(other, Some("@ada")).await, AccountState::SignedIn { name: "acct:9".into() }, "another account's session is not shown under this binding's name");
        let (_dir, project) = folder_with_hooks("");
        bind_account(&project, "social", &AccountBinding { id: "acct:1".into(), name: Some("@ada".into()) }).unwrap();
        assert_eq!(manager(&project).0.account_state("social").await, AccountState::SignedIn { name: "@ada".into() }, "no copy_account: the binding");
        let failed = r#"{ success: false, error: "rate limited" }"#;
        assert!(matches!(state_with(failed, Some("@ada")).await, AccountState::Unread { name: Some(n), why: Some(_) } if n == "@ada"), "a failed read is neither signed in nor signed out");
        assert!(matches!(state_with(failed, None).await, AccountState::Unread { name: None, why: Some(_) }));
    }
}
