//! Vault identity: the keypair vocabulary, the on-disk identity service, and
//! the file-based keystore. The OS keyring and the Tauri commands stay app-side
//! and re-export these. Request signing is no longer among them — it is seta's
//! wire format, and ADR-078 moved it to `seta::signing` in this crate.
pub mod keypair;
pub mod keystore;
pub mod secrets;
pub mod service;

pub use keypair::Identity;
