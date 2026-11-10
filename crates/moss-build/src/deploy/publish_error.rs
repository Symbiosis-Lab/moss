//! Why a publish or a site registration failed, kept typed until the surface
//! that shows it puts it in words.

use std::path::Path;

/// A failed publish or registration. The refusals a person can lift are kinds,
/// so each surface says what to do in its own terms: the terminal names the
/// flag, the app opens its sign-in or its name field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The hosting service doesn't know this folder's sign-in, or the folder
    /// lost its key. `host` is the site's address once it has one, else the
    /// hosting service's.
    NotSignedIn { host: String },
    /// The address is taken, or this folder can't claim it.
    NameTaken { address: String },
    /// Anything else, already a sentence.
    Other(String),
}

impl From<String> for PublishError {
    fn from(text: String) -> Self {
        Self::Other(text)
    }
}

impl PublishError {
    /// The terminal's words: each refusal names the flag that lifts it.
    pub fn cli_text(&self, folder: &Path) -> String {
        match self {
            Self::NotSignedIn { host } => crate::identity::verify_email::not_signed_in(folder, host),
            Self::NameTaken { address } => format!(
                "{address} is taken, or this folder can't claim it. Choose another name: moss deploy {} --site-id=<name>",
                folder.display()
            ),
            Self::Other(text) => text.clone(),
        }
    }

    /// The same in the app's language, naming no command: what the publish's
    /// Failed stage says on every surface.
    pub fn plain_text(&self) -> String {
        self.plain_text_in(crate::i18n::app_language())
    }

    fn plain_text_in(&self, lang: crate::i18n::AppLanguage) -> String {
        use crate::infra::app_advisory::fmt_in;
        match self {
            Self::NotSignedIn { host } => fmt_in(lang, "not_signed_in", &[("host", host)]),
            Self::NameTaken { address } => fmt_in(lang, "name_taken", &[("address", address)]),
            Self::Other(text) => text.clone(),
        }
    }
}

/// The plain words: what the publish guard's Failed stage shows.
impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.plain_text())
    }
}

#[cfg(test)]
#[path = "publish_error_tests.rs"]
mod tests;
