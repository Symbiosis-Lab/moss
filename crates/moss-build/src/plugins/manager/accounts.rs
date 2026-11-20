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

fn recorded(project_path: &str, plugin: &str) -> Option<AccountBinding> {
    let path = Path::new(project_path).join(".moss").join("state.toml");
    let text = crate::build::site_config::read_managed_toml(&path).ok()??;
    let root: toml::Table = toml::from_str(&text).ok()?;
    let binding: AccountBinding = root.get(ACCOUNTS)?.get(plugin)?.clone().try_into().ok()?;
    (!binding.id.trim().is_empty()).then_some(binding)
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

impl PluginManager {
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
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join(".moss/plugins/social");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), r#"{"name":"social","version":"0.1.0","entry":"main.bundle.js",
            "global_name":"Social","contributes":{"channel":{"login":true}}}"#).unwrap();
        std::fs::write(plugin.join("main.bundle.js"), format!("var Social = {{ async login(ctx) {{ {login_body} }} }};")).unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        (dir, path)
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
}
