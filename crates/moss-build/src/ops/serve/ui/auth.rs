use std::path::Path;

pub struct SignIn {
    pub code: String,
}

impl SignIn {
    /// The code rides in the fragment, which never reaches a server or a log.
    pub fn url_path(&self) -> String {
        format!("/__moss/sign-in#code={}", self.code)
    }
}

#[derive(Debug, PartialEq)]
pub enum SignInError {
    Unauthorized,
    Unavailable,
}

/// The UI origin knows only this contract; hosts choose the adapter.
pub trait UiAuth: Send + Sync {
    fn validate_location(&self, served: &Path) -> Result<(), String>;
    fn mint_sign_in(&self) -> Result<SignIn, String>;
    fn sign_in(&self, code: &str, user_agent: &str) -> Result<String, SignInError>;
    fn admits(&self, secret: &str) -> bool;
    /// Recheck an existing stream without extending its inactivity deadline.
    fn session_active(&self, secret: &str) -> bool;
    fn sign_out(&self, secret: &str) -> Result<(), String>;
    fn list_sessions(&self) -> Result<Vec<super::sessions::SessionInfo>, String>;
    fn revoke_session(&self, id: &str) -> Result<(), String>;
    fn revoke_all(&self) -> Result<(), String>;
}

#[derive(Clone)]
pub(crate) struct BrowserSession {
    pub auth: std::sync::Arc<dyn UiAuth>,
    pub secret: String,
}

impl BrowserSession {
    pub fn admits(&self) -> bool { self.auth.session_active(&self.secret) }
}
