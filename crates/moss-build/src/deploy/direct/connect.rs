//! Connect: sign in once as the person does with `ssh` (their key files and
//! agent, or the server's password, given once) and add moss's key for this
//! folder to the server's `~/.ssh/authorized_keys`. Publishing then signs in
//! with moss's key, and the check that follows is the proof the line took.
//!
//! Only the app's own Connect command and `moss deploy --connect` run this,
//! never a hook. Agent forwarding is never asked for.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::client::{Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey};
use russh::{MethodKind, MethodSet};
use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};

use super::check::{self, Checked};
use super::openssh::{Agent, Resolved, Ssh};
use super::server::{self, Keys};
use super::sftp::{self, Refusal, Session, Trust};

/// A server's password, held for one sign-in. It never prints, so it cannot
/// reach a log, an error report or a panic message.
pub struct Password(String);

impl Password {
    pub fn new(text: String) -> Password {
        Password(text)
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(…)")
    }
}

/// The most of the person's keys offered, under OpenSSH's default
/// `MaxAuthTries` of six, leaving a try for the password.
const MOST_KEYS: usize = 5;
const AUTHORIZED_KEYS: &str = ".ssh/authorized_keys";

/// Add moss's key for `folder` to its server, then check the folder as a
/// publish would.
pub async fn connect(folder: &Path, keys: &Keys, password: Option<Password>) -> Result<Checked, String> {
    connect_with(&Ssh::default(), folder, keys, password).await
}

pub(super) async fn connect_with(ssh_config: &Ssh, folder: &Path, keys: &Keys, password: Option<Password>) -> Result<Checked, String> {
    let settings = server::settings(folder)?.ok_or("Set the server first.")?;
    let publisher = server::publisher(folder)?;
    let line = keys.public_line(&publisher, &check::label(folder))?;
    let resolved = match sftp::resolve(ssh_config, &settings.server).await {
        Ok(resolved) => resolved,
        Err(refusal) => return check::refused(folder, keys, refusal, &settings.server),
    };
    let installed = match sftp::session(ssh_config, &resolved, settings.host_key.as_deref()).await {
        Err(refusal) => Err(refusal),
        Ok(Session { mut ssh, host_key }) => {
            if settings.host_key.as_deref() != Some(host_key.as_str()) {
                server::save_host_key(folder, &host_key)?;
            }
            match sign_in(&mut ssh, &resolved, password.as_ref()).await {
                Ok(()) => install(&ssh, &resolved.host, &line).await,
                Err(refusal) => Err(refusal),
            }
        }
    };
    match installed {
        Ok(()) => check::check_resolved(ssh_config, &resolved, folder, keys, None, &check::web_folders()).await,
        Err(refusal) => check::refused(folder, keys, refusal, &resolved.reached(&settings.server.folder)),
    }
}

/// A key of the person's to offer: one the agent holds, or an unencrypted
/// key file read for this sign-in only.
enum Offer {
    Agent(PublicKey),
    File(Arc<PrivateKey>),
}

impl Offer {
    fn public(&self) -> &PublicKey {
        match self {
            Offer::Agent(key) => key,
            Offer::File(key) => key.public_key(),
        }
    }
}

#[cfg(unix)]
type AgentClient = russh::keys::agent::client::AgentClient<tokio::net::UnixStream>;

#[cfg(unix)]
async fn agent(resolved: &Resolved) -> Option<AgentClient> {
    match &resolved.agent {
        Agent::Env => AgentClient::connect_env().await.ok(),
        Agent::Socket(path) => AgentClient::connect_uds(path).await.ok(),
        Agent::None => None,
    }
}

/// The keys `ssh` would offer, in its order: the agent's keys that a key
/// file names, then, unless `identities_only`, its other keys, then the key
/// files the agent does not hold, read unencrypted. Encrypted files and
/// security-key files are skipped; the agent signs for both.
fn offers(identity_files: &[PathBuf], identities_only: bool, held: &[PublicKey]) -> Vec<Offer> {
    let named: Vec<Option<PublicKey>> = identity_files.iter().map(|f| public_half(f)).collect();
    let is_named = |key: &PublicKey| named.iter().flatten().any(|n| n.key_data() == key.key_data());
    let is_held = |key: &PublicKey| held.iter().any(|h| h.key_data() == key.key_data());
    let (mut offers, others): (Vec<Offer>, Vec<Offer>) =
        held.iter().cloned().map(Offer::Agent).partition(|o| is_named(o.public()));
    if !identities_only {
        offers.extend(others);
    }
    for (file, public) in identity_files.iter().zip(&named) {
        if public.as_ref().is_some_and(is_held) {
            continue;
        }
        // OpenSSH's own format only; the agent signs every other.
        // allow:raw_read a key file in ~/.ssh, never a vault input
        let read = std::fs::read_to_string(file).ok().and_then(|text| PrivateKey::from_openssh(text).ok());
        if let Some(key) = read {
            if !key.is_encrypted() && !matches!(key.algorithm(), russh::keys::Algorithm::SkEd25519 | russh::keys::Algorithm::SkEcdsaSha2NistP256) {
                offers.push(Offer::File(Arc::new(key)));
            }
        }
    }
    let mut seen: Vec<PublicKey> = Vec::new();
    offers.retain(|o| {
        let new = !seen.iter().any(|s| s.key_data() == o.public().key_data());
        seen.push(o.public().clone());
        new
    });
    offers.truncate(MOST_KEYS);
    offers
}

fn public_half(file: &Path) -> Option<PublicKey> {
    let mut path = PathBuf::from(file).into_os_string();
    path.push(".pub");
    // allow:raw_read a key file in ~/.ssh, never a vault input
    PublicKey::from_openssh(std::fs::read_to_string(path).ok()?.trim()).ok()
}

fn offered(methods: &Option<MethodSet>, kind: MethodKind) -> bool {
    methods.as_ref().is_none_or(|m| m.contains(&kind))
}

/// Sign in as the person: their keys, then the password once, each only
/// while the server still offers that way in.
async fn sign_in(ssh: &mut Handle<Trust>, resolved: &Resolved, password: Option<&Password>) -> Result<(), Refusal> {
    let host = resolved.host.clone();
    let user = sftp::user(resolved)?;
    let failed = |e: String| Refusal::SignInFailed { host: host.clone(), reason: e };
    let mut methods: Option<MethodSet> = None;
    #[cfg(unix)]
    let mut agent = agent(resolved).await;
    #[cfg(unix)]
    let held: Vec<PublicKey> = match agent.as_mut() {
        Some(a) => a.request_identities().await.unwrap_or_default().into_iter().filter_map(|i| match i {
            russh::keys::agent::AgentIdentity::PublicKey { key, .. } => Some(key),
            _ => None,
        }).collect(),
        None => Vec::new(),
    };
    #[cfg(not(unix))]
    let held: Vec<PublicKey> = Vec::new();
    for offer in offers(&resolved.identity_files, resolved.identities_only, &held) {
        if !offered(&methods, MethodKind::PublicKey) {
            break;
        }
        let hash = match offer.public().algorithm().is_rsa() {
            true => ssh.best_supported_rsa_hash().await.ok().flatten().flatten().or(Some(HashAlg::Sha256)),
            false => None,
        };
        let result = match offer {
            Offer::File(key) => ssh.authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash)).await.map_err(|e| e.to_string()),
            #[cfg(unix)]
            Offer::Agent(public) => match agent.as_mut() {
                Some(a) => ssh.authenticate_publickey_with(user, public, hash, a).await.map_err(|e| e.to_string()),
                None => continue,
            },
            #[cfg(not(unix))]
            Offer::Agent(_) => continue,
        };
        match result.map_err(failed)? {
            russh::client::AuthResult::Success => return Ok(()),
            russh::client::AuthResult::Failure { remaining_methods, .. } => methods = Some(remaining_methods),
        }
    }
    let takes_password = offered(&methods, MethodKind::Password);
    let takes_prompt = offered(&methods, MethodKind::KeyboardInteractive);
    let Some(password) = password else {
        return Err(match takes_password || takes_prompt {
            true => Refusal::NeedsPassword { host },
            false => Refusal::KeysOnly { host },
        });
    };
    if takes_password {
        match ssh.authenticate_password(user, &password.0).await.map_err(|e| failed(e.to_string()))? {
            russh::client::AuthResult::Success => return Ok(()),
            russh::client::AuthResult::Failure { remaining_methods, .. } => methods = Some(remaining_methods),
        }
    }
    // Many servers turn password sign-in off and leave the same password on
    // through PAM. Only one hidden prompt is answered with it.
    if offered(&methods, MethodKind::KeyboardInteractive) {
        let mut reply = ssh.authenticate_keyboard_interactive_start(user, None).await.map_err(|e| failed(e.to_string()))?;
        let mut answered = false;
        loop {
            match reply {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure { .. } => break,
                KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } if prompts.is_empty() => {
                    reply = ssh.authenticate_keyboard_interactive_respond(Vec::new()).await.map_err(|e| failed(e.to_string()))?;
                }
                KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } if !answered && prompts.len() == 1 && !prompts[0].echo => {
                    answered = true;
                    reply = ssh.authenticate_keyboard_interactive_respond(vec![password.0.clone()]).await.map_err(|e| failed(e.to_string()))?;
                }
                KeyboardInteractiveAuthResponse::InfoRequest { .. } => break,
            }
        }
    }
    Err(Refusal::PasswordRefused { host })
}

/// Append `line` to `~/.ssh/authorized_keys` over SFTP, unless its key is
/// there already. The folder and file are made only when absent (0700,
/// 0600); an existing one keeps its mode.
async fn install(ssh: &Handle<Trust>, host: &str, line: &str) -> Result<(), Refusal> {
    let sftp = sftp::open_sftp(ssh, host).await?;
    sftp.init().await.map_err(|_| Refusal::NoSftp { host: host.to_string() })?;
    let failed = |e: russh_sftp::client::error::Error| Refusal::WriteFailed { path: format!("~/{AUTHORIZED_KEYS}"), reason: e.to_string() };
    if sftp.stat(".ssh").await.is_err() {
        sftp.mkdir(".ssh", FileAttributes { permissions: Some(0o700), ..Default::default() }).await.map_err(failed)?;
    }
    let existing = read(&sftp, AUTHORIZED_KEYS).await.map_err(failed)?;
    let key = line.split_whitespace().nth(2).unwrap_or_default();
    if String::from_utf8_lossy(&existing).lines().any(|l| l.split_whitespace().any(|w| w == key)) {
        return Ok(());
    }
    // A last line with no newline would fuse with moss's, breaking both.
    let lead = if existing.last().is_some_and(|b| *b != b'\n') { "\n" } else { "" };
    let flags = OpenFlags::WRITE | OpenFlags::APPEND | OpenFlags::CREATE;
    let attrs = FileAttributes { permissions: Some(0o600), ..Default::default() };
    let handle = sftp.open(AUTHORIZED_KEYS, flags, attrs).await.map_err(failed)?.handle;
    let written = sftp.write(handle.clone(), existing.len() as u64, format!("{lead}{line}\n").into_bytes()).await;
    let closed = sftp.close(handle).await;
    written.map_err(failed)?;
    closed.map_err(failed)?;
    Ok(())
}

/// The most of `authorized_keys` read: a larger one is refused, not cut,
/// since the append lands after what was read.
const MOST_BYTES: usize = 4 << 20;

/// A small file's bytes, or none when it is not there.
async fn read(sftp: &RawSftpSession, path: &str) -> Result<Vec<u8>, russh_sftp::client::error::Error> {
    let handle = match sftp.open(path, OpenFlags::READ, FileAttributes::default()).await {
        Ok(h) => h.handle,
        Err(russh_sftp::client::error::Error::Status(s)) if s.status_code == russh_sftp::protocol::StatusCode::NoSuchFile => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    let read = loop {
        match sftp.read(&handle, bytes.len() as u64, 32 * 1024).await {
            Ok(data) if bytes.len() + data.data.len() > MOST_BYTES => {
                break Err(russh_sftp::client::error::Error::UnexpectedBehavior(format!("{path} is over {} MiB", MOST_BYTES >> 20)));
            }
            Ok(data) if !data.data.is_empty() => bytes.extend_from_slice(&data.data),
            Ok(_) => break Ok(()),
            Err(russh_sftp::client::error::Error::Status(s)) if s.status_code == russh_sftp::protocol::StatusCode::Eof => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    let _ = sftp.close(handle).await;
    read.map(|()| bytes)
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;
