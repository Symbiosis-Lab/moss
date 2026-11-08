//! Isolated UI origin and host-managed browser authentication.
mod assets;
pub mod auth;
pub mod client;
pub use super::trust_boundary::PublicOrigin;
mod carrier;
mod listener;
pub(crate) mod routes;
mod secrets;
pub mod sessions;
pub mod sign_in;

#[cfg(test)]
pub(crate) use listener::router;
pub(crate) use listener::{bind, drive};
pub use assets::UiAsset;
pub use listener::{UiOriginConfig, UiPort};

#[cfg(test)]
mod listener_tests;
#[cfg(test)]
mod tests;
