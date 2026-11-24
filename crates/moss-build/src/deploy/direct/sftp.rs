//! Your own server as a [`Destination`]: a folder on it, reached over SSH and
//! written with SFTP.
//!
//! Every way a connection can fail is a [`Refusal`] whose text is the one
//! sentence a person needs, so the check and a failed publish say the same
//! thing.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::{error::Error as SftpError, RawSftpSession};
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};

use super::destination::{Body, Destination};
use super::openssh::{Known, Resolved, Ssh};
use super::server::Server;

/// How long a server has to answer before it counts as not answering.
const ANSWER_WITHIN: Duration = Duration::from_secs(15);
/// Bytes per write request, and how many are in flight at once: one request
/// per round trip would make a far server crawl.
const CHUNK: usize = 32 * 1024;
const IN_FLIGHT: usize = 16;
/// The host keys moss accepts. russh offers RSA even built without it, and
/// then fails the signature; left out, an RSA-only server is named as one.
const HOST_KEYS: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP384 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP521 },
];
/// The OpenSSH extension that renames over an existing file in one step.
const POSIX_RENAME: &str = "posix-rename@openssh.com";

/// Why moss could not reach, trust, sign in to or write to a server. Typed
/// on the wire, so each surface words it in its own language.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
// The bindings already carry the plugin registry's `Refusal`.
#[specta(rename = "ServerRefusal")]
pub enum Refusal {
    NoSuchServer { host: String },
    NoAnswer { host: String, port: u16 },
    PortRefused { host: String, port: u16 },
    /// Seen for the first time: the person decides whether to trust it.
    UnknownHostKey { host: String, fingerprint: String },
    HostKeyChanged { host: String, was: String, now: String },
    /// This computer's `known_hosts` lists another key for the server: what
    /// `ssh` refuses, so moss refuses it too, with no way to trust it here.
    HostKeyDiffers { host: String, fingerprint: String },
    HostKeyRevoked { host: String },
    RsaOnly { host: String },
    KeyRefused { host: String },
    /// No user given, and no `ssh` on this computer to name one.
    NoUser { host: String },
    /// Connecting: the person's ssh keys didn't sign in, and the server
    /// takes a password.
    NeedsPassword { host: String },
    PasswordRefused { host: String },
    /// Connecting: nothing the person has signs in, and the server takes
    /// no password, so moss's key line is added by hand.
    KeysOnly { host: String },
    /// Signing in failed for a reason other than a refused key.
    SignInFailed { host: String, reason: String },
    /// Signed in, but the server runs no SFTP.
    NoSftp { host: String },
    NoFolder { folder: String },
    /// No folder given, and none of the usual web folders is on the server.
    NoWebFolder { host: String },
    /// No folder given, and several web folders take files but none shows
    /// at the address: the person picks one.
    WhichFolder { host: String },
    CannotWrite { folder: String },
    /// The folder takes files, but the address doesn't serve them (yet).
    NotServed { address: String },
    /// A write in the folder failed; `reason` is the server's own words.
    WriteFailed { path: String, reason: String },
    /// The server cannot replace a file in one step, so a visitor could meet
    /// a missing file mid-publish.
    NoAtomicRename { host: String },
    /// The connection broke before moss could sign in.
    Broken { host: String, reason: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NoSuchServer { host } => write!(f, "No server named {host}."),
            Refusal::NoAnswer { host, port } => write!(f, "{host} didn't answer on port {port}. Is the port right?"),
            Refusal::PortRefused { host, port } => write!(f, "{host} refused port {port}."),
            Refusal::UnknownHostKey { host, fingerprint } => write!(f, "{host}'s key is {fingerprint}."),
            Refusal::HostKeyChanged { host, was, now } => write!(
                f,
                "{host}'s key changed (was {was}, now {now}). Trust the new key only if you rebuilt the server."
            ),
            Refusal::HostKeyDiffers { host, fingerprint } => {
                write!(f, "{host}'s key ({fingerprint}) differs from the one this computer's ssh knows.")
            }
            Refusal::HostKeyRevoked { host } => write!(f, "{host}'s key is revoked in this computer's ssh."),
            Refusal::RsaOnly { host } => write!(
                f,
                "{host} only offers an RSA key, which moss doesn't use. Add an ed25519 host key to the server."
            ),
            Refusal::KeyRefused { host } => write!(f, "{host} refused moss's key."),
            Refusal::NoUser { host } => write!(f, "Who signs in to {host}? Add the user, like deploy@{host}."),
            Refusal::NeedsPassword { host } => write!(f, "{host} didn't take your ssh keys."),
            Refusal::PasswordRefused { host } => write!(f, "{host} refused the password."),
            Refusal::KeysOnly { host } => write!(f, "{host} didn't take your ssh keys, and takes no password."),
            Refusal::SignInFailed { host, reason } => write!(f, "Couldn't sign in to {host}: {reason}"),
            Refusal::NoSftp { host } => write!(f, "Signed in, but {host} doesn't offer SFTP."),
            Refusal::NoFolder { folder } => write!(f, "Signed in, but there is no folder {folder}."),
            Refusal::NoWebFolder { host } => write!(f, "No web folder found on {host}. Add its path after the server, like deploy@{host}:/var/www/html."),
            Refusal::WhichFolder { host } => write!(f, "Which folder on {host} holds the site?"),
            Refusal::NotServed { address } => write!(f, "{address} doesn't show this folder yet."),
            Refusal::CannotWrite { folder } => write!(f, "Signed in, but can't write to {folder}."),
            Refusal::WriteFailed { path, reason } => write!(f, "write {path}: {reason}"),
            Refusal::NoAtomicRename { host } => write!(
                f,
                "{host} can't replace a file in one step (its SFTP server lacks {POSIX_RENAME}), so moss won't publish there."
            ),
            Refusal::Broken { host, reason } => write!(f, "Couldn't talk to {host}: {reason}"),
        }
    }
}

/// Why a server's key was not trusted. Only a plain key has a fingerprint
/// worth trusting; without one there is nothing to pin.
pub(super) fn untrusted(host: String, pinned: Option<&str>, seen: Seen) -> Refusal {
    let Some(now) = seen.fingerprint else {
        return Refusal::Broken { host, reason: "it showed no plain host key".into() };
    };
    match (seen.known, pinned) {
        (Some(Known::Differs), _) => Refusal::HostKeyDiffers { host, fingerprint: now },
        (Some(Known::Revoked), _) => Refusal::HostKeyRevoked { host },
        (_, None) => Refusal::UnknownHostKey { host, fingerprint: now },
        (_, Some(was)) => Refusal::HostKeyChanged { host, was: was.to_string(), now },
    }
}

/// The key a server showed, and what this computer's ssh says of it.
#[derive(Default, Clone)]
pub(super) struct Seen {
    pub fingerprint: Option<String>,
    pub known: Option<Known>,
}

/// Trusts a server's host key when `known_hosts` lists it, or when it is the
/// one the folder pinned and `known_hosts` does not list another. A key
/// `known_hosts` lists differently or revokes is refused whatever the pin
/// says, since `ssh` would refuse it.
pub(super) struct Trust {
    pinned: Option<String>,
    ssh: Ssh,
    resolved: Resolved,
    seen: Arc<Mutex<Seen>>,
}

impl russh::client::Handler for Trust {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return Ok(false);
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let (ssh, resolved, shown) = (self.ssh.clone(), self.resolved.clone(), key.clone());
        let known = tokio::task::spawn_blocking(move || ssh.known(&resolved, &shown)).await.unwrap_or(Known::New);
        let trusted = match known {
            Known::Listed => true,
            Known::Differs | Known::Revoked => false,
            Known::New => self.pinned.as_deref() == Some(fingerprint.as_str()),
        };
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Seen { fingerprint: Some(fingerprint), known: Some(known) };
        Ok(trusted)
    }
}

/// Where web servers usually serve sites from; a relative path is under the
/// home folder, `{host}` is the server's name.
pub(super) const WEB_FOLDERS: &[&str] =
    &["public_html", "www", "htdocs", "domains/{host}/public_html", "/var/www/{host}", "/var/www/html", "/srv/www"];

/// A folder on a server moss has signed in to.
pub struct SftpDestination {
    sftp: RawSftpSession,
    /// The folder's real path on the server, with no trailing `/`.
    root: String,
    /// Folders already found to lie inside `root`.
    inside: Mutex<HashSet<String>>,
    _ssh: russh::client::Handle<Trust>,
}

/// A server reached and its host key trusted, before anyone signs in.
pub(super) struct Session {
    pub ssh: russh::client::Handle<Trust>,
    /// The fingerprint of the host key that was trusted.
    pub host_key: String,
}

/// `server` as this computer's `ssh` would reach it, resolved once for a
/// whole check, connect or publish.
pub(super) async fn resolve(ssh_config: &Ssh, server: &Server) -> Result<Resolved, Refusal> {
    let (ssh_config, given) = (ssh_config.clone(), server.clone());
    let broken = |reason: String| Refusal::Broken { host: server.host.clone(), reason };
    tokio::task::spawn_blocking(move || ssh_config.resolve(&given)).await.map_err(|e| broken(e.to_string()))?.map_err(broken)
}

/// Reach the resolved server and trust its key. `host_key` is the
/// fingerprint the folder pinned, if any.
pub(super) async fn session(ssh_config: &Ssh, resolved: &Resolved, host_key: Option<&str>) -> Result<Session, Refusal> {
    let host = resolved.host.clone();
    let tcp = reach(&host, resolved.port).await?;
    let seen = Arc::new(Mutex::new(Seen::default()));
    let kinds = {
        let (ssh_config, resolved) = (ssh_config.clone(), resolved.clone());
        tokio::task::spawn_blocking(move || ssh_config.known_kinds(&resolved)).await.unwrap_or_default()
    };
    let trust = Trust { pinned: host_key.map(str::to_string), ssh: ssh_config.clone(), resolved: resolved.clone(), seen: seen.clone() };
    let config = russh::client::Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        preferred: russh::Preferred { key: std::borrow::Cow::Owned(host_keys(&kinds)), ..russh::Preferred::DEFAULT },
        ..Default::default()
    };
    let ssh = match russh::client::connect_stream(Arc::new(config), tcp, trust).await {
        Ok(ssh) => ssh,
        Err(russh::Error::UnknownKey) => {
            return Err(untrusted(host, host_key, seen.lock().unwrap_or_else(|p| p.into_inner()).clone()));
        }
        Err(russh::Error::NoCommonAlgo { kind: russh::AlgorithmKind::Key, theirs, .. })
            if theirs.iter().all(|a| a.contains("rsa")) =>
        {
            return Err(Refusal::RsaOnly { host })
        }
        Err(e) => return Err(Refusal::Broken { host, reason: e.to_string() }),
    };
    let host_key = seen.lock().unwrap_or_else(|p| p.into_inner()).fingerprint.clone().unwrap_or_default();
    Ok(Session { ssh, host_key })
}

/// The user to sign in as. Without OpenSSH nothing fills it in, and a
/// sign-in as nobody would read as a refused key.
pub(super) fn user(resolved: &Resolved) -> Result<&str, Refusal> {
    match resolved.user.as_str() {
        "" => Err(Refusal::NoUser { host: resolved.host.clone() }),
        user => Ok(user),
    }
}

/// [`HOST_KEYS`], with the types this computer's ssh already knows the server
/// by first, so a server with several keys shows the one that can match.
fn host_keys(known: &[String]) -> Vec<Algorithm> {
    let mut order: Vec<Algorithm> = HOST_KEYS.iter().filter(|a| known.iter().any(|k| k == a.as_str())).cloned().collect();
    let rest: Vec<Algorithm> = HOST_KEYS.iter().filter(|a| !order.contains(a)).cloned().collect();
    order.extend(rest);
    order
}

/// Open SFTP on a signed-in session.
pub(super) async fn open_sftp(ssh: &russh::client::Handle<Trust>, host: &str) -> Result<RawSftpSession, Refusal> {
    let no_sftp = || Refusal::NoSftp { host: host.to_string() };
    let channel = ssh.channel_open_session().await.map_err(|_| no_sftp())?;
    channel.request_subsystem(true, "sftp").await.map_err(|_| no_sftp())?;
    let sftp = RawSftpSession::new(channel.into_stream());
    sftp.set_timeout(ANSWER_WITHIN.as_secs());
    Ok(sftp)
}

/// Each step of reaching a server with moss's key, run in order; the first
/// that fails is the refusal. Returns, alongside, the fingerprint of the host
/// key trusted on the way, whatever happened after.
pub async fn connect(
    ssh_config: &Ssh,
    resolved: &Resolved,
    folder: &str,
    host_key: Option<&str>,
    key: Arc<PrivateKey>,
) -> (Option<String>, Result<SftpDestination, Refusal>) {
    let Session { mut ssh, host_key } = match session(ssh_config, resolved, host_key).await {
        Ok(session) => session,
        Err(refusal) => return (None, Err(refusal)),
    };
    (Some(host_key), open_destination(&mut ssh, resolved, folder, key).await.map(|(sftp, root)| SftpDestination {
        sftp,
        root,
        inside: Mutex::new(HashSet::new()),
        _ssh: ssh,
    }))
}

/// Sign in with moss's key and open `folder`, or the home folder when none
/// is given.
async fn open_destination(
    ssh: &mut russh::client::Handle<Trust>,
    resolved: &Resolved,
    folder: &str,
    key: Arc<PrivateKey>,
) -> Result<(RawSftpSession, String), Refusal> {
    let host = resolved.host.clone();
    let signed_in = ssh
        .authenticate_publickey(user(resolved)?, PrivateKeyWithHashAlg::new(key, None))
        .await
        .map_err(|e| Refusal::SignInFailed { host: host.clone(), reason: e.to_string() })?;
    if !signed_in.success() {
        return Err(Refusal::KeyRefused { host });
    }
    let sftp = open_sftp(ssh, &host).await?;
    let version = sftp.init().await.map_err(|_| Refusal::NoSftp { host: host.clone() })?;
    if !version.extensions.contains_key(POSIX_RENAME) {
        return Err(Refusal::NoAtomicRename { host });
    }
    // OpenSSH answers realpath for a path that is not there.
    let no_folder = || Refusal::NoFolder { folder: folder.to_string() };
    let wanted = if folder.is_empty() { "." } else { folder };
    let root = match sftp.realpath(wanted).await {
        Ok(name) => name.files.into_iter().next().map(|f| f.filename).unwrap_or_default(),
        Err(_) => return Err(no_folder()),
    };
    if !sftp.stat(&root).await.is_ok_and(|attrs| attrs.attrs.is_dir()) {
        return Err(no_folder());
    }
    Ok((sftp, root.trim_end_matches('/').to_string()))
}

async fn reach(host: &str, port: u16) -> Result<tokio::net::TcpStream, Refusal> {
    let addrs: Vec<_> = match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => addrs.collect(),
        Err(_) => return Err(Refusal::NoSuchServer { host: host.to_string() }),
    };
    if addrs.is_empty() {
        return Err(Refusal::NoSuchServer { host: host.to_string() });
    }
    match tokio::time::timeout(ANSWER_WITHIN, tokio::net::TcpStream::connect(&addrs[..])).await {
        Err(_) => Err(Refusal::NoAnswer { host: host.to_string(), port }),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Err(Refusal::PortRefused { host: host.to_string(), port })
        }
        Ok(Err(_)) => Err(Refusal::NoAnswer { host: host.to_string(), port }),
        Ok(Ok(tcp)) => Ok(tcp),
    }
}

fn is_missing(e: &SftpError) -> bool {
    matches!(e, SftpError::Status(s) if s.status_code == StatusCode::NoSuchFile)
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

impl SftpDestination {
    /// Of `candidates`, the folders on the server, as full paths, in order.
    pub async fn folders(&self, candidates: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        for candidate in candidates {
            let Ok(real) = self.sftp.realpath(candidate).await else { continue };
            let Some(path) = real.files.into_iter().next().map(|f| f.filename) else { continue };
            if !found.contains(&path) && self.sftp.stat(&path).await.is_ok_and(|a| a.attrs.is_dir()) {
                found.push(path);
            }
        }
        found
    }

    /// Write into `folder` from now on.
    pub fn move_to(&mut self, folder: &str) {
        self.root = folder.trim_end_matches('/').to_string();
        self.inside = Mutex::new(HashSet::new());
    }

    /// Write a file under a temporary name, then rename it over the live one,
    /// so a visitor never meets a half written file. A refusal stays typed for
    /// the check, which words it in each surface's language.
    pub async fn put(&self, path: &str, body: Body) -> Result<(), Refusal> {
        let (dir, name) = split(path);
        self.make_dirs(dir).await.map_err(|e| Refusal::WriteFailed { path: path.into(), reason: e.to_string() })?;
        let full = self.full(path);
        let temp = self.full(&format!("{}{}.{name}.moss-tmp-{}", dir, if dir.is_empty() { "" } else { "/" }, std::process::id()));
        let written = match &body {
            Body::File(_) | Body::Bytes(_) => self.write_file(&temp, &body).await,
            // The SFTP draft sends the link path first; OpenSSH reads the
            // target first, so the arguments go in OpenSSH's order.
            Body::Link(target) => self.sftp.symlink(target.as_str(), temp.as_str()).await.map(|_| ()),
        };
        let landed = match written {
            Ok(()) => self.rename_over(&temp, &full).await,
            failed => failed,
        };
        match landed {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.sftp.remove(temp).await;
                Err(match e {
                    SftpError::Status(s) if s.status_code == StatusCode::PermissionDenied => {
                        Refusal::CannotWrite { folder: self.full(dir) }
                    }
                    e => Refusal::WriteFailed { path: path.into(), reason: e.to_string() },
                })
            }
        }
    }

    /// The folder's real path, as the server named it.
    pub fn root(&self) -> &str {
        &self.root
    }

    fn full(&self, path: &str) -> String {
        if path.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{path}", self.root)
        }
    }

    /// Refuse `dir` (relative) when its real path on the server is outside
    /// the root: a link inside the destination can lead out of it.
    async fn check_inside(&self, dir: &str) -> Result<(), SftpError> {
        if dir.is_empty() || self.inside.lock().unwrap_or_else(|p| p.into_inner()).contains(dir) {
            return Ok(());
        }
        let real = self.sftp.realpath(self.full(dir)).await?;
        let real = real.files.into_iter().next().map(|f| f.filename).unwrap_or_default();
        if real != self.root && !real.starts_with(&format!("{}/", self.root)) {
            return Err(SftpError::UnexpectedBehavior(format!("{dir} leads outside {}", self.root)));
        }
        self.inside.lock().unwrap_or_else(|p| p.into_inner()).insert(dir.to_string());
        Ok(())
    }

    /// Create `dir` and its parents, each checked to stay inside the root.
    async fn make_dirs(&self, dir: &str) -> Result<(), SftpError> {
        let mut at = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            if !at.is_empty() {
                at.push('/');
            }
            at.push_str(part);
            if self.inside.lock().unwrap_or_else(|p| p.into_inner()).contains(&at) {
                continue;
            }
            // Already there is the common case; whatever else went wrong
            // shows at the realpath or the write that follows.
            let _ = self.sftp.mkdir(self.full(&at), FileAttributes::default()).await;
            self.check_inside(&at).await?;
        }
        Ok(())
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<(), SftpError> {
        let mut data = Vec::with_capacity(from.len() + to.len() + 8);
        for s in [from, to] {
            data.extend_from_slice(&(s.len() as u32).to_be_bytes());
            data.extend_from_slice(s.as_bytes());
        }
        match self.sftp.extended(POSIX_RENAME, data).await? {
            russh_sftp::protocol::Packet::Status(s) if s.status_code == StatusCode::Ok => Ok(()),
            russh_sftp::protocol::Packet::Status(s) => Err(SftpError::Status(s)),
            _ => Err(SftpError::UnexpectedPacket),
        }
    }

    async fn write_file(&self, temp: &str, body: &Body) -> Result<(), SftpError> {
        let reader: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match body {
            // allow:raw_read built output being uploaded — dataless is absent
            Body::File(local) => Box::new(tokio::fs::File::open(local).await?),
            Body::Bytes(data) => Box::new(std::io::Cursor::new(data.clone())),
            Body::Link(_) => unreachable!("a link is not written as a file"),
        };
        let attrs = FileAttributes { permissions: Some(0o644), ..Default::default() };
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        let handle = self.sftp.open(temp, flags, attrs).await?.handle;
        let written = self.write_all(&handle, reader).await;
        let closed = self.sftp.close(handle).await;
        written?;
        closed.map(|_| ())
    }

    /// Send the reader's bytes, `IN_FLIGHT` requests at a time.
    async fn write_all(&self, handle: &str, mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>) -> Result<(), SftpError> {
        use tokio::io::AsyncReadExt;
        let mut pending = futures::stream::FuturesUnordered::new();
        let mut offset = 0u64;
        loop {
            let mut chunk = vec![0; CHUNK];
            let mut filled = 0;
            while filled < CHUNK {
                match reader.read(&mut chunk[filled..]).await? {
                    0 => break,
                    n => filled += n,
                }
            }
            if filled == 0 {
                break;
            }
            chunk.truncate(filled);
            pending.push(self.sftp.write(handle.to_string(), offset, chunk));
            offset += filled as u64;
            if pending.len() >= IN_FLIGHT {
                if let Some(done) = pending.next().await {
                    done?;
                }
            }
        }
        while let Some(done) = pending.next().await {
            done?;
        }
        Ok(())
    }

    /// Remove a file, and the folders that removing it emptied.
    pub(super) async fn remove(&self, path: &str) -> Result<(), SftpError> {
        let (dir, _) = split(path);
        match self.check_inside(dir).await {
            Err(e) if is_missing(&e) => return Ok(()),
            other => other?,
        }
        match self.sftp.remove(self.full(path)).await {
            Ok(_) => {}
            Err(e) if is_missing(&e) => return Ok(()),
            Err(e) => return Err(e),
        }
        // Folders the deletion emptied go too, up to the root; the server
        // refuses to remove one with anything left in it.
        let mut dir = dir;
        while !dir.is_empty() {
            if self.sftp.rmdir(self.full(dir)).await.is_err() {
                break;
            }
            self.inside.lock().unwrap_or_else(|p| p.into_inner()).remove(dir);
            dir = split(dir).0;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Destination for SftpDestination {
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        match self.check_inside(split(path).0).await {
            Err(e) if is_missing(&e) => return Ok(None),
            other => other.map_err(|e| format!("read {path}: {e}"))?,
        }
        let handle = match self.sftp.open(self.full(path), OpenFlags::READ, FileAttributes::default()).await {
            Ok(h) => h.handle,
            Err(e) if is_missing(&e) => return Ok(None),
            Err(e) => return Err(format!("read {path}: {e}")),
        };
        let mut bytes = Vec::new();
        let read = loop {
            match self.sftp.read(&handle, bytes.len() as u64, CHUNK as u32).await {
                Ok(data) => bytes.extend_from_slice(&data.data),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break Ok(()),
                Err(e) => break Err(format!("read {path}: {e}")),
            }
        };
        let _ = self.sftp.close(handle).await;
        read.map(|()| Some(bytes))
    }

    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String> {
        self.put(path, body).await.map_err(|r| r.to_string())
    }

    async fn delete(&self, path: &str) -> Result<(), String> {
        self.remove(path).await.map_err(|e| format!("delete {path}: {e}"))
    }
}
