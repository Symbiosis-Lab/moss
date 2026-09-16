//! The three credential arms of the engine seam, and the custody rule they
//! encode (ADR-072 §3, as amended 2026-08-30).
//!
//! A secret is the keystore one shape down: a token somebody else issued the
//! user, which the plugin does read because it puts it in a header. The scope is
//! the calling plugin, resolved from the dispatch seam and never from an
//! argument, so no plugin can name another's — which is why the read arm needs
//! no gate at all.
//!
//! The rule is NOT "moss is the only writer". That was too strong, and it cost
//! something real: the one first-party plugin holding a live user credential is
//! not a deploy target, so it kept an access token in plaintext under
//! `.moss/plugins/`, which is inside the user's repo and is not gitignored. What
//! stops credential phishing is that a plugin can never DRAW a credential input
//! — the modal is moss's, the fields come from the manifest, the plugin supplies
//! ids and sentences and no pixels. Depositing what an authenticated flow
//! already handed it is a different act, and [`set`] allows it. ADR-032 settled
//! the same question for signing keys, as custody rather than restriction.
//!
//! The line runs between KEYS, not between plugins. A key the manifest declared
//! as one moss asks the user for stays moss's to write; every other key in the
//! plugin's own scope is the plugin's. See [`require_plugin_owned_key`].

use super::{keystore_caller_scope, req_str, reply_json, EngineHost};
use crate::identity::secrets::SecretStore;

pub(super) fn get(
    host: &EngineHost<'_>,
    args: &serde_json::Value,
) -> std::result::Result<String, String> {
    let scope = keystore_caller_scope(host, "get_plugin_secret")?;
    let key = req_str(args, "key")?;
    let store = SecretStore::require("get_plugin_secret")?;
    let v = store.get(&scope, &key).map_err(|e| format!("secrets: {e:?}"))?;
    reply_json(&v)
}

/// The write arm. Scoped to the KEY, not to the plugin.
///
/// It was gated on `Capability::Login`, which is the wrong shape: the question
/// is not whether the plugin runs a login somewhere, it is whether THIS slot is
/// one a login filled. A plugin holding a flow-obtained token and a pasted one
/// declares `login` either way, and the capability gate let it overwrite both.
///
/// So the refusal reads the manifest's own answer. A key the manifest declared
/// under `setup.credentials`, or as a `secret` in `config_schema`, is one moss
/// asked the user for in moss's own modal; the plugin may not write it. Every
/// other key in its scope is its own, and depositing there is the act ADR-072
/// §3 allows. The scope still comes from the dispatch seam, so this narrows
/// what a plugin may do to itself and takes nothing away from the guarantee
/// that it can never reach another plugin's store.
///
/// A plugin that wants a user-typed credential replaced has an arm for it:
/// `rejectSecret` erases and re-asks through moss's modal, so the replacement
/// is still something the user typed.
pub(super) fn set(
    host: &EngineHost<'_>,
    args: &serde_json::Value,
) -> std::result::Result<String, String> {
    let scope = keystore_caller_scope(host, "set_plugin_secret")?;
    let key = req_str(args, "key")?;
    require_plugin_owned_key(host, &key)?;
    let value = req_str(args, "value")?;
    SecretStore::require("set_plugin_secret")?
        .set(&scope, &key, &value)
        .map_err(|e| format!("secrets: {e:?}"))?;
    Ok("null".into())
}

/// Reject = erase AND re-ask. git-credential's `erase` is only the first half:
/// erasing alone means the publish fails now and the user finds out on their
/// own. So moss draws its own modal, carrying the plugin's `detail` sentence,
/// and this replies with the replacement (or null on cancel) — the plugin's
/// error path becomes a retry.
///
/// The wait is a PERSON's, so it is unbounded, which is `check_setup`'s
/// reasoning exactly. Headless there is nobody to ask: the erase stands and the
/// reply is null, because forgetting a dead token is right in a build too and
/// only the asking needs a window. Same for a key the manifest never declared —
/// moss has no reviewed words for that field, so it erases and asks nothing.
pub(super) async fn reject(
    host: &EngineHost<'_>,
    args: &serde_json::Value,
) -> std::result::Result<String, String> {
    let scope = keystore_caller_scope(host, "reject_plugin_secret")?;
    let key = req_str(args, "key")?;
    let detail = args.get("detail").and_then(|v| v.as_str()).map(String::from);
    SecretStore::require("reject_plugin_secret")?
        .remove(&scope, &key)
        .map_err(|e| format!("secrets: {e:?}"))?;
    let replacement = match (host.app, host.plugin) {
        (Some(app), Some(plugin)) => {
            app.prompt_credential(plugin, host.project_path, &key, detail).await?
        }
        _ => None,
    };
    reply_json(&replacement)
}

/// Refuse a write to a credential slot the plugin does not hold custody of.
///
/// Fail-closed the same way the `requires` gate is: a plugin missing from the
/// discovered snapshot has no declaration to read, so every key is refused
/// rather than treated as unclaimed.
///
/// Two refusals, one admission. A key the user fills in refuses outright —
/// substituting a token for the one the user believes is there is the phishing
/// shape the custody rule exists for. A key declared as a `hidden` secret
/// setting is declared custody, and admits. An UNDECLARED key takes the same
/// migration posture as the blanket `execute_binary` grant: a manifest that
/// declares no hidden secrets predates the contract and keeps writing with a
/// deprecation warning (the matters plugin deposits per-user `token_<name>`
/// keys no manifest can enumerate), while a manifest that has declared any is
/// held to its declaration.
pub(super) fn require_plugin_owned_key(
    host: &EngineHost<'_>,
    key: &str,
) -> std::result::Result<(), String> {
    let (plugin, declared) = super::grants::caller_declared(host, "set_plugin_secret")?;
    if declared.user_supplied_secrets.iter().any(|d| d == key) {
        return Err(format!(
            "'set_plugin_secret' refused: '{key}' is a credential the user gives moss \
directly, so plugin '{plugin}' may not write it. Use rejectSecret to have moss ask \
for a replacement."
        ));
    }
    if declared.plugin_owned_secrets.iter().any(|d| d == key) {
        return Ok(());
    }
    if declared.plugin_owned_secrets.is_empty() {
        log::warn!(
            target: "plugin",
            "plugin '{plugin}' deposits secret '{key}' without declaring custody — \
             declare it as a hidden `secret` setting in the manifest"
        );
        return Ok(());
    }
    Err(format!(
        "'set_plugin_secret' refused: plugin '{plugin}' does not declare '{key}' as a \
hidden `secret` setting, and its manifest declares custody of other keys — declare \
this one before depositing into it"
    ))
}
