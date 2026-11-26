//! Which account each channel is signed in to on this computer.
//!
//! A channel's session lives in this computer's webview cookies or secret
//! store, not in the site folder, so its record lives beside the secret store
//! in the app-support directory and never in a synced folder. It is written
//! from definite answers only: a sign-in that finished, or the `copy_account`
//! answer a run already asks for. Reading it never asks the channel, because
//! asking can be a billed call.

use crate::identity::keystore::EntryName;
use crate::plugins::manager::copies::Account;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The store's directory name under the app-support directory.
pub const SIGN_INS_DIRNAME: &str = "sign-ins";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum SignInState {
    Ready,
    /// The channel answered that it has no usable session; the person signs
    /// in again. The last account stays, so it can be named.
    NeedsSignIn,
}

/// One channel's sign-in on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SignIn {
    /// The id copies are kept with (`copy_account`'s `account`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The account as a person knows it, when the channel says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub state: SignInState,
    /// When this record last changed (RFC 3339).
    pub at: String,
}

pub struct SignIns {
    /// `<app-data>/sign-ins`.
    root: PathBuf,
}

impl SignIns {
    pub fn in_app_data(app_data_dir: &Path) -> Self {
        Self { root: app_data_dir.join(SIGN_INS_DIRNAME) }
    }

    /// The store beside the secret store; `None` when the platform has no data
    /// directory.
    pub fn app_default() -> Option<Self> {
        crate::infra::app_data::credential_root().map(|d| Self::in_app_data(&d))
    }

    fn path(&self, channel: &str) -> Result<PathBuf, String> {
        let entry = EntryName::new(channel).map_err(|_| format!("'{channel}' is not a channel name"))?;
        Ok(self.root.join(format!("{}.json", entry.file)))
    }

    /// The channel's record; `None` when it has never answered on this computer.
    pub fn get(&self, channel: &str) -> Option<SignIn> {
        let bytes = std::fs::read(self.path(channel).ok()?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Record what `channel` answered. Only a definite answer writes:
    /// `Unknown` (a failed or unavailable read) and `Unsaid` leave the record
    /// as it was.
    pub fn record(&self, channel: &str, account: &Account) -> Result<(), String> {
        let previous = self.get(channel);
        let next = match account {
            Account::Known { id, name } => SignIn { account: Some(id.clone()), name: name.clone(), state: SignInState::Ready, at: now() },
            Account::SignedOut => SignIn {
                account: previous.as_ref().and_then(|p| p.account.clone()),
                name: previous.as_ref().and_then(|p| p.name.clone()),
                state: SignInState::NeedsSignIn,
                at: now(),
            },
            Account::Unsaid | Account::Unknown(_) => return Ok(()),
        };
        if previous.as_ref().is_some_and(|p| p.account == next.account && p.name == next.name && p.state == next.state) {
            return Ok(());
        }
        crate::infra::atomic_write::write_json_atomic(&self.path(channel)?, &next)
    }

    /// The person signed out of `channel` on this computer: no record, so the
    /// channel reads as never signed in.
    pub fn forget(&self, channel: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(channel)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("could not forget the {channel} sign-in: {e}")),
            _ => Ok(()),
        }
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_never_opens_the_real_store() {
        let root = SignIns::app_default().unwrap().root;
        assert!(root.starts_with(std::env::temp_dir()), "{}", root.display());
    }

    fn known(id: &str, name: Option<&str>) -> Account {
        Account::Known { id: id.into(), name: name.map(str::to_string) }
    }

    #[test]
    fn signing_out_forgets_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let store = SignIns::in_app_data(dir.path());
        store.forget("acct").unwrap();
        store.record("acct", &known("acct:1", Some("@ada"))).unwrap();
        store.forget("acct").unwrap();
        assert_eq!(store.get("acct"), None);
    }

    #[test]
    fn a_known_account_is_ready_and_signed_out_keeps_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = SignIns::in_app_data(dir.path());
        store.record("acct", &known("acct:1", Some("@ada"))).unwrap();
        assert_eq!(store.get("acct").map(|s| (s.state, s.name)), Some((SignInState::Ready, Some("@ada".into()))));
        store.record("acct", &Account::SignedOut).unwrap();
        let after = store.get("acct").unwrap();
        assert_eq!((after.state, after.account.as_deref(), after.name.as_deref()), (SignInState::NeedsSignIn, Some("acct:1"), Some("@ada")));
    }

    #[test]
    fn a_failed_or_unsaid_answer_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = SignIns::in_app_data(dir.path());
        store.record("acct", &Account::Unknown("offline".into())).unwrap();
        store.record("acct", &Account::Unsaid).unwrap();
        assert!(store.get("acct").is_none());
        store.record("acct", &known("acct:1", None)).unwrap();
        store.record("acct", &Account::Unknown("no cookie jar here".into())).unwrap();
        assert_eq!(store.get("acct").unwrap().state, SignInState::Ready);
    }

    #[test]
    fn a_channel_name_cannot_leave_the_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(SignIns::in_app_data(dir.path()).record("../x", &known("1", None)).is_err());
    }
}
