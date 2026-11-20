//! Your own server's settings: where it is (`[deploy.server]` in the
//! folder's config, synced with the folder, nothing secret) and the key moss
//! signs in to it with (kept on this computer only).

use std::path::Path;
use std::sync::Arc;

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{PrivateKey, PublicKey};
use toml::Value;

use crate::identity::keystore::Scope;
use crate::identity::secrets::SecretStore;
use crate::vault::config::{edit_config, subtable};

/// A folder on a server: `sftp://user@host:port/folder`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub user: String,
    pub host: String,
    pub port: u16,
    /// An absolute path on the server.
    pub folder: String,
}

impl Server {
    /// Read `sftp://user@host[:port]/folder`, or the `user@host:/folder` a
    /// developer copies from scp. An scp path without a leading `/` is
    /// relative to the home folder, which the URL form cannot say, so it is
    /// refused rather than read as a different folder.
    pub fn parse(text: &str) -> Result<Server, String> {
        let text = text.trim();
        let wrong = || format!("{text:?} isn't a server address like deploy@example.com:/var/www/html");
        let url = text.strip_prefix("sftp://");
        let (user, rest) = url.unwrap_or(text).split_once('@').ok_or_else(wrong)?;
        let (host, rest) = match rest.strip_prefix('[') {
            Some(bracketed) => bracketed.split_once(']').ok_or_else(wrong)?,
            None => rest.split_at(rest.find([':', '/']).ok_or_else(wrong)?),
        };
        let (port, folder) = if url.is_some() {
            let (port, path) = rest.split_once('/').ok_or_else(wrong)?;
            let port = match port.strip_prefix(':') {
                Some(p) => p.parse::<u16>().map_err(|_| wrong())?,
                None if port.is_empty() => 22,
                None => return Err(wrong()),
            };
            (port, format!("/{path}"))
        } else {
            (22, rest.strip_prefix(':').ok_or_else(wrong)?.to_string())
        };
        if !folder.starts_with('/') {
            return Err(format!("{text:?} names a folder relative to the home folder; give its full path, like /home/deploy/www"));
        }
        if user.is_empty() || host.is_empty() || user.contains(['/', ':']) {
            return Err(wrong());
        }
        Ok(Server { user: user.into(), host: host.into(), port, folder })
    }

    /// The one shape the config stores.
    pub fn to_url(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let port = if self.port == 22 { String::new() } else { format!(":{}", self.port) };
        format!("sftp://{}@{host}{port}{}", self.user, self.folder)
    }
}

/// `[deploy.server]` as the folder holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub server: Server,
    /// The host key's fingerprint, pinned when the person trusted it.
    pub host_key: Option<String>,
}

/// The folder's server settings, or `None` before an address is set.
pub fn settings(folder: &Path) -> Result<Option<Settings>, String> {
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy())?;
    let Some(to) = config.section(&["deploy", "server", "to"]).and_then(Value::as_str) else {
        return Ok(None);
    };
    let host_key = config.section(&["deploy", "server", "host_key"]).and_then(Value::as_str).map(str::to_string);
    Ok(Some(Settings { server: Server::parse(to)?, host_key }))
}

/// Publish the folder to `server` from now on. A different host forgets the
/// pinned key, which was a fact about the old one.
pub fn save_server(folder: &Path, server: &Server) -> Result<(), String> {
    let same_host = settings(folder).ok().flatten().is_some_and(|s| s.server.host == server.host && s.server.port == server.port);
    edit_config(folder, |root| {
        let hook = Value::String(crate::config::deployment::SERVER_TARGET_ID.into());
        subtable(root, "hooks")?.insert("deploy".into(), hook);
        let table = subtable(subtable(root, "deploy")?, "server")?;
        table.insert("to".into(), Value::String(server.to_url()));
        if !same_host {
            table.remove("host_key");
        }
        Ok(())
    })
}

/// Pin the server's host key.
pub fn save_host_key(folder: &Path, fingerprint: &str) -> Result<(), String> {
    edit_config(folder, |root| {
        let table = subtable(subtable(root, "deploy")?, "server")?;
        table.insert("host_key".into(), Value::String(fingerprint.into()));
        Ok(())
    })
}

/// The site's publisher id: the same on every computer publishing the
/// folder (it syncs with it), minted the first time it is asked for. The
/// destination's record names it, and moss's key for the folder is filed
/// under it.
pub fn publisher(folder: &Path) -> Result<String, String> {
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy())?;
    if let Some(id) = config.section(&["deploy", "publisher"]).and_then(Value::as_str) {
        return Ok(id.to_string());
    }
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    let id = hex::encode(bytes);
    edit_config(folder, |root| {
        subtable(root, "deploy")?.insert("publisher".into(), Value::String(id.clone()));
        Ok(())
    })?;
    Ok(id)
}

/// The keys moss signs in to servers with, in their own folder under this
/// computer's data folder: never synced or roamed, so a key made on one
/// computer never leaves it, and apart from the store plugins read their
/// secrets from, so no plugin can name its way to one.
pub struct Keys {
    store: SecretStore,
}

const KEY: &str = "key";

impl Keys {
    pub fn on_this_computer() -> Result<Keys, String> {
        let roots = crate::infra::app_data::AppRoots::running().ok_or("this computer has no data folder")?;
        Ok(Keys::in_folder(&roots.local))
    }

    /// The keys under `app_data`, the folder plugin secrets also live under.
    pub fn in_folder(app_data: &Path) -> Keys {
        Keys { store: SecretStore::in_app_data(&app_data.join("server-keys")) }
    }

    fn scope(publisher: &str) -> Result<Scope, String> {
        Scope::plugin(publisher).map_err(|e| format!("{e:?}"))
    }

    /// The folder's key, or `None` when this computer has none for it yet.
    pub fn key(&self, publisher: &str) -> Result<Option<Arc<PrivateKey>>, String> {
        let Some(seed) = self.store.get(&Self::scope(publisher)?, KEY).map_err(|e| format!("{e:?}"))? else {
            return Ok(None);
        };
        let seed: [u8; 32] = hex::decode(seed).ok().and_then(|b| b.try_into().ok()).ok_or("the stored key is damaged")?;
        Ok(Some(Arc::new(PrivateKey::from(Ed25519Keypair::from_seed(&seed)))))
    }

    /// The folder's key, made now if this computer has none.
    pub fn key_or_new(&self, publisher: &str) -> Result<Arc<PrivateKey>, String> {
        if let Some(key) = self.key(publisher)? {
            return Ok(key);
        }
        let mut seed = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut seed);
        self.store.set(&Self::scope(publisher)?, KEY, &hex::encode(seed)).map_err(|e| format!("{e:?}"))?;
        Ok(Arc::new(PrivateKey::from(Ed25519Keypair::from_seed(&seed))))
    }

    /// The line to add to the server's `authorized_keys`, labelled `label`.
    pub fn public_line(&self, publisher: &str, label: &str) -> Result<String, String> {
        let mut public: PublicKey = self.key_or_new(publisher)?.public_key().clone();
        public.set_comment(label);
        public.to_openssh().map_err(|e| e.to_string())
    }
}
