//! Swift bindings for the shared JSON boundary.

use crate::Site;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, uniffi::Error)]
pub enum MossError {
    InvalidJson { message: String },
}

impl std::fmt::Display for MossError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for MossError {}

#[derive(uniffi::Object)]
pub struct MossSite {
    inner: Mutex<Site>,
}

impl MossSite {
    fn site(&self) -> MutexGuard<'_, Site> {
        // Notes change only after successful parsing; unwinding cannot leave a partial map.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[uniffi::export]
impl MossSite {
    #[uniffi::constructor]
    pub fn new(site_json: String) -> Result<Arc<Self>, MossError> {
        let site = Site::from_json(&site_json).map_err(|message| MossError::InvalidJson { message })?;
        Ok(Arc::new(Self { inner: Mutex::new(site) }))
    }

    pub fn check(&self, path: String, text: String) -> String {
        self.site().check(&path, &text)
    }

    pub fn embedded_notes(&self, path: String, text: String) -> String {
        self.site().embedded_notes(&path, &text)
    }

    pub fn set_notes(&self, notes_json: String) -> Result<(), MossError> {
        self.site().set_notes(&notes_json).map_err(|message| MossError::InvalidJson { message })
    }

    pub fn preview(&self, path: String, text: String, base_href: String) -> String {
        self.site().preview(&path, &text, &base_href)
    }
}

