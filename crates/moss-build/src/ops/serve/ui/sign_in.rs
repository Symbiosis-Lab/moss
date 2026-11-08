//! Single-use sign-in links; neither codes nor URLs are logged.
use super::{
    auth::{SignIn, SignInError, UiAuth},
    secrets,
    sessions::{SessionInfo, SessionStore},
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// The one live code's hash and expiry. Codes carry 256 random bits, so
/// guessing needs no throttle.
pub struct SignInLink {
    code: Mutex<Option<(Vec<u8>, u64)>>,
    sessions: SessionStore,
}

impl SignInLink {
    pub fn new(session_file: PathBuf, site_root: &Path) -> Result<Self, String> {
        let sessions = SessionStore::new(session_file)?;
        sessions.validate_location(site_root)?;
        Ok(Self {
            code: Mutex::default(),
            sessions,
        })
    }
}

impl UiAuth for SignInLink {
    fn validate_location(&self, served: &Path) -> Result<(), String> {
        self.sessions.validate_location(served)
    }

    fn mint_sign_in(&self) -> Result<SignIn, String> {
        let code = secrets::random()?;
        let expires = secrets::now() + 600;
        // A new host-issued link retires the previous link.
        *self.code.lock().unwrap() = Some((secrets::hash(&code), expires));
        Ok(SignIn { code })
    }

    fn sign_in(&self, code: &str, user_agent: &str) -> Result<String, SignInError> {
        let mut live = self.code.lock().unwrap();
        let valid = live
            .as_ref()
            .is_some_and(|(hash, expires)| *hash == secrets::hash(code) && *expires > secrets::now());
        if !valid {
            return Err(SignInError::Unauthorized);
        }
        *live = None;
        self.sessions.open(user_agent).map_err(|_| SignInError::Unavailable)
    }

    fn admits(&self, secret: &str) -> bool {
        self.sessions.admits(secret, true).unwrap_or(false)
    }
    fn session_active(&self, secret: &str) -> bool {
        self.sessions.admits(secret, false).unwrap_or(false)
    }
    fn sign_out(&self, secret: &str) -> Result<(), String> {
        self.sessions.remove_secret(secret)
    }
    fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        self.sessions.list()
    }
    fn revoke_session(&self, id: &str) -> Result<(), String> {
        self.sessions.revoke(id)
    }
    fn revoke_all(&self) -> Result<(), String> {
        self.sessions.revoke_all()
    }
}

#[cfg(test)]
impl SignInLink {
    pub(super) fn expire(&self, code: &str) {
        *self.code.lock().unwrap() = Some((secrets::hash(code), 0));
    }
}
